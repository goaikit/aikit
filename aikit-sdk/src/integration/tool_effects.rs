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
    /// Each replacement matches the original content, never an earlier result.
    EditTextBatch {
        path: String,
        replacements: Vec<TextReplacement>,
    },
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextReplacement {
    pub old: String,
    pub new: String,
}

impl ToolEffect {
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::ReplaceFile { path, .. }
            | Self::EditText { path, .. }
            | Self::EditTextBatch { path, .. } => Some(path),
            _ => None,
        }
    }

    /// Predict exact bytes using a caller-owned immutable baseline. This never
    /// reads files or executes tools. Native normalization/fuzzy matching is not
    /// reproduced: the caller must compare the actual result before attribution.
    pub fn expected_content(
        &self,
        before: Option<&[u8]>,
    ) -> Result<Option<Vec<u8>>, IntegrationError> {
        let invalid =
            || IntegrationError::Invalid("edit intent is not an exact unambiguous match".into());
        let text = || std::str::from_utf8(before.ok_or_else(invalid)?).map_err(|_| invalid());
        match self {
            Self::ReadOnly | Self::Unknown => Ok(None),
            Self::ReplaceFile { content, .. } => Ok(Some(content.as_bytes().to_vec())),
            Self::EditText { old, new, all, .. } => {
                let original = text()?;
                let count = original.matches(old).count();
                if old.is_empty() || count == 0 || (!all && count != 1) {
                    return Err(invalid());
                }
                let next = if *all {
                    original.replace(old, new)
                } else {
                    original.replacen(old, new, 1)
                };
                Ok(Some(next.into_bytes()))
            }
            Self::EditTextBatch { replacements, .. } => {
                let original = text()?;
                if replacements.is_empty() {
                    return Err(invalid());
                }
                let mut spans = Vec::with_capacity(replacements.len());
                for replacement in replacements {
                    if replacement.old.is_empty() {
                        return Err(invalid());
                    }
                    let mut matches = original.match_indices(&replacement.old);
                    let (start, matched) = matches.next().ok_or_else(invalid)?;
                    if matches.next().is_some() {
                        return Err(invalid());
                    }
                    spans.push((start, start + matched.len(), replacement.new.as_str()));
                }
                spans.sort_unstable_by_key(|span| span.0);
                if spans.windows(2).any(|pair| pair[0].1 > pair[1].0) {
                    return Err(invalid());
                }
                let mut next = original.to_owned();
                for (start, end, replacement) in spans.into_iter().rev() {
                    next.replace_range(start..end, replacement);
                }
                Ok(Some(next.into_bytes()))
            }
        }
    }
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
            "pi" => pi_effect(tool_name, input),
            _ => ToolEffect::Unknown,
        })
    }
}

