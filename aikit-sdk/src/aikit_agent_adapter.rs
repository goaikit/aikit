use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::Arc;

use aikit_agent::agent_definition::AgentPersona;
use aikit_agent::context::{ContextToolCall, ContextToolResult};
use aikit_agent::llm::openai_compat::OpenAiCompatProvider;
use aikit_agent::{AgentConfig, AgentInternalEvent, HostToolProvider, LlmGateway, Turn};

use crate::session_store::{
    now_rfc3339, SessionFile, SessionStore, SessionStoreError, SessionToolCall, SessionToolResult,
    SessionTurn,
};
use crate::{
    AgentEvent, AgentEventPayload, AgentEventStream, QuotaCategory, QuotaExceededInfo, RunError,
    RunOptions, RunResult, TokenUsage, UsageSource,
};

#[cfg(unix)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(code << 8)
}

#[cfg(windows)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    ExitStatus::from_raw(code as u32)
}

pub fn run_aikit_agent<F>(
    prompt: &str,
    options: &RunOptions,
    host_tool_provider: Option<Arc<dyn HostToolProvider>>,
    mut on_event: F,
) -> Result<RunResult, RunError>
where
    F: FnMut(AgentEvent) + Send,
{
    let workdir = options
        .current_dir
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let mut config = AgentConfig::from_env(workdir, options.stream, options.model.clone())
        .map_err(|e| emit_error(prompt, options, &mut on_event, e.to_string()))?;
    config.host_tool_provider = host_tool_provider;

    apply_session_options(options, &mut config);

    let gateway = OpenAiCompatProvider::new(config.timeout_secs, config.connect_timeout_secs)
        .map_err(|e| emit_error(prompt, options, &mut on_event, e.to_string()))?;

    run_with_config_and_gateway(
        prompt,
        options,
        config,
        Box::new(gateway),
        SessionStore::open(),
        &mut on_event,
    )
}

/// Run the builtin aikit agent with an externally-supplied LLM gateway.
///
/// Intended for integration tests that need to inject a mock gateway. In
/// production code, use [`run_aikit_agent`] which creates the gateway from
/// environment configuration.
pub fn run_aikit_agent_with_gateway<F>(
    prompt: &str,
    options: &RunOptions,
    gateway: Box<dyn LlmGateway>,
    store: Option<SessionStore>,
    mut on_event: F,
) -> Result<RunResult, RunError>
where
    F: FnMut(AgentEvent) + Send,
{
    let workdir = options
        .current_dir
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let mut config = config_for_injected_gateway(workdir, options);

    apply_session_options(options, &mut config);

    let resolved_store = store.unwrap_or_else(SessionStore::open);
    run_with_config_and_gateway(
        prompt,
        options,
        config,
        gateway,
        resolved_store,
        &mut on_event,
    )
}

