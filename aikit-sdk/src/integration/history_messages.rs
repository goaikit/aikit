//! Read-only correlation of an existing history page with a native append intent.
use super::{IntegrationError, MessageAppendCheckpoint, MESSAGE_CONTENT_LIMIT};
use crate::history::{HistoryContent, HistoryMessage, HistorySession};
use crate::runner::{backend::Backend, types::MessageRole};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MessageHistoryMatch {
    pub page_index: usize,
    /// Exact user text and main-session lineage match the retained frame hash.
    pub framed_payload_matches: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MessageHistoryInspection {
    /// All rows with the preallocated UUID, including duplicates and rewrites.
    pub records: Vec<MessageHistoryMatch>,
}

fn invalid() -> IntegrationError {
    IntegrationError::Invalid("message history page scope/content is invalid".into())
}

fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// Correlate one page from the existing HistoryReader without sending anything
/// or changing a receipt. Obtain `checkpoint` from the original SessionBinding's
/// `message_append_checkpoint`, and `workspace` from its installation receipt.
/// Paths must already be canonical; comparison is exact and performs no I/O.
///
/// This is candidate evidence only. History canonicalization omits native origin
/// and version metadata; a matching row does not authenticate an invocation,
/// prove acknowledgement/consumption, establish complete pagination, or permit
/// retry. A missing row may be filtered, compacted, on another page, or unreadable.
/// Preserve every duplicate rather than selecting a successful-looking match.
pub fn inspect_message_history_page(
    checkpoint: &MessageAppendCheckpoint,
    workspace: &Path,
    session: &HistorySession,
    messages: &[HistoryMessage],
) -> Result<MessageHistoryInspection, IntegrationError> {
    super::message_append::validate_intent(&checkpoint.intent)?;
    let target = &checkpoint.reservation.target;
    if !matches!(checkpoint.schema_version, 1 | 2)
        || (checkpoint.schema_version == 2) != checkpoint.input.is_some()
        || session.backend != Backend::Claude
        || session.session_id != target.native_session_id
        || session.cwd.as_deref() != Some(workspace)
        || !workspace.is_absolute()
        || checkpoint.reservation.request.content.is_empty()
        || checkpoint.reservation.request.content.len() > MESSAGE_CONTENT_LIMIT
        || hash(&checkpoint.reservation.request.content) != checkpoint.reservation.payload_sha256
        || messages.len() > 1000
        || serde_json::to_vec(messages)?.len() > 4 * MESSAGE_CONTENT_LIMIT
        || messages
            .iter()
            .any(|row| row.session_id != session.session_id)
    {
        return Err(invalid());
    }
    let records = messages
        .iter()
        .enumerate()
        .filter(|(_, row)| row.uuid == checkpoint.intent.native_row_id)
        .map(|(page_index, row)| MessageHistoryMatch {
            page_index,
            framed_payload_matches: row.role == MessageRole::User
                && row.parent_tool_use_id.is_none()
                && matches!(&row.content, HistoryContent::Text(text)
                    if hash(text) == checkpoint.intent.framed_payload_sha256),
        })
        .collect();
    Ok(MessageHistoryInspection { records })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryBlock;

    fn workspace() -> std::path::PathBuf {
        std::env::temp_dir().join("aikit-history-inspection")
    }
    fn fixture() -> (MessageAppendCheckpoint, HistorySession, HistoryMessage) {
        let checkpoint = serde_json::from_value(serde_json::json!({
            "schema_version":1,"reservation":{
                "binding_id":"binding","target":{"installation_id":"install",
                "installation_revision":"rev","native_session_id":"session","start_cursor":1},
                "request":{"operation_id":"review","content":"frozen 雪"},
                "payload_sha256":hash("frozen 雪"),"reservation_id":"reservation"},
            "intent":{"native_row_id":"row","native_source":"claude/session.append",
                "framed_payload_sha256":hash("plugin frame: frozen 雪")},
            "prepared_at_unix_ms":1,"stored":null,"stored_at_unix_ms":null
        }))
        .unwrap();
        let session = serde_json::from_value(serde_json::json!({
            "backend":"claude","session_id":"session","summary":"test",
            "cwd":workspace(),"last_modified_ms":1
        }))
        .unwrap();
        let row = HistoryMessage {
            role: MessageRole::User,
            uuid: "row".into(),
            session_id: "session".into(),
            parent_tool_use_id: None,
            content: HistoryContent::Text("plugin frame: frozen 雪".into()),
        };
        (checkpoint, session, row)
    }

    #[test]
    fn matches_exact_frame_and_preserves_duplicates_and_rewrites() {
        let (checkpoint, session, row) = fixture();
        let mut changed = row.clone();
        changed.content = HistoryContent::Text("rewritten".into());
        let result = inspect_message_history_page(
            &checkpoint,
            &workspace(),
            &session,
            &[row.clone(), changed, row],
        )
        .unwrap();
        assert_eq!(
            result
                .records
                .iter()
                .map(|r| r.framed_payload_matches)
                .collect::<Vec<_>>(),
            [true, false, true]
        );
    }

    #[test]
    fn rejects_foreign_scope_and_corrupted_logical_payload() {
        let (mut checkpoint, session, row) = fixture();
        assert!(inspect_message_history_page(
            &checkpoint,
            &workspace().join("other"),
            &session,
            &[row.clone()]
        )
        .is_err());
        let mut foreign = row.clone();
        foreign.session_id = "foreign".into();
        assert!(
            inspect_message_history_page(&checkpoint, &workspace(), &session, &[foreign]).is_err()
        );
        checkpoint.reservation.request.content.push('!');
        assert!(inspect_message_history_page(&checkpoint, &workspace(), &session, &[row]).is_err());
    }

    #[test]
    fn non_plain_text_subagent_and_wrong_role_are_not_matches() {
        let (checkpoint, session, row) = fixture();
        let mut blocks = row.clone();
        blocks.content = HistoryContent::Blocks(vec![HistoryBlock::Text {
            text: "plugin frame: frozen 雪".into(),
        }]);
        let mut subagent = row.clone();
        subagent.parent_tool_use_id = Some("parent".into());
        let mut assistant = row;
        assistant.role = MessageRole::Assistant;
        let result = inspect_message_history_page(
            &checkpoint,
            &workspace(),
            &session,
            &[blocks, subagent, assistant],
        )
        .unwrap();
        assert!(result.records.iter().all(|row| !row.framed_payload_matches));
        assert!(
            inspect_message_history_page(&checkpoint, &workspace(), &session, &[])
                .unwrap()
                .records
                .is_empty()
        );
    }
}
