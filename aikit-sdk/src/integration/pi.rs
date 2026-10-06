//! Pi's generated extension is a thin process bridge into the shared SDK.
use super::hooks::{optional_string, required_string};
use super::{Decision, HookEvent, HookRequest, Installation, IntegrationError};
use crate::runner::{AgentEventPayload, HookAction, HookPhase};
use serde_json::{json, Value};

pub(super) fn extension(installation: &Installation) -> Result<Vec<u8>, IntegrationError> {
    let spec = &installation.spec;
    let config = json!({"executable":spec.handler.executable,"arguments":spec.handler.arguments,
        "events":spec.events,"timeout_ms":u64::from(spec.timeout_seconds)*1000});
    Ok(format!(
        "// Generated and owned by AIKit. Update through plan/apply.\nconst config = {config};\n{}",
        include_str!("pi_bridge.js")
    )
    .into_bytes())
}

pub(super) fn decode(
    installation: &Installation,
    input: &[u8],
) -> Result<HookRequest, IntegrationError> {
    if input.len() > 1024 * 1024 {
        return Err(IntegrationError::Invalid("hook input exceeds 1 MiB".into()));
    }
    let value: Value = serde_json::from_slice(input)?;
    if value.get("aikit_hook_version").and_then(Value::as_u64) != Some(1) {
        return Err(IntegrationError::Unsupported(
            "Pi bridge wire version".into(),
        ));
    }
    let native = required_string(&value, "hook_event_name", 128)?;
    let event = match native.as_str() {
        "session_start" => HookEvent::SessionStarted,
        "input" => HookEvent::InputSubmitted,
        "tool_call" => HookEvent::BeforeTool,
        "tool_result" => match value.get("is_error").and_then(Value::as_bool) {
            Some(false) => HookEvent::AfterTool,
            Some(true) => HookEvent::ToolFailed,
            None => {
                return Err(IntegrationError::Invalid(
                    "tool result needs boolean is_error".into(),
                ))
            }
        },
        "agent_before_settle"
            if value.get("outcome").and_then(Value::as_str) == Some("completed") =>
        {
            HookEvent::CompletionProposed
        }
        "agent_settled"
            if matches!(
                value.get("outcome").and_then(Value::as_str),
                Some("error" | "aborted")
            ) =>
        {
            HookEvent::CompletionFailed
        }
        "session_shutdown" => HookEvent::SessionEnded,
        _ => {
            return Err(IntegrationError::Unsupported(
                "Pi event/outcome is not represented by this adapter".into(),
            ))
        }
    };
    if !installation.spec.events.contains(&event) {
        return Err(IntegrationError::Invalid(
            "event is not registered by this installation".into(),
        ));
    }
    let session_id = required_string(&value, "session_id", 256)?;
    let cwd = std::fs::canonicalize(required_string(&value, "cwd", 32768)?)?;
    if !cwd.starts_with(&installation.spec.workspace) {
        return Err(IntegrationError::Invalid(
            "hook workspace differs from installation".into(),
        ));
    }
    let payload = match event {
        HookEvent::SessionStarted => AgentEventPayload::SessionStarted {
            session_id: session_id.clone(),
        },
        HookEvent::BeforeTool => AgentEventPayload::ToolUse {
            call_id: required_string(&value, "tool_use_id", 256)?,
            tool_name: required_string(&value, "tool_name", 256)?,
            input: value
                .get("tool_input")
                .filter(|v| v.is_object())
                .cloned()
                .ok_or_else(|| IntegrationError::Invalid("missing tool input object".into()))?,
        },
        HookEvent::AfterTool | HookEvent::ToolFailed => AgentEventPayload::ToolResult {
            call_id: required_string(&value, "tool_use_id", 256)?,
            output: value
                .get("tool_response")
                .cloned()
                .ok_or_else(|| IntegrationError::Invalid("missing tool result".into()))?,
            is_error: event == HookEvent::ToolFailed,
            duration_ms: None,
            started_at_ms: None,
        },
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
    let final_answer = if event == HookEvent::CompletionProposed {
        optional_string(&value, "last_assistant_message", 512 * 1024)?
    } else {
        None
    };
    let stop_hook_active = match value.get("stop_hook_active") {
        None => false,
        Some(v) => v
            .as_bool()
            .ok_or_else(|| IntegrationError::Invalid("invalid continuation state".into()))?,
    };
    Ok(HookRequest {
        id: uuid::Uuid::new_v4().to_string(),
        installation_id: installation.id.clone(),
        session_id,
        // Pi exposes no stable turn/process identity here. Never manufacture one.
        prompt_id: None,
        agent_id: optional_string(&value, "agent_id", 256)?,
        event,
        cwd,
        payload,
        final_answer,
        stop_hook_active,
    })
}

pub(super) fn encode(decision: Option<&Decision>) -> Result<Value, IntegrationError> {
    Ok(match decision {
        Some(decision) => serde_json::to_value(decision)?,
        None => json!({}),
    })
}

#[cfg(test)]
#[path = "pi_tests.rs"]
mod tests;
