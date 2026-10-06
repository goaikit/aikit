//! Native tool intent translation. Intent is not evidence of a completed edit.
use super::{HookEvent, HookRequest, IntegrationError, IntegrationService};
use crate::runner::AgentEventPayload;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The application can compare an expected edit with observed workspace bytes.
/// Shells, MCP tools and unknown tools remain Unknown; matching a tool name is
/// never enough to claim that its operation succeeded or to attribute other files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolEffect {
    ReadOnly,
    ReplaceFile {
        path: String,
        content: String,
    },
    EditText {
        path: String,
        old: String,
        new: String,
        all: bool,
    },
    Unknown,
}

impl IntegrationService {
    /// Normalize the original BeforeTool callback payload. Replay omits inputs,
    /// so its effect is Unknown. Applications must persist derived evidence before
    /// returning Allow; this helper reads no workspace files and performs no edit.
    pub fn tool_effect(&self, request: &HookRequest) -> Result<ToolEffect, IntegrationError> {
        let installation = self.installed(&request.installation_id)?;
        if request.event != HookEvent::BeforeTool {
            return Ok(ToolEffect::Unknown);
        }
        let AgentEventPayload::ToolUse {
            tool_name, input, ..
        } = &request.payload
        else {
            return Ok(ToolEffect::Unknown);
        };
        Ok(match installation.spec.agent_key.as_str() {
            "claude" => claude_effect(tool_name, input),
            _ => ToolEffect::Unknown,
        })
    }
}

fn claude_effect(name: &str, input: &Value) -> ToolEffect {
    if !input.is_object() {
        return ToolEffect::Unknown;
    }
    let path = || {
        input
            .get("file_path")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty() && !s.contains('\0'))
    };
    match name {
        "Read" | "Glob" | "Grep" => ToolEffect::ReadOnly,
        "Write" => match (path(), input.get("content").and_then(Value::as_str)) {
            (Some(path), Some(content)) => ToolEffect::ReplaceFile {
                path: path.into(),
                content: content.into(),
            },
            _ => ToolEffect::Unknown,
        },
        "Edit" => {
            let old = input
                .get("old_string")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty());
            let new = input.get("new_string").and_then(Value::as_str);
            let all = match input.get("replace_all") {
                None => Some(false),
                Some(v) => v.as_bool(),
            };
            match (path(), old, new, all) {
                (Some(path), Some(old), Some(new), Some(all)) => ToolEffect::EditText {
                    path: path.into(),
                    old: old.into(),
                    new: new.into(),
                    all,
                },
                _ => ToolEffect::Unknown,
            }
        }
        _ => ToolEffect::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn preserves_exact_edit_intent_and_refuses_to_infer_shell_or_missing_payload_effects() {
        assert_eq!(
            claude_effect(
                "Write",
                &json!({"file_path":"a file.rs", "content":"raw\r\n"})
            ),
            ToolEffect::ReplaceFile {
                path: "a file.rs".into(),
                content: "raw\r\n".into()
            }
        );
        assert_eq!(
            claude_effect(
                "Edit",
                &json!({"file_path":"a.rs", "old_string":"before", "new_string":"after"})
            ),
            ToolEffect::EditText {
                path: "a.rs".into(),
                old: "before".into(),
                new: "after".into(),
                all: false
            }
        );
        assert!(matches!(
            claude_effect(
                "Edit",
                &json!({"file_path":"a.rs", "old_string":"x", "new_string":"", "replace_all":true})
            ),
            ToolEffect::EditText { all: true, .. }
        ));
        for (name, input) in [
            ("Bash", json!({"command":"cat file.txt"})),
            ("mcp__server__Read", json!({})),
            ("Read", Value::Null),
            ("Write", json!({"file_path":"a.rs"})),
            (
                "Edit",
                json!({"file_path":"a.rs", "old_string":"", "new_string":"new"}),
            ),
            (
                "Edit",
                json!({"file_path":"a.rs", "old_string":"old", "new_string":"new", "replace_all":"false"}),
            ),
        ] {
            assert_eq!(claude_effect(name, &input), ToolEffect::Unknown);
        }
    }
}
