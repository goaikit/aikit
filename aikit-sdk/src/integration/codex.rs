//! Codex command hooks. Native trust review remains owned by Codex/the user.
use super::hooks::{optional_string, required_string};
use super::{HookEvent, HookRequest, Installation, IntegrationError};
use crate::runner::{AgentEventPayload, HookAction, HookPhase};
use serde_json::{json, Value};

pub(super) fn event_name(event: HookEvent) -> Result<&'static str, IntegrationError> {
    match event {
        HookEvent::SessionStarted => Ok("SessionStart"),
        HookEvent::InputSubmitted => Ok("UserPromptSubmit"),
        HookEvent::BeforeTool => Ok("PreToolUse"),
        HookEvent::AfterTool => Ok("PostToolUse"),
        HookEvent::CompletionProposed => Ok("Stop"),
        HookEvent::SessionEnded => Ok("SessionEnd"),
        HookEvent::ToolFailed | HookEvent::CompletionFailed => Err(IntegrationError::Unsupported(
            "Codex does not expose the required distinct tool/turn failure hook; PostToolUse and Interrupt are not substitutes".into(),
        )),
    }
}

pub(super) fn decode(
    installation: &Installation,
    input: &[u8],
) -> Result<HookRequest, IntegrationError> {
    if input.len() > 1024 * 1024 {
        return Err(IntegrationError::Invalid("hook input exceeds 1 MiB".into()));
    }
    let value: Value = serde_json::from_slice(input)?;
    let native = required_string(&value, "hook_event_name", 128)?;
    let event = installation
        .spec
        .events
        .iter()
        .copied()
        .find(|event| event_name(*event).ok() == Some(native.as_str()))
        .ok_or_else(|| {
            IntegrationError::Invalid("event is not registered by this installation".into())
        })?;
    let session_id = required_string(&value, "session_id", 256)?;
    let cwd = std::fs::canonicalize(required_string(&value, "cwd", 32768)?)?;
    if !cwd.starts_with(&installation.spec.workspace) {
        return Err(IntegrationError::Invalid(
            "hook workspace differs from installation".into(),
        ));
    }
    let prompt_id = if matches!(event, HookEvent::SessionStarted | HookEvent::SessionEnded) {
        None
    } else {
        Some(required_string(&value, "turn_id", 256)?)
    };
    let payload = match event {
        HookEvent::SessionStarted => AgentEventPayload::SessionStarted {
            session_id: session_id.clone(),
        },
        HookEvent::BeforeTool => AgentEventPayload::ToolUse {
            call_id: required_string(&value, "tool_use_id", 256)?,
            tool_name: required_string(&value, "tool_name", 256)?,
            input: value
                .get("tool_input")
                .cloned()
                .ok_or_else(|| IntegrationError::Invalid("missing tool input".into()))?,
        },
        HookEvent::AfterTool => {
            if value.get("tool_response").is_none() {
                return Err(IntegrationError::Invalid("missing tool response".into()));
            }
            // PostToolUse also fires for failed commands. Its arbitrary JSON/text
            // output has no universal success discriminator. Retain typed hook
            // identity without fabricating ToolResult { is_error: false }.
            AgentEventPayload::Hook {
                phase: HookPhase::AfterTool,
                hook_name: "external.PostToolUse".into(),
                action: HookAction::Observed,
                payload: Some(json!({
                    "call_id": required_string(&value, "tool_use_id", 256)?,
                    "tool_name": required_string(&value, "tool_name", 256)?,
                    "outcome": "unknown"
                })),
            }
        }
        _ => AgentEventPayload::Hook {
            phase: if event == HookEvent::InputSubmitted {
                HookPhase::BeforeModel
            } else {
                HookPhase::RunEnd
            },
            hook_name: format!("external.{native}"),
            action: HookAction::Observed,
            payload: None,
        },
    };
    let final_answer = if event == HookEvent::CompletionProposed
        && value
            .get("last_assistant_message")
            .is_some_and(|value| !value.is_null())
    {
        optional_string(&value, "last_assistant_message", 512 * 1024)?
    } else {
        None
    };
    let stop_hook_active = if event == HookEvent::CompletionProposed {
        value
            .get("stop_hook_active")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                IntegrationError::Invalid("Stop requires boolean stop_hook_active".into())
            })?
    } else {
        false
    };
    Ok(HookRequest {
        id: uuid::Uuid::new_v4().to_string(),
        installation_id: installation.id.clone(),
        session_id,
        invocation_id: None,
        prompt_id,
        agent_id: optional_string(&value, "agent_id", 256)?,
        event,
        cwd,
        payload,
        final_answer,
        stop_hook_active,
    })
}

#[cfg(test)]
#[path = "codex_tests.rs"]
mod tests;
