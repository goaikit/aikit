//! Cursor external hooks. Shares installation, journal and binding machinery;
//! completion continuation is deliberately not represented as a blocking Stop.
use super::hooks::{optional_string, required_string};
use super::{Decision, HookCommand, HookEvent, HookRequest, Installation, IntegrationError};
use crate::runner::{AgentEventPayload, HookAction, HookPhase};
use serde_json::{json, Value};

pub(super) fn event_name(event: HookEvent) -> Result<&'static str, IntegrationError> {
    Ok(match event {
        HookEvent::SessionStarted => "sessionStart",
        HookEvent::InputSubmitted => "beforeSubmitPrompt",
        HookEvent::BeforeTool => "preToolUse",
        HookEvent::AfterTool => "postToolUse",
        HookEvent::ToolFailed => "postToolUseFailure",
        HookEvent::SessionEnded => "sessionEnd",
        HookEvent::CompletionProposed | HookEvent::CompletionFailed => return Err(
            IntegrationError::Unsupported("Cursor completion hooks require a distinct continuation/observation contract; stop follow-ups are not an enforced completion proposal".into())
        ),
    })
}

pub(super) fn command(handler: &HookCommand) -> Result<String, IntegrationError> {
    command_for(handler, cfg!(windows))
}

fn command_for(handler: &HookCommand, windows: bool) -> Result<String, IntegrationError> {
    let executable = handler
        .executable
        .to_str()
        .ok_or_else(|| IntegrationError::Invalid("hook executable must be UTF-8".into()))?;
    if windows {
        use base64::Engine;
        // Cursor executes command strings through its selected Windows shell.
        // Keep the outer command shell-neutral. PowerShell only interprets an
        // encoded constant script; ProcessStartInfo passes native argv/stdin and
        // stdout without PowerShell's lossy native argument or pipeline conversion.
        let argv = handler
            .arguments
            .iter()
            .map(|v| windows_argument(v))
            .collect::<Vec<_>>()
            .join(" ");
        let script = format!("$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$p=[System.Diagnostics.ProcessStartInfo]::new();$p.FileName={};$p.Arguments={};$p.UseShellExecute=$false;$p.RedirectStandardInput=$true;$p.RedirectStandardOutput=$true;$p.RedirectStandardError=$true;$c=[System.Diagnostics.Process]::Start($p);$o=$c.StandardOutput.BaseStream.CopyToAsync([Console]::OpenStandardOutput());$e=$c.StandardError.BaseStream.CopyToAsync([Console]::OpenStandardError());[Console]::OpenStandardInput().CopyTo($c.StandardInput.BaseStream);$c.StandardInput.Close();$c.WaitForExit();[void]$o.GetAwaiter().GetResult();[void]$e.GetAwaiter().GetResult();exit $c.ExitCode", ps_literal(executable), ps_literal(&argv));
        let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let command =
            format!("powershell.exe -NoProfile -NonInteractive -EncodedCommand {encoded}");
        // Leave room for Cursor's surrounding stdin transport and shell command.
        if command.len() > 24000 {
            return Err(IntegrationError::Invalid(
                "Windows hook command exceeds supported length".into(),
            ));
        }
        Ok(command)
    } else {
        Ok(std::iter::once(executable)
            .chain(handler.arguments.iter().map(String::as_str))
            .map(|v| format!("'{}'", v.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join(" "))
    }
}

fn ps_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn windows_argument(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        result.extend(std::iter::repeat('\\').take(if c == '"' {
            slashes * 2 + 1
        } else {
            slashes
        }));
        result.push(c);
        slashes = 0;
    }
    result.extend(std::iter::repeat('\\').take(slashes * 2));
    result.push('"');
    result
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
        .find(|e| event_name(*e).ok() == Some(native.as_str()))
        .ok_or_else(|| {
            IntegrationError::Invalid("event is not registered by this installation".into())
        })?;
    let session_id = required_string(&value, "conversation_id", 256)?;
    if let Some(native_session) = optional_string(&value, "session_id", 256)? {
        if native_session != session_id {
            return Err(IntegrationError::Invalid(
                "Cursor session identities differ".into(),
            ));
        }
    }
    // This library binds one worktree. A multiroot conversation cannot silently
    // acquire another workspace through the same installation's identity.
    let roots = value
        .get("workspace_roots")
        .and_then(Value::as_array)
        .filter(|v| v.len() == 1)
        .ok_or_else(|| {
            IntegrationError::Unsupported(
                "Cursor integration requires exactly one workspace root".into(),
            )
        })?;
    let root = roots[0]
        .as_str()
        .filter(|s| s.len() <= 32768 && !s.contains('\0'))
        .ok_or_else(|| IntegrationError::Invalid("invalid workspace root".into()))?;
    if std::fs::canonicalize(root)? != installation.spec.workspace {
        return Err(IntegrationError::Invalid(
            "hook workspace differs from installation".into(),
        ));
    }
    let cwd = match optional_string(&value, "cwd", 32768)? {
        Some(path) => std::fs::canonicalize(path)?,
        None => installation.spec.workspace.clone(),
    };
    if !cwd.starts_with(&installation.spec.workspace) {
        return Err(IntegrationError::Invalid(
            "hook cwd is outside the installation".into(),
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
        HookEvent::AfterTool | HookEvent::ToolFailed => {
            let output = if event == HookEvent::AfterTool {
                serde_json::from_str(&required_string(&value, "tool_output", 1024 * 1024)?)?
            } else {
                Value::String(required_string(&value, "error_message", 512 * 1024)?)
            };
            let duration_ms = match value.get("duration") {
                None => None,
                Some(v) => Some(
                    v.as_u64()
                        .ok_or_else(|| IntegrationError::Invalid("invalid tool duration".into()))?,
                ),
            };
            AgentEventPayload::ToolResult {
                call_id: required_string(&value, "tool_use_id", 256)?,
                output,
                is_error: event == HookEvent::ToolFailed,
                duration_ms,
                started_at_ms: None,
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
    Ok(HookRequest {
        id: uuid::Uuid::new_v4().to_string(),
        installation_id: installation.id.clone(),
        session_id,
        prompt_id: optional_string(&value, "generation_id", 256)?,
        agent_id: optional_string(&value, "subagent_id", 256)?,
        event,
        cwd,
        payload,
        final_answer: None,
        stop_hook_active: false,
    })
}

pub(super) fn encode(
    event: HookEvent,
    decision: Option<&Decision>,
) -> Result<Value, IntegrationError> {
    match (event, decision) {
        (HookEvent::BeforeTool, Some(Decision::Allow)) => Ok(json!({"permission":"allow"})),
        (HookEvent::BeforeTool, Some(Decision::Block { reason })) => {
            Ok(json!({"permission":"deny","user_message":reason,"agent_message":reason}))
        }
        (HookEvent::InputSubmitted, Some(Decision::Allow)) => Ok(json!({"continue":true})),
        (HookEvent::InputSubmitted, Some(Decision::Block { reason })) => {
            Ok(json!({"continue":false,"user_message":reason}))
        }
        (_, None) => Ok(json!({})),
        _ => Err(IntegrationError::Unsupported(
            "Cursor observation cannot return a decision".into(),
        )),
    }
}

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod tests;
