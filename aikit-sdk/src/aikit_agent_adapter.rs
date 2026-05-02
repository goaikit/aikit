use std::path::PathBuf;
use std::process::ExitStatus;

use aikit_agent::llm::openai_compat::OpenAiCompatProvider;
use aikit_agent::{AgentConfig, AgentInternalEvent};

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
    mut on_event: F,
) -> Result<RunResult, RunError>
where
    F: FnMut(AgentEvent) + Send,
{
    let workdir = options
        .current_dir
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let config = AgentConfig::from_env(workdir, options.stream, options.model.clone())
        .map_err(|e| emit_error(prompt, options, &mut on_event, e.to_string()))?;

    let gateway = OpenAiCompatProvider::new(config.timeout_secs, config.connect_timeout_secs)
        .map_err(|e| emit_error(prompt, options, &mut on_event, e.to_string()))?;

    match aikit_agent::run(config, prompt, Box::new(gateway)) {
        Ok(events) => emit_events(events, options, &mut on_event, true),
        Err(err) => {
            let mut result = emit_events(
                vec![AgentInternalEvent::Error {
                    code: error_code(&err.to_string()),
                    message: err.to_string(),
                }],
                options,
                &mut on_event,
                false,
            )?;
            result.stderr = err.to_string().into_bytes();
            Ok(result)
        }
    }
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
