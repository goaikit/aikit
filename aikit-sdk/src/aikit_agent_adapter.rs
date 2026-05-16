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

    run_with_config_and_gateway(prompt, options, config, Box::new(gateway), &mut on_event)
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

    apply_session_options(options, &mut config);

    run_with_config_and_gateway(prompt, options, config, gateway, &mut on_event)
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
    on_event: &mut F,
) -> Result<RunResult, RunError>
where
    F: FnMut(AgentEvent) + Send,
{
    let store = SessionStore::open();
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

        let prior_turns = session_turns_to_turns(&session.turns);

        match aikit_agent::run_with_context(config, prior_turns, prompt, gateway) {
            Ok(events) => {
                let new_turns = internal_events_to_turns(prompt, &events);
                let mut updated_session = session;
                updated_session.turns.extend(new_turns);
                updated_session.updated_at = now_rfc3339();
                let _ = store.save(&updated_session);
                let _ = store.update_index(&cwd_str, &updated_session.session_id);
                emit_events(events, options, on_event, true)
            }
            Err(err) => {
                let mut result = emit_events(
                    vec![AgentInternalEvent::Error {
                        code: error_code(&err.to_string()),
                        message: err.to_string(),
                    }],
                    options,
                    on_event,
                    false,
                )?;
                result.stderr = err.to_string().into_bytes();
                Ok(result)
            }
        }
    } else {
        // New session: generate UUID, run, save.
        let session_id = uuid::Uuid::new_v4().to_string();
        let now = now_rfc3339();

        match aikit_agent::run(config, prompt, gateway) {
            Ok(events) => {
                let turns = internal_events_to_turns(prompt, &events);
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
                let mut result = emit_events(events, options, on_event, true)?;
                let session_line = format!("Session: {}\n", session_id);
                result.stderr.extend_from_slice(session_line.as_bytes());
                Ok(result)
            }
            Err(err) => {
                let mut result = emit_events(
                    vec![AgentInternalEvent::Error {
                        code: error_code(&err.to_string()),
                        message: err.to_string(),
                    }],
                    options,
                    on_event,
                    false,
                )?;
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

fn emit_events<F>(
    events: Vec<AgentInternalEvent>,
    _options: &RunOptions,
    on_event: &mut F,
    success: bool,
) -> Result<RunResult, RunError>
where
    F: FnMut(AgentEvent) + Send,
{
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut token_usage = None;
    let mut quota_exceeded = None;

    for (seq, event) in events.into_iter().enumerate() {
        let (stream, payload, printable) = convert_event(event);

        if let Some(text) = printable {
            match stream {
                AgentEventStream::Stdout => stdout.extend_from_slice(text.as_bytes()),
                AgentEventStream::Stderr => stderr.extend_from_slice(text.as_bytes()),
            }
        }

        if let AgentEventPayload::TokenUsageLine { usage, .. } = &payload {
            token_usage = Some(usage.clone());
        }

        if let AgentEventPayload::QuotaExceeded { info, .. } = &payload {
            quota_exceeded = Some(info.clone());
        }

        let agent_event = AgentEvent {
            agent_key: "aikit".to_string(),
            seq: seq as u64,
            stream,
            payload,
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            on_event(agent_event);
        }));
        if let Err(panic) = result {
            return Err(RunError::CallbackPanic(panic));
        }
    }

    Ok(RunResult {
        status: exit_status(if success { 0 } else { 1 }),
        stdout,
        stderr,
        token_usage,
        quota_exceeded,
    })
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
