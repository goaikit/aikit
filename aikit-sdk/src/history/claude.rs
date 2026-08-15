//! `HistoryReader`/`HistoryMutator` for the Claude backend — spec 008 Phase 2.
//!
//! Wraps `claude-agent-sdk-rust`'s spawn-free `session::history` /
//! `session::mutations` modules. No `claude` subprocess is ever spawned:
//! every function here is a pure filesystem read/append (spec 008 §7
//! acceptance criterion 2).

use std::path::Path;

use serde_json::Value;

use crate::runner::backend::Backend;
use crate::runner::types::MessageRole;

use super::types::{
    HistoryBlock, HistoryContent, HistoryError, HistoryMessage, HistoryQuery, HistorySession,
    MessagesQuery,
};
use super::HistoryReader;

/// A bare `ClaudeHistory` reads the default `~/.claude` projects directory;
/// the underlying SDK honours `CLAUDE_CONFIG_DIR` (not `CLAUDE_HOME`) via
/// `session::paths::get_projects_dir`, which tests use to point at an
/// isolated fixture directory rather than a real `~/.claude`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ClaudeHistory;

/// Applied when `HistoryQuery::limit`/`MessagesQuery::limit` is `None` or
/// `Some(0)` (spec 008 §8 contract: "0/absent → default 100").
const DEFAULT_LIMIT: usize = 100;
/// Caps an oversized requested limit (spec 008 §8 contract: "oversized →
/// capped").
const MAX_LIMIT: usize = 1000;

fn effective_limit(limit: Option<usize>) -> usize {
    match limit {
        None | Some(0) => DEFAULT_LIMIT,
        Some(l) => l.min(MAX_LIMIT),
    }
}

/// Apply `effective_limit`/`offset` to an already-fetched, unpaged `Vec` —
/// used only on the adapter-side tag-filter path (spec 008 §7), where the
/// SDK's own paging can't be used because it runs before the tag filter.
fn page<T>(items: Vec<T>, limit: Option<usize>, offset: usize) -> Vec<T> {
    let lim = effective_limit(limit);
    items.into_iter().skip(offset).take(lim).collect()
}

fn validate_session_id(id: &str) -> Result<(), HistoryError> {
    if claude_agent_sdk::session::paths::validate_uuid(id).is_some() {
        Ok(())
    } else {
        Err(HistoryError::InvalidId {
            session_id: id.to_string(),
        })
    }
}

impl HistoryReader for ClaudeHistory {
    fn backend(&self) -> Backend {
        Backend::Claude
    }

    fn list(&self, q: &HistoryQuery) -> Result<Vec<HistorySession>, HistoryError> {
        let dir = q.cwd.as_deref();
        let infos = if let Some(tag) = q.tag.as_deref() {
            // Adapter-side tag filter (spec 008 §7): `list_sessions` has no
            // tag parameter and applies limit/offset internally, so
            // filtering after its own paging would page over the
            // *unfiltered* set and return wrong pages. Fetch unpaged
            // (limit=None, offset=0), filter, then page ourselves.
            let mut all = claude_agent_sdk::list_sessions(dir, None, 0, false);
            all.retain(|i| i.tag.as_deref() == Some(tag));
            page(all, q.limit, q.offset)
        } else {
            // No tag filter: pass limit/offset straight through to the SDK,
            // which already sorts newest-first and pages internally.
            claude_agent_sdk::list_sessions(dir, Some(effective_limit(q.limit)), q.offset, false)
        };
        Ok(infos.into_iter().map(map_session).collect())
    }

    fn info(&self, id: &str, cwd: Option<&Path>) -> Result<Option<HistorySession>, HistoryError> {
        validate_session_id(id)?;
        Ok(claude_agent_sdk::get_session_info(id, cwd).map(map_session))
    }

