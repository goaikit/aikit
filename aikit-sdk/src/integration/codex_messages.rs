//! Factual decoding of copied native queue/read responses. No connection,
//! reservation, admission, acknowledgement or consumption proof is supplied.
use super::{IntegrationError, MESSAGE_CONTENT_LIMIT};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CodexMessageLocation {
    Queued { queue_id: String },
    Retained { turn_id: String, message_id: String },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CodexMessageMatch {
    pub location: CodexMessageLocation,
    /// Exact plain-text input only; unknown framing/attachments never match.
    pub payload_matches: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CodexMessageInspection {
    /// Every matching caller ID, including duplicates and changed payloads.
    /// Never select the first match as an authoritative receipt.
    pub records: Vec<CodexMessageMatch>,
    /// Null ends only this queue page, not an attestation of preceding pages.
    pub queue_next_cursor: Option<String>,
    pub history_has_unloaded_items: bool,
}

fn invalid() -> IntegrationError {
    IntegrationError::Invalid("invalid Codex message inspection scope or snapshot".into())
}
fn id(value: &Value) -> Result<&str, IntegrationError> {
    value
        .as_str()
        .filter(|text| !text.trim().is_empty() && text.len() <= 256 && !text.contains('\0'))
        .ok_or_else(invalid)
}
fn exact_text(value: &Value, expected: &str) -> bool {
    let Some(items) = value.as_array().filter(|items| items.len() == 1) else {
        return false;
    };
    let Some(item) = items[0].as_object() else {
        return false;
    };
    item.keys()
        .all(|key| matches!(key.as_str(), "type" | "text" | "text_elements"))
        && item.get("type").and_then(Value::as_str) == Some("text")
        && item.get("text").and_then(Value::as_str) == Some(expected)
        && item
            .get("text_elements")
            .map_or(true, |value| value.as_array().is_some_and(Vec::is_empty))
}

/// Inspect one copied `thread/queue/list` result and `thread/read` result for a
/// preallocated caller message ID and exact frozen plain text. Native fields were
/// sampled on Windows Codex 0.163.0; shape decoding is not version qualification.
///
/// This sends nothing and changes no journal. Callers must separately establish
/// the source, canonical workspace, current generation and complete pagination.
/// Missing matches, partial history and overlapping/duplicate matches are not
/// failure, successful delivery or permission to retry an uncertain native add.
/// Neither retained input nor a queued entry is actual model-consumption proof.
pub fn inspect_codex_message_snapshot(
    expected_thread_id: &str,
    client_message_id: &str,
    frozen_content: &str,
    queue_json: &[u8],
    history_json: &[u8],
) -> Result<CodexMessageInspection, IntegrationError> {
    for value in [expected_thread_id, client_message_id] {
        if value.trim().is_empty() || value.len() > 256 || value.contains('\0') {
            return Err(invalid());
        }
    }
    if frozen_content.is_empty()
        || frozen_content.len() > MESSAGE_CONTENT_LIMIT
        || queue_json.len() > 4 * MESSAGE_CONTENT_LIMIT
        || history_json.len() > 4 * MESSAGE_CONTENT_LIMIT
    {
        return Err(invalid());
    }
    let queue: Value = serde_json::from_slice(queue_json).map_err(|_| invalid())?;
    let history: Value = serde_json::from_slice(history_json).map_err(|_| invalid())?;
    let thread = history
        .get("thread")
        .and_then(Value::as_object)
        .ok_or_else(invalid)?;
    if id(thread.get("id").ok_or_else(invalid)?)? != expected_thread_id {
        return Err(invalid());
    }
    let queue_next_cursor = match queue.get("nextCursor").ok_or_else(invalid)? {
        Value::Null => None,
        value => Some(id(value)?.to_owned()),
    };
    let mut records = Vec::new();
    for item in queue
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?
    {
        if id(item.get("clientUserMessageId").ok_or_else(invalid)?)? == client_message_id {
            records.push(CodexMessageMatch {
                location: CodexMessageLocation::Queued {
                    queue_id: id(item.get("id").ok_or_else(invalid)?)?.to_owned(),
                },
                payload_matches: exact_text(item.get("input").ok_or_else(invalid)?, frozen_content),
            });
        }
    }
    let mut history_has_unloaded_items = false;
    for turn in thread
        .get("turns")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?
    {
        let turn_id = id(turn.get("id").ok_or_else(invalid)?)?;
        history_has_unloaded_items |= turn.get("itemsView").and_then(Value::as_str) != Some("full");
        for item in turn
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?
        {
            if item.get("type").and_then(Value::as_str) == Some("userMessage")
                && item.get("clientId").and_then(Value::as_str) == Some(client_message_id)
            {
                records.push(CodexMessageMatch {
                    location: CodexMessageLocation::Retained {
                        turn_id: turn_id.to_owned(),
                        message_id: id(item.get("id").ok_or_else(invalid)?)?.to_owned(),
                    },
                    payload_matches: exact_text(
                        item.get("content").ok_or_else(invalid)?,
                        frozen_content,
                    ),
                });
            }
        }
    }
    Ok(CodexMessageInspection {
        records,
        queue_next_cursor,
        history_has_unloaded_items,
    })
}

#[cfg(test)]
#[path = "codex_messages_tests.rs"]
mod tests;