fn config_for_injected_gateway(workdir: PathBuf, options: &RunOptions) -> AgentConfig {
    AgentConfig::from_env(workdir.clone(), options.stream, options.model.clone()).unwrap_or_else(
        |_| AgentConfig {
            model: options
                .model
                .clone()
                .or_else(|| std::env::var("AIKIT_MODEL").ok())
                .unwrap_or_else(|| "gpt-4o".to_string()),
            base_url: std::env::var("AIKIT_LLM_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
            api_key: "injected-gateway".to_string(),
            stream: options.stream
                || std::env::var("AIKIT_STREAM")
                    .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                    .unwrap_or(false),
            max_iterations: std::env::var("AIKIT_MAX_ITERATIONS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10u32),
            max_subagent_depth: std::env::var("AIKIT_MAX_SUBAGENT_DEPTH")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2u32),
            context_budget_tokens: std::env::var("AIKIT_CONTEXT_BUDGET_TOKENS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(12000u64),
            allowed_roots: vec![workdir.clone()],
            skills_dirs: Vec::new(),
            agents_md_path: agents_md_path(&workdir),
            workdir,
            timeout_secs: 60,
            connect_timeout_secs: 10,
            session_persona: None,
            session_agents: std::collections::HashMap::new(),
            host_tool_provider: None,
        },
    )
}

fn agents_md_path(workdir: &std::path::Path) -> Option<PathBuf> {
    let agents_md = workdir.join("AGENTS.md");
    let claude_md = workdir.join("CLAUDE.md");
    if agents_md.exists() {
        Some(agents_md)
    } else if claude_md.exists() {
        Some(claude_md)
    } else {
        None
    }
}

fn apply_session_options(options: &RunOptions, config: &mut AgentConfig) {
    // Apply session persona: deserialize from JSON, then apply model override if no CLI --model.
    if let Some(ref persona_val) = options.session_persona {
        match serde_json::from_value::<AgentPersona>(persona_val.clone()) {
            Ok(persona) => {
                // Apply persona model only when CLI did not specify --model.
                if options.model.is_none() {
                    if let Some(ref m) = persona.model {
                        config.model = m.clone();
                    }
                }
                config.session_persona = Some(persona);
            }
            Err(e) => {
                tracing::warn!("failed to deserialize session persona: {}", e);
            }
        }
    }

    // Apply session agents: deserialize each entry from JSON.
    for (key, agent_val) in &options.session_agents {
        match serde_json::from_value::<AgentPersona>(agent_val.clone()) {
            Ok(persona) => {
                config.session_agents.insert(key.clone(), persona);
            }
            Err(e) => {
                tracing::warn!("failed to deserialize session agent '{}': {}", key, e);
            }
        }
    }
}

fn run_with_config_and_gateway<F>(
    prompt: &str,
    options: &RunOptions,
    config: AgentConfig,
    gateway: Box<dyn LlmGateway>,
    store: SessionStore,
    on_event: &mut F,
) -> Result<RunResult, RunError>
where
    F: FnMut(AgentEvent) + Send,
{
    let cwd = options
        .current_dir
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let cwd_str = cwd.to_string_lossy().into_owned();

    if let Some(ref session_id) = options.session_id {
        // Resume existing session.
        let session = store.load(session_id).map_err(|e| match e {
            SessionStoreError::NotFound(id) => RunError::SessionNotFound(id),
            SessionStoreError::Parse { id, reason } => RunError::SessionLoadFailed { id, reason },
            SessionStoreError::Io(io_err) => RunError::SessionLoadFailed {
                id: session_id.clone(),
                reason: io_err.to_string(),
            },
        })?;

        emit_session_started(on_event, &session.session_id);

        let prior_turns = session_turns_to_turns(&session.turns);

        // Forward each event to `on_event` the instant the agent loop produces it (BUG-6):
        // `collected` mirrors the stream purely so the full transcript is available for
        // session persistence once the run finishes — it is never used to (re)drive
        // `on_event`, so there is no end-of-run replay and no unbounded unread buffer.
        let mut collected: Vec<AgentInternalEvent> = Vec::new();
        let mut state = EmitState::new();
        let mut callback_panic: Option<Box<dyn std::any::Any + Send>> = None;
        let run_result = aikit_agent::run_with_context_streaming(
            config,
            prior_turns,
            prompt,
            gateway,
            |event| {
                collected.push(event.clone());
                if callback_panic.is_none() {
                    if let Err(RunError::CallbackPanic(panic)) = state.emit(event, on_event) {
                        callback_panic = Some(panic);
                    }
                }
            },
        );

        match run_result {
            Ok(()) => {
                let new_turns = internal_events_to_turns(prompt, &collected);
                let mut updated_session = session;
                updated_session.turns.extend(new_turns);
                updated_session.updated_at = now_rfc3339();
                let _ = store.save(&updated_session);
                let _ = store.update_index(&cwd_str, &updated_session.session_id);
                if let Some(panic) = callback_panic {
                    return Err(RunError::CallbackPanic(panic));
                }
                Ok(state.finish(true))
            }
            Err(err) => {
                if callback_panic.is_none() && !state.had_error_event {
                    // The run loop pushes an `Error` event onto the sink before returning
                    // `Err` for almost every failure path; this only fires for the rare
                    // gap (e.g. context-compression failure) where no event preceded it,
                    // so a client always gets at least one error frame.
                    let _ = state.emit(
                        AgentInternalEvent::Error {
                            code: error_code(&err.to_string()),
                            message: err.to_string(),
                        },
                        on_event,
                    );
                }
                if let Some(panic) = callback_panic {
                    return Err(RunError::CallbackPanic(panic));
                }
                let mut result = state.finish(false);
                result.stderr = err.to_string().into_bytes();
                Ok(result)
            }
        }
    } else {
        // New session: generate UUID, run, save.
        let session_id = uuid::Uuid::new_v4().to_string();
        let now = now_rfc3339();

        emit_session_started(on_event, &session_id);

        let mut collected: Vec<AgentInternalEvent> = Vec::new();
        let mut state = EmitState::new();
        let mut callback_panic: Option<Box<dyn std::any::Any + Send>> = None;
        let run_result = aikit_agent::run_streaming(config, prompt, gateway, |event| {
            collected.push(event.clone());
            if callback_panic.is_none() {
                if let Err(RunError::CallbackPanic(panic)) = state.emit(event, on_event) {
                    callback_panic = Some(panic);
                }
            }
        });

        match run_result {
            Ok(()) => {
                let turns = internal_events_to_turns(prompt, &collected);
                let session = SessionFile {
                    session_id: session_id.clone(),
                    agent: "aikit".to_string(),
                    created_at: now.clone(),
                    updated_at: now,
                    cwd: cwd_str.clone(),
                    turns,
                };
                let _ = store.save(&session);
                let _ = store.update_index(&cwd_str, &session_id);
                if let Some(panic) = callback_panic {
                    return Err(RunError::CallbackPanic(panic));
                }
                let mut result = state.finish(true);
                let session_line = format!("Session: {}\n", session_id);
                result.stderr.extend_from_slice(session_line.as_bytes());
                Ok(result)
            }
            Err(err) => {
                if callback_panic.is_none() && !state.had_error_event {
                    let _ = state.emit(
                        AgentInternalEvent::Error {
                            code: error_code(&err.to_string()),
                            message: err.to_string(),
                        },
                        on_event,
                    );
                }
                if let Some(panic) = callback_panic {
                    return Err(RunError::CallbackPanic(panic));
                }
                let mut result = state.finish(false);
                result.stderr = err.to_string().into_bytes();
                Ok(result)
            }
        }
    }
}

fn internal_events_to_turns(prompt: &str, events: &[AgentInternalEvent]) -> Vec<SessionTurn> {
    let mut turns: Vec<SessionTurn> = Vec::new();

    turns.push(SessionTurn {
        role: "user".to_string(),
        content: prompt.to_string(),
        tool_calls: None,
        tool_results: None,
    });

    let mut current_assistant: Option<SessionTurn> = None;
    let mut pending_tool_calls: Vec<SessionToolCall> = Vec::new();
    let mut pending_tool_results: Vec<SessionToolResult> = Vec::new();

    for event in events {
        match event {
            AgentInternalEvent::TextFinal { content, .. } => {
                // Flush any pending tool results from previous step
                if !pending_tool_results.is_empty() {
                    if let Some(asst) = current_assistant.take() {
                        turns.push(asst);
                    }
                    turns.push(SessionTurn {
                        role: "tool".to_string(),
                        content: String::new(),
                        tool_calls: None,
                        tool_results: Some(std::mem::take(&mut pending_tool_results)),
                    });
                }
                current_assistant = Some(SessionTurn {
                    role: "assistant".to_string(),
                    content: content.clone(),
                    tool_calls: None,
                    tool_results: None,
                });
                pending_tool_calls.clear();
            }
            AgentInternalEvent::ToolUse {
                tool_name,
                tool_input,
                call_id,
            } => {
                pending_tool_calls.push(SessionToolCall {
                    id: call_id.clone(),
                    name: tool_name.clone(),
                    input: tool_input.to_string(),
                });
            }
            AgentInternalEvent::ToolResult {
                call_id, output, ..
            } => {
                pending_tool_results.push(SessionToolResult {
                    tool_call_id: call_id.clone(),
                    name: String::new(),
                    output: output.clone(),
                });
            }
            AgentInternalEvent::StepFinish { .. } => {
                if !pending_tool_calls.is_empty() {
                    if let Some(ref mut asst) = current_assistant {
                        asst.tool_calls = Some(std::mem::take(&mut pending_tool_calls));
                    }
                    if !pending_tool_results.is_empty() {
                        if let Some(asst) = current_assistant.take() {
                            turns.push(asst);
                        }
                        turns.push(SessionTurn {
                            role: "tool".to_string(),
                            content: String::new(),
                            tool_calls: None,
                            tool_results: Some(std::mem::take(&mut pending_tool_results)),
                        });
                    } else if let Some(asst) = current_assistant.take() {
                        turns.push(asst);
                    }
                } else if let Some(asst) = current_assistant.take() {
                    turns.push(asst);
                }
            }
            _ => {}
        }
    }

    // Flush any remaining assistant turn
    if let Some(asst) = current_assistant {
        turns.push(asst);
    }

    turns
}

fn session_turns_to_turns(session_turns: &[SessionTurn]) -> Vec<Turn> {
    session_turns
        .iter()
        .map(|st| match st.role.as_str() {
            "assistant" => {
                if let Some(ref calls) = st.tool_calls {
                    let ctx_calls: Vec<ContextToolCall> = calls
                        .iter()
                        .map(|c| ContextToolCall {
                            id: c.id.clone(),
                            name: c.name.clone(),
                            arguments: c.input.clone(),
                        })
                        .collect();
                    Turn::assistant_with_tool_calls(&st.content, ctx_calls)
                } else {
                    Turn::assistant(&st.content)
                }
            }
            "tool" => {
                let results = st
                    .tool_results
                    .as_ref()
                    .map(|rs| {
                        rs.iter()
                            .map(|r| ContextToolResult {
                                call_id: r.tool_call_id.clone(),
                                output: r.output.clone(),
                                is_error: false,
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Turn::tool_result(results)
            }
            _ => Turn::user(&st.content),
        })
        .collect()
}

fn emit_session_started<F>(on_event: &mut F, session_id: &str)
where
    F: FnMut(AgentEvent) + Send,
{
    let event = AgentEvent {
        agent_key: "aikit".to_string(),
        seq: 0,
        stream: AgentEventStream::Stdout,
        payload: AgentEventPayload::SessionStarted {
            session_id: session_id.to_string(),
        },
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        on_event(event);
    }));
}

fn emit_error<F>(_prompt: &str, options: &RunOptions, on_event: &mut F, message: String) -> RunError
where
    F: FnMut(AgentEvent) + Send,
{
    let _ = emit_events(
        vec![AgentInternalEvent::Error {
            code: error_code(&message),
            message,
        }],
        options,
        on_event,
        false,
    );
    RunError::SpawnFailed(std::io::Error::other(
        "built-in aikit agent failed to start",
    ))
}

/// Accumulates the pieces of a [`RunResult`] (stdout/stderr bytes, token usage, quota
/// info, and the outbound `seq` counter) across a stream of [`AgentInternalEvent`]s that
/// arrive one at a time.
///
/// BUG-6: this replaces the old pattern of collecting a `Vec<AgentInternalEvent>` for the
/// *entire* run and converting+forwarding all of them in one pass after the run finishes.
/// `emit` is now called once per event, immediately as the aikit-agent run loop produces
/// it (see `run_with_config_and_gateway`), so an SSE client sees each frame as it happens
/// instead of a single end-of-run burst.
struct EmitState {
    seq: u64,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    token_usage: Option<TokenUsage>,
    quota_exceeded: Option<QuotaExceededInfo>,
    /// Set once an `AgentInternalEvent::Error` has been converted and forwarded, so the
    /// caller doesn't synthesize a second, duplicate error frame when the run
    /// subsequently returns `Err`.
    had_error_event: bool,
}

impl EmitState {
    fn new() -> Self {
        Self {
            seq: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
            token_usage: None,
            quota_exceeded: None,
            had_error_event: false,
        }
    }

    /// Convert one internal event, fold it into the accumulated result state, and
    /// forward it to `on_event` immediately.
    fn emit<F>(&mut self, event: AgentInternalEvent, on_event: &mut F) -> Result<(), RunError>
    where
        F: FnMut(AgentEvent) + Send,
    {
        if matches!(event, AgentInternalEvent::Error { .. }) {
            self.had_error_event = true;
        }

        let (stream, payload, printable) = convert_event(event);

        if let Some(text) = printable {
            match stream {
                AgentEventStream::Stdout => self.stdout.extend_from_slice(text.as_bytes()),
                AgentEventStream::Stderr => self.stderr.extend_from_slice(text.as_bytes()),
            }
        }

        if let AgentEventPayload::TokenUsageLine { usage, .. } = &payload {
            self.token_usage = Some(usage.clone());
        }

        if let AgentEventPayload::QuotaExceeded { info, .. } = &payload {
            self.quota_exceeded = Some(info.clone());
        }

        let agent_event = AgentEvent {
            agent_key: "aikit".to_string(),
            seq: self.seq,
            stream,
            payload,
        };
        self.seq += 1;

        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            on_event(agent_event);
        }))
        .map_err(RunError::CallbackPanic)
    }

    fn finish(self, success: bool) -> RunResult {
        RunResult {
            status: exit_status(if success { 0 } else { 1 }),
            stdout: self.stdout,
            stderr: self.stderr,
            token_usage: self.token_usage,
            quota_exceeded: self.quota_exceeded,
        }
    }
}

/// Convert and forward a fixed, already-known batch of events in one pass. Only used for
/// the pre-run error path ([`emit_error`]), which has a single synthetic event and no
/// underlying agent run to stream from.
fn emit_events<F>(
    events: Vec<AgentInternalEvent>,
    _options: &RunOptions,
    on_event: &mut F,
    success: bool,
) -> Result<RunResult, RunError>
where
    F: FnMut(AgentEvent) + Send,
{
    let mut state = EmitState::new();
    for event in events {
        state.emit(event, on_event)?;
    }
    Ok(state.finish(success))
}

fn convert_event(
    event: AgentInternalEvent,
) -> (AgentEventStream, AgentEventPayload, Option<String>) {
    match event {
        AgentInternalEvent::TextDelta { content, turn_id } => (
            AgentEventStream::Stdout,
            AgentEventPayload::AikitTextDelta {
                content: content.clone(),
                turn_id,
            },
            Some(content),
        ),
        AgentInternalEvent::TextFinal { content, turn_id } => (
            AgentEventStream::Stdout,
            AgentEventPayload::AikitTextFinal {
                content: content.clone(),
                turn_id,
            },
            Some(format!("{}\n", content)),
        ),
        AgentInternalEvent::ToolUse {
            tool_name,
            tool_input,
            call_id,
        } => (
            AgentEventStream::Stdout,
            AgentEventPayload::AikitToolUse {
                tool_name,
                tool_input,
                call_id,
            },
            None,
        ),
        AgentInternalEvent::ToolResult {
            call_id,
            output,
            is_error,
        } => (
            AgentEventStream::Stdout,
            AgentEventPayload::AikitToolResult {
                call_id,
                output,
                is_error,
            },
            None,
        ),
        AgentInternalEvent::SubagentSpawn {
            subagent_id,
            workdir,
        } => (
            AgentEventStream::Stdout,
            AgentEventPayload::AikitSubagentSpawn {
                subagent_id,
                workdir,
            },
            None,
        ),
        AgentInternalEvent::SubagentResult {
            subagent_id,
            status,
            changed_files,
            key_findings,
            final_message,
        } => (
            AgentEventStream::Stdout,
            AgentEventPayload::AikitSubagentResult {
                subagent_id,
                status,
                changed_files,
                key_findings,
                final_message,
            },
            None,
        ),
        AgentInternalEvent::ContextCompressed {
            original_tokens,
            compressed_tokens,
            turns_summarized,
        } => (
            AgentEventStream::Stdout,
            AgentEventPayload::AikitContextCompressed {
                original_tokens,
                compressed_tokens,
                turns_summarized,
            },
            None,
        ),
        AgentInternalEvent::StepFinish {
            iteration,
            finish_reason,
        } => (
            AgentEventStream::Stdout,
            AgentEventPayload::AikitStepFinish {
                iteration,
                finish_reason,
            },
            None,
        ),
        AgentInternalEvent::TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens,
        } => (
            AgentEventStream::Stdout,
            AgentEventPayload::TokenUsageLine {
                usage: TokenUsage {
                    input_tokens,
                    output_tokens,
                    total_tokens,
                    cache_read_tokens: None,
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                source: UsageSource::Aikit,
                raw_agent_line_seq: 0,
            },
            None,
        ),
        AgentInternalEvent::Error { code, message } => {
            let payload = if code == "E_AIKIT_LLM_ERROR_RESPONSE" && message.contains("HTTP 429") {
                AgentEventPayload::QuotaExceeded {
                    info: QuotaExceededInfo {
                        agent_key: "aikit".to_string(),
                        category: QuotaCategory::Requests,
                        raw_message: message.clone(),
                    },
                    raw_agent_line_seq: 0,
                }
            } else {
                AgentEventPayload::RawLine(format!("{}: {}", code, message))
            };
            (
                AgentEventStream::Stderr,
                payload,
                Some(format!("{}: {}\n", code, message)),
            )
        }
    }
}

fn error_code(message: &str) -> String {
    message
        .split(':')
        .next()
        .filter(|s| s.starts_with("E_AIKIT_"))
        .unwrap_or("E_AIKIT_LLM_REQUEST_FAILED")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // F3 (D2 follow-up): the previously-untested middle link of the tool-policy
    // chain. serve builds `RunOptions.session_persona` from
    // `SendMessageRequest.tools`/`disallowed_tools` (tested serve-side); this
    // must land on `AgentConfig.session_persona`, which `loop_runner::build_tools`
    // then hard-filters (tested in aikit-agent). This closes the gap between "the
    // request carries the JSON" and "the agent actually filters the tool".
    #[test]
    fn disallowed_tools_flow_from_run_options_into_agent_config_persona() {
        let tmp = tempfile::tempdir().unwrap();
        let persona = serde_json::json!({
            "name": "",
            "description": "",
            "prompt": "",
            "tools": null,
            "disallowed_tools": ["run_bash"],
        });
        let options = RunOptions::default().with_session_persona(persona);
        let mut config = config_for_injected_gateway(tmp.path().to_path_buf(), &options);

        apply_session_options(&options, &mut config);

        let sp = config
            .session_persona
            .expect("session persona should be applied onto AgentConfig");
        assert_eq!(sp.disallowed_tools, Some(vec!["run_bash".to_string()]));
        assert!(sp.tools.is_none());
    }

    #[test]
    fn no_tool_policy_leaves_persona_unset() {
        let tmp = tempfile::tempdir().unwrap();
        let options = RunOptions::default();
        let mut config = config_for_injected_gateway(tmp.path().to_path_buf(), &options);
        apply_session_options(&options, &mut config);
        assert!(
            config.session_persona.is_none(),
            "no tool policy → persona unset → full toolset (ADR 0012 default)"
        );
    }
}