    fn messages(
        &self,
        id: &str,
        q: &MessagesQuery,
        cwd: Option<&Path>,
    ) -> Result<Vec<HistoryMessage>, HistoryError> {
        validate_session_id(id)?;
        // Existence pre-check (spec 008 §7): `get_session_messages` returns
        // an empty Vec for *every* failure mode — invalid id, missing file,
        // and a genuinely empty session — so it cannot distinguish "not
        // found" from "empty" on its own. `get_session_info` returns
        // `Option` and is the reliable existence signal; without this check,
        // the contract's 404 would be unreachable and `200 []` would lose
        // its "exists, zero messages" meaning.
        if claude_agent_sdk::get_session_info(id, cwd).is_none() {
            return Err(HistoryError::NotFound {
                session_id: id.to_string(),
            });
        }
        let limit = effective_limit(q.limit);
        let raw = claude_agent_sdk::get_session_messages(id, cwd, Some(limit), q.offset);
        Ok(raw.into_iter().map(to_history_message).collect())
    }
}

/// Map one `SDKSessionInfo` to the canonical `HistorySession`.
///
/// `last_modified`/`created_at` are already milliseconds since epoch in the
/// SDK (`system_time_ms`/`parse_iso_epoch_ms` internally) — copied through
/// verbatim, **no arithmetic** (spec 008 §5/§7, data-model.md).
/// `message_count` is `None`: `SDKSessionInfo` has no such field yet (SDK
/// follow-up noted in spec 008 §7, deliberately not implemented here — the
/// SDK repo is out of scope for this change).
fn map_session(info: claude_agent_sdk::SDKSessionInfo) -> HistorySession {
    HistorySession {
        backend: Backend::Claude,
        session_id: info.session_id,
        summary: info.summary,
        first_prompt: info.first_prompt,
        custom_title: info.custom_title,
        tag: info.tag,
        cwd: info.cwd.map(std::path::PathBuf::from),
        git_branch: info.git_branch,
        last_modified_ms: info.last_modified,
        created_at_ms: info.created_at,
        message_count: None,
        size_bytes: info.file_size,
    }
}

/// Derive the canonical `MessageRole` from a transcript entry's `type` and
/// (for `user` entries) whether its content carries a `tool_result` block.
/// Unrecognized types map to `System` — never dropped (data-model.md).
fn history_role(entry_type: &str, message: &Value) -> MessageRole {
    match entry_type {
        "assistant" => MessageRole::Assistant,
        "user" => {
            let is_tool_result = message
                .get("content")
                .and_then(Value::as_array)
                .map(|blocks| {
                    blocks
                        .iter()
                        .any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
                })
                .unwrap_or(false);
            if is_tool_result {
                MessageRole::Tool
            } else {
                MessageRole::User
            }
        }
        "system" => MessageRole::System,
        _ => MessageRole::System,
    }
}

/// Canonicalize one transcript entry's content.
///
/// `session::SessionMessage.message` is only the *inner* `message` value
/// (`entry.get("message")`), but `claude_agent_sdk::parse_message` expects
/// the full envelope shape (`{"type": ..., "message": {...}, ...}` — the
/// same shape a raw JSONL/CLI-output line has). This reconstructs that
/// envelope around the stored `message` value and runs it through the same
/// typed parser the live decode path uses (ADR 0018 / ADR 0011), mapping
/// each `ContentBlock` directly to a `HistoryBlock` rather than routing
/// through the flattened `Decoded` stream frames.
fn history_content(entry_type: &str, message: &Value, session_id: &str) -> HistoryContent {
    let envelope = serde_json::json!({
        "type": entry_type,
        "message": message,
        "session_id": session_id,
    });
    match claude_agent_sdk::parse_message(&envelope) {
        Ok(Some(claude_agent_sdk::Message::Assistant(a))) => {
            HistoryContent::Blocks(a.content.into_iter().map(map_block).collect())
        }
        Ok(Some(claude_agent_sdk::Message::User(u))) => match u.content {
            claude_agent_sdk::UserContent::Text(s) => HistoryContent::Text(s),
            claude_agent_sdk::UserContent::Blocks(blocks) => {
                HistoryContent::Blocks(blocks.into_iter().map(map_block).collect())
            }
        },
        // Every other typed Message variant (System, Result, ...), a parse
        // error, or an unrecognized type: pass the original value through
        // verbatim rather than drop the turn (data-model.md).
        _ => HistoryContent::Raw(message.clone()),
    }
}