fn pi_effect(name: &str, input: &Value) -> ToolEffect {
    // These are expected bytes, not a reimplementation of Pi's native matching
    // engine. In particular fuzzy/normalized results still require comparison.
    let Some(path) = input.get("path").and_then(Value::as_str).filter(|path| {
        !path.trim().is_empty()
            && !path.contains('\0')
            && !path.starts_with(['@', '~'])
            && !path.starts_with("file://")
            && !path.chars().any(|c| {
                matches!(
                    c,
                    '\u{00a0}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
                )
            })
            && !(cfg!(windows) && path.starts_with('/') && !path.starts_with("//"))
    }) else {
        return ToolEffect::Unknown;
    };
    match name {
        "write" => input
            .get("content")
            .and_then(Value::as_str)
            .map(|content| ToolEffect::ReplaceFile {
                path: path.into(),
                content: content.into(),
            })
            .unwrap_or(ToolEffect::Unknown),
        "edit" => {
            let parsed;
            let mut edits = match input.get("edits") {
                Some(Value::String(value)) => {
                    let Ok(value) = serde_json::from_str::<Value>(value) else {
                        return ToolEffect::Unknown;
                    };
                    parsed = value;
                    match &parsed {
                        Value::Array(edits) => edits.iter().collect::<Vec<_>>(),
                        Value::Object(_) => vec![&parsed],
                        _ => return ToolEffect::Unknown,
                    }
                }
                Some(Value::Array(edits)) => edits.iter().collect(),
                Some(value @ Value::Object(_)) => vec![value],
                None => vec![],
                _ => return ToolEffect::Unknown,
            };
            if input.get("oldText").is_some() || input.get("newText").is_some() {
                edits.push(input);
            }
            if edits.is_empty() {
                return ToolEffect::Unknown;
            }
            let replacements: Option<Vec<_>> = edits
                .into_iter()
                .map(|edit| {
                    Some(TextReplacement {
                        old: edit
                            .get("oldText")?
                            .as_str()
                            .filter(|s| !s.is_empty())?
                            .into(),
                        new: edit.get("newText")?.as_str()?.into(),
                    })
                })
                .collect();
            replacements
                .map(|replacements| ToolEffect::EditTextBatch {
                    path: path.into(),
                    replacements,
                })
                .unwrap_or(ToolEffect::Unknown)
        }
        _ => ToolEffect::Unknown,
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
    fn exact_content_prediction_preserves_single_edit_contract_and_batch_original_offsets() {
        let single = claude_effect(
            "Edit",
            &json!({"file_path":"a", "old_string":"aa", "new_string":"b", "replace_all":true}),
        );
        assert_eq!(
            single.expected_content(Some(b"aa aa")).unwrap(),
            Some(b"b b".to_vec())
        );
        let batch = pi_effect(
            "edit",
            &json!({"path":"a", "edits":[
                {"oldText":"bravo", "newText":"écho"},
                {"oldText":"alpha", "newText":"bravo"}
            ]}),
        );
        assert_eq!(batch.path(), Some("a"));
        assert_eq!(
            batch
                .expected_content(Some("alpha / bravo / Ω".as_bytes()))
                .unwrap(),
            Some("bravo / écho / Ω".as_bytes().to_vec())
        );
        for (input, baseline) in [
            (
                json!({"path":"a", "edits":[{"oldText":"alpha", "newText":"x"},{"oldText":"pha", "newText":"y"}]}),
                "alpha",
            ),
            (
                json!({"path":"a", "edits":[{"oldText":"same", "newText":"x"}]}),
                "same same",
            ),
            (
                json!({"path":"a", "edits":[{"oldText":"absent", "newText":"x"}]}),
                "present",
            ),
            (
                json!({"path":"a", "edits":[{"oldText":"alpha", "newText":"x"},{"oldText":"x", "newText":"y"}]}),
                "alpha",
            ),
        ] {
            assert!(pi_effect("edit", &input)
                .expected_content(Some(baseline.as_bytes()))
                .is_err());
        }
        assert!(batch.expected_content(None).is_err());
        assert!(batch.expected_content(Some(&[0xff])).is_err());
        assert_eq!(ToolEffect::Unknown.expected_content(None).unwrap(), None);
        assert_eq!(ToolEffect::ReadOnly.expected_content(None).unwrap(), None);
    }

    #[test]
    fn pi_input_shapes_keep_exact_bytes_and_do_not_guess_native_path_expansions() {
        let write = pi_effect(
            "write",
            &json!({"path":"dir/a file é.txt", "content":"\u{feff}raw\r\n"}),
        );
        assert_eq!(
            write.expected_content(None).unwrap(),
            Some("\u{feff}raw\r\n".as_bytes().to_vec())
        );
        for edits in [
            json!([{"oldText":"old", "newText":""}]),
            json!({"oldText":"old", "newText":""}),
            json!("[{\"oldText\":\"old\",\"newText\":\"\"}]"),
            json!("{\"oldText\":\"old\",\"newText\":\"\"}"),
        ] {
            assert_eq!(
                pi_effect("edit", &json!({"path":"a", "edits":edits}))
                    .expected_content(Some(b"old"))
                    .unwrap(),
                Some(vec![])
            );
        }
        let legacy = pi_effect(
            "edit",
            &json!({"path":"a", "oldText":"old", "newText":"new", "edits":[{"oldText":"other", "newText":"last"}]}),
        );
        assert_eq!(
            legacy.expected_content(Some(b"old other")).unwrap(),
            Some(b"new last".to_vec())
        );
        for path in ["", "\0", "@a", "~/a", "file:///a", "a\u{202f}b"] {
            assert_eq!(
                pi_effect("write", &json!({"path":path,"content":"x"})),
                ToolEffect::Unknown
            );
        }
        for (name, input) in [
            ("write", json!({"path":"a"})),
            ("write", json!({"path":"a","content":1})),
            ("edit", json!({"path":"a","edits":[]})),
            ("edit", json!({"path":"a","edits":"invalid"})),
            (
                "edit",
                json!({"path":"a","edits":[{"oldText":"","newText":"x"}]}),
            ),
            ("bash", json!({"path":"a","content":"x"})),
            ("read", json!({"path":"a"})),
        ] {
            assert_eq!(pi_effect(name, &input), ToolEffect::Unknown);
        }
    }

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