/// Map one `SessionMessage` to the canonical `HistoryMessage`.
fn to_history_message(sm: claude_agent_sdk::SessionMessage) -> HistoryMessage {
    let role = history_role(&sm.r#type, &sm.message);
    let content = history_content(&sm.r#type, &sm.message, &sm.session_id);
    let parent_tool_use_id = sm.parent_tool_use_id.as_ref().and_then(|v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    });
    HistoryMessage {
        role,
        uuid: sm.uuid,
        session_id: sm.session_id,
        parent_tool_use_id,
        content,
    }
}

/// The six SDK `ContentBlock` variants map 1:1 onto `HistoryBlock` (spec 008
/// §5/§7, data-model.md); `ContentBlock` is a closed (non-`#[non_exhaustive]`)
/// enum in `claude-agent-sdk-rust`, so this match is exhaustive without a
/// wildcard arm — there is no "unrecognized `ContentBlock`" case to route to
/// `HistoryBlock::Raw` at this level (unrecognized raw block *types* are
/// already silently skipped by the SDK's own parser, forward-compatibly,
/// before a typed `ContentBlock` ever reaches this function).
fn map_block(block: claude_agent_sdk::ContentBlock) -> HistoryBlock {
    use claude_agent_sdk::ContentBlock;
    match block {
        ContentBlock::Text(t) => HistoryBlock::Text { text: t.text },
        ContentBlock::Thinking(t) => HistoryBlock::Thinking { text: t.thinking },
        ContentBlock::ToolUse(t) => HistoryBlock::ToolUse {
            id: t.id,
            name: t.name,
            input: Value::Object(t.input),
        },
        ContentBlock::ToolResult(t) => HistoryBlock::ToolResult {
            tool_use_id: t.tool_use_id,
            content: t.content.unwrap_or(Value::Null),
            is_error: t.is_error.unwrap_or(false),
        },
        ContentBlock::ServerToolUse(s) => HistoryBlock::ServerToolUse {
            id: s.id,
            name: server_tool_name(&s.name),
            input: Value::Object(s.input),
        },
        // `ServerToolResultBlock` carries no `is_error` field in the SDK;
        // `false` matches the same default the live decode path
        // (`backends/claude.rs::map_message`) uses for this variant.
        ContentBlock::ServerToolResult(s) => HistoryBlock::ServerToolResult {
            tool_use_id: s.tool_use_id,
            content: Value::Object(s.content),
            is_error: false,
        },
    }
}

fn server_tool_name(name: &claude_agent_sdk::ServerToolName) -> String {
    serde_json::to_value(name)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{name:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── effective_limit / page ─────────────────────────────────────────

    #[test]
    fn effective_limit_defaults_and_caps() {
        assert_eq!(effective_limit(None), DEFAULT_LIMIT);
        assert_eq!(effective_limit(Some(0)), DEFAULT_LIMIT);
        assert_eq!(effective_limit(Some(5)), 5);
        assert_eq!(effective_limit(Some(999_999)), MAX_LIMIT);
    }

    // ── validate_session_id ─────────────────────────────────────────────

    #[test]
    fn validate_session_id_rejects_non_uuid() {
        assert!(matches!(
            validate_session_id("not-a-uuid"),
            Err(HistoryError::InvalidId { .. })
        ));
        assert!(validate_session_id("550e8400-e29b-41d4-a716-446655440000").is_ok());
    }

    // ── history_role ──────────────────────────────────────────────────

    #[test]
    fn role_assistant() {
        assert_eq!(
            history_role("assistant", &serde_json::json!({})),
            MessageRole::Assistant
        );
    }

    #[test]
    fn role_plain_user_is_user() {
        let msg = serde_json::json!({"content": "hi"});
        assert_eq!(history_role("user", &msg), MessageRole::User);
    }

    #[test]
    fn role_tool_result_bearing_user_is_tool() {
        let msg = serde_json::json!({"content": [
            {"type": "tool_result", "tool_use_id": "tu_1", "content": "ok"}
        ]});
        assert_eq!(history_role("user", &msg), MessageRole::Tool);
    }

    #[test]
    fn role_unknown_type_is_system() {
        assert_eq!(
            history_role("progress", &serde_json::json!({})),
            MessageRole::System
        );
        assert_eq!(
            history_role("system", &serde_json::json!({})),
            MessageRole::System
        );
    }

    // ── history_content / map_block ──────────────────────────────────────

    #[test]
    fn assistant_content_maps_to_blocks() {
        let message = serde_json::json!({
            "model": "claude-x",
            "content": [
                {"type": "text", "text": "hello"},
                {"type": "thinking", "thinking": "let me think", "signature": "sig"},
                {"type": "tool_use", "id": "tu_1", "name": "Bash", "input": {"command": "ls"}},
            ]
        });
        let content = history_content("assistant", &message, "sess-1");
        match content {
            HistoryContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 3);
                assert!(matches!(&blocks[0], HistoryBlock::Text { text } if text == "hello"));
                assert!(
                    matches!(&blocks[1], HistoryBlock::Thinking { text } if text == "let me think")
                );
                assert!(matches!(&blocks[2], HistoryBlock::ToolUse { id, name, .. }
                    if id == "tu_1" && name == "Bash"));
            }
            other => panic!("expected Blocks, got {other:?}"),
        }
    }

    #[test]
    fn user_plain_string_content_maps_to_text() {
        let message = serde_json::json!({"content": "plain prompt"});
        let content = history_content("user", &message, "sess-1");
        assert_eq!(content, HistoryContent::Text("plain prompt".to_string()));
    }

    #[test]
    fn user_tool_result_content_maps_to_blocks() {
        let message = serde_json::json!({"content": [
            {"type": "tool_result", "tool_use_id": "tu_1", "content": "file.txt", "is_error": false}
        ]});
        let content = history_content("user", &message, "sess-1");
        match content {
            HistoryContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                assert!(
                    matches!(&blocks[0], HistoryBlock::ToolResult { tool_use_id, .. }
                    if tool_use_id == "tu_1")
                );
            }
            other => panic!("expected Blocks, got {other:?}"),
        }
    }

    #[test]
    fn unparseable_message_falls_back_to_raw() {
        // "assistant" type but missing required `model`/`content` fields —
        // the SDK parser errors, and the adapter must not drop the turn.
        let message = serde_json::json!({"nonsense": true});
        let content = history_content("assistant", &message, "sess-1");
        assert_eq!(content, HistoryContent::Raw(message));
    }

    #[test]
    fn unrecognized_type_falls_back_to_raw() {
        let message = serde_json::json!({"content": "x"});
        let content = history_content("progress", &message, "sess-1");
        assert_eq!(content, HistoryContent::Raw(message));
    }

    // ── live fixture test (spec 008 §7/§12, tasks.md Phase 2 Task 3) ────
    //
    // `#[ignore]`d by default: it mutates the process-global `CLAUDE_CONFIG_DIR`
    // env var, which would race other tests in the same binary if run
    // concurrently. Run explicitly and single-threaded:
    //   cargo test -p aikit-sdk --all-features -- --ignored --test-threads=1
    //
    // Points `CLAUDE_CONFIG_DIR` at an isolated temp fixture — **never** the
    // real `~/.claude` — and proves two things end-to-end through the public
    // `Backend::history_reader()` seam: (a) `list`/`info`/`messages` read a
    // hand-written fixture correctly, and (b) the whole call graph is
    // spawn-free (nothing in this module — or the SDK functions it calls —
    // touches `std::process::Command`; the on-disk-JSONL-read design makes a
    // subprocess structurally unreachable, not just empirically absent).
    #[test]
    #[ignore]
    fn live_fixture_list_info_messages_via_claude_config_dir() {
        let fixture = tempfile::tempdir().unwrap();
        let project_dir = fixture.path().join("projects").join("-tmp-fixture-proj");
        std::fs::create_dir_all(&project_dir).unwrap();

        let session_id = "550e8400-e29b-41d4-a716-446655440000";
        let jsonl_path = project_dir.join(format!("{session_id}.jsonl"));
        let lines = [
            serde_json::json!({
                "type": "user", "uuid": "u1", "sessionId": session_id,
                "message": {"content": "hello from the fixture"},
                "timestamp": "2026-01-15T10:00:00Z",
                "cwd": "/tmp/fixture-proj", "gitBranch": "main"
            }),
            serde_json::json!({
                "type": "assistant", "uuid": "a1", "parentUuid": "u1", "sessionId": session_id,
                "message": {"model": "claude-x", "content": [{"type": "text", "text": "hi there"}]},
                "timestamp": "2026-01-15T10:00:05Z"
            }),
        ];
        let body = lines
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&jsonl_path, body).unwrap();

        // SAFETY-equivalent: single-threaded (`--test-threads=1`), `#[ignore]`d
        // run; no other test observes this process's env concurrently.
        let prev = std::env::var("CLAUDE_CONFIG_DIR").ok();
        std::env::set_var("CLAUDE_CONFIG_DIR", fixture.path());

        let reader = Backend::Claude
            .history_reader()
            .expect("claude-sdk feature is enabled for this test");

        let sessions = reader
            .list(&HistoryQuery::default())
            .expect("list should succeed");
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.session_id, session_id);
        assert_eq!(session.backend, Backend::Claude);
        // 2026-range regression guard: the SDK's `parse_iso_epoch_ms` already
        // returns milliseconds; the adapter must not rescale.
        assert!(session.last_modified_ms > 1_700_000_000_000);
        assert!(session.last_modified_ms < 2_000_000_000_000);

        let info = reader
            .info(session_id, None)
            .expect("info should succeed")
            .expect("session should exist");
        assert_eq!(info.session_id, session_id);

        let messages = reader
            .messages(session_id, &MessagesQuery::default(), None)
            .expect("messages should succeed");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, MessageRole::User);
        assert_eq!(messages[1].role, MessageRole::Assistant);
        match &messages[1].content {
            HistoryContent::Blocks(blocks) => {
                assert!(matches!(&blocks[0], HistoryBlock::Text { text } if text == "hi there"));
            }
            other => panic!("expected Blocks, got {other:?}"),
        }

        // A missing (but well-formed) id is 404-shaped (`NotFound`), not an
        // empty success.
        let missing = "00000000-0000-4000-8000-000000000000";
        assert!(matches!(
            reader.info(missing, None),
            Ok(None) // info() reports absence as Ok(None), not an error
        ));
        assert!(matches!(
            reader.messages(missing, &MessagesQuery::default(), None),
            Err(HistoryError::NotFound { .. })
        ));

        match prev {
            Some(v) => std::env::set_var("CLAUDE_CONFIG_DIR", v),
            None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
        }
    }

    #[test]
    fn server_tool_blocks_map_with_is_error_false_default() {
        let message = serde_json::json!({
            "model": "claude-x",
            "content": [
                {"type": "server_tool_use", "id": "stu_1", "name": "web_search", "input": {"query": "rust"}},
                {"type": "advisor_tool_result", "tool_use_id": "stu_1", "content": {"result": "ok"}},
            ]
        });
        let content = history_content("assistant", &message, "sess-1");
        match content {
            HistoryContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 2);
                assert!(
                    matches!(&blocks[0], HistoryBlock::ServerToolUse { id, .. } if id == "stu_1")
                );
                assert!(matches!(&blocks[1],
                    HistoryBlock::ServerToolResult { tool_use_id, is_error, .. }
                    if tool_use_id == "stu_1" && !is_error));
            }
            other => panic!("expected Blocks, got {other:?}"),
        }
    }
}
