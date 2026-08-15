//! Canonical history (transcript) types — spec 008, ADR 0018.
//!
//! These are aikit's own types, not re-exports of `claude-agent-sdk`'s. The
//! seam stays Backend-agnostic and stable across SDK versions: no
//! Backend-specific type ever crosses [`HistoryReader`](super::HistoryReader).
//!
//! `HistoryMessage::role` reuses the canonical
//! [`MessageRole`](crate::runner::types::MessageRole) — the one type this
//! *transcript* vocabulary shares with the *streaming* vocabulary (ADR 0018).
//! Everything else (`HistoryContent`, `HistoryBlock`) is its own taxonomy,
//! grouped by message rather than flattened into incremental frames.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::runner::backend::Backend;
use crate::runner::types::MessageRole;

/// One discovered session, as reported by a [`HistoryReader`](super::HistoryReader).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub struct HistorySession {
    /// Which Backend owns this transcript.
    pub backend: Backend,
    /// UUID identifying the session.
    pub session_id: String,
    /// Display title: custom title → auto summary → first prompt →
    /// `session_id[..8]` (adapter-resolved).
    pub summary: String,
    /// The first meaningful user prompt, if any.
    pub first_prompt: Option<String>,
    /// A user-set custom title (via rename), if any.
    pub custom_title: Option<String>,
    /// A user-set tag, if any.
    pub tag: Option<String>,
    /// The working directory the session ran in, if known.
    pub cwd: Option<PathBuf>,
    /// The git branch active at session end, if known.
    pub git_branch: Option<String>,
    /// Last-modified time, **milliseconds since epoch**. The SDK already
    /// reports milliseconds; adapters copy this through verbatim — no
    /// arithmetic (spec 008 §5, data-model.md).
    pub last_modified_ms: i64,
    /// Session creation time, **milliseconds since epoch**, if known. Same
    /// already-ms passthrough rule as `last_modified_ms`.
    pub created_at_ms: Option<i64>,
    /// Message count, if the store can report it for free during `list`.
    /// `None` on the Claude store path today — the SDK's `SDKSessionInfo`
    /// has no such field yet (spec 008 §7 SDK follow-up).
    pub message_count: Option<u64>,
    /// Size of the underlying transcript file in bytes, if known.
    pub size_bytes: Option<u64>,
}

/// One message in a session transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub struct HistoryMessage {
    /// Canonical role — reuses [`MessageRole`], shared with the streaming
    /// vocabulary (ADR 0018). Tool-result-bearing `user` lines classify as
    /// [`MessageRole::Tool`]; unrecognized entry types map to
    /// [`MessageRole::System`] (never dropped).
    pub role: MessageRole,
    /// The transcript entry's UUID.
    pub uuid: String,
    /// The session this message belongs to.
    pub session_id: String,
    /// Subagent lineage: the tool_use id of the parent call, if this message
    /// originated inside a subagent turn.
    pub parent_tool_use_id: Option<String>,
    /// The message's canonical, renderable content.
    pub content: HistoryContent,
}

/// Canonical renderable content for one [`HistoryMessage`].
///
/// Distinct from the streaming vocabulary's flattened `Decoded` frames
/// (ADR 0018): a transcript message keeps its ordered blocks intact rather
/// than being re-flattened into one frame per block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HistoryContent {
    /// Plain string content (e.g. a user prompt with no blocks).
    Text(String),
    /// Structured assistant/user content blocks, in transcript order.
    Blocks(Vec<HistoryBlock>),
    /// The structure is unrecognized; passed through verbatim so clients can
    /// still render something rather than silently drop the turn.
    Raw(serde_json::Value),
}

/// One content block within a [`HistoryContent::Blocks`] message.
///
/// A 1:1 image of `claude_agent_sdk::ContentBlock`'s six variants, re-expressed
/// under aikit names so Claude SDK types never cross the [`HistoryReader`](super::HistoryReader)
/// trait (spec 008 §5/§7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HistoryBlock {
    /// Plain text content.
    Text { text: String },
    /// Model "thinking"/reasoning content.
    Thinking { text: String },
    /// A tool invocation.
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// The result of a tool invocation.
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
        is_error: bool,
    },
    /// A server-side / advisor tool invocation.
    ServerToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// The result of a server-side / advisor tool invocation.
    ServerToolResult {
        tool_use_id: String,
        content: serde_json::Value,
        is_error: bool,
    },
    /// Block type not yet in the taxonomy — passed through so the turn still
    /// renders when a future SDK adds a block variant this crate doesn't
    /// know about yet.
    Raw(serde_json::Value),
}

/// Query parameters for [`HistoryReader::list`](super::HistoryReader::list).
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct HistoryQuery {
    /// Restrict to sessions for this working directory. `None` = across all
    /// projects.
    pub cwd: Option<PathBuf>,
    /// Restrict to sessions carrying this tag. Filtered adapter-side, before
    /// paging (spec 008 §7 — the SDK's `list_sessions` has no tag param).
    pub tag: Option<String>,
    /// Page size. `None` = the adapter's default (100).
    pub limit: Option<usize>,
    /// Page offset.
    pub offset: usize,
    // NOTE: `include_worktrees` intentionally omitted — the SDK's
    // `list_sessions` ignores its `_include_worktrees` param today. Re-add
    // (additive, this struct is `#[non_exhaustive]`) once the SDK honours it.
}

/// Query parameters for [`HistoryReader::messages`](super::HistoryReader::messages).
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct MessagesQuery {
    /// Page size. `None` = the adapter's default (100).
    pub limit: Option<usize>,
    /// Page offset.
    pub offset: usize,
}

/// Errors from a [`HistoryReader`](super::HistoryReader) or
/// [`HistoryMutator`](super::HistoryMutator) operation.
///
/// [`HistoryError::Unsupported`] is the first-class signal distinguishing
/// "this Backend has no history store" from "the store is empty" — it must
/// never be conflated with an empty `Vec`. [`HistoryError::InvalidId`] (→
/// HTTP 400) is likewise distinct from [`HistoryError::NotFound`] (→ 404):
/// the former means the id was never a well-formed session id, the latter
/// that a well-formed id has no matching session.
#[derive(Debug)]
#[non_exhaustive]
pub enum HistoryError {
    /// This Backend has no history store — a first-class signal, distinct
    /// from an empty result.
    Unsupported { backend: Backend },
    /// A well-formed session id has no matching session.
    NotFound { session_id: String },
    /// The session id is not well-formed (e.g. not a UUID).
    InvalidId { session_id: String },
    /// An I/O failure reading the store.
    Io {
        source: std::io::Error,
        path: Option<PathBuf>,
    },
    /// The on-disk content could not be decoded into canonical types.
    Decode { message: String },
    /// An external `SessionStore` failure (future; not produced today).
    Store { message: String },
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HistoryError::Unsupported { backend } => {
                write!(f, "history is unsupported for backend '{}'", backend.key())
            }
            HistoryError::NotFound { session_id } => {
                write!(f, "session '{session_id}' not found")
            }
            HistoryError::InvalidId { session_id } => {
                write!(f, "invalid session id '{session_id}'")
            }
            HistoryError::Io { source, path } => match path {
                Some(p) => write!(f, "I/O error at {}: {source}", p.display()),
                None => write!(f, "I/O error: {source}"),
            },
            HistoryError::Decode { message } => write!(f, "decode error: {message}"),
            HistoryError::Store { message } => write!(f, "store error: {message}"),
        }
    }
}

impl std::error::Error for HistoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            HistoryError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_is_distinct_from_empty_list() {
        // The type system already enforces this (Result<Vec<_>, HistoryError>
        // vs Ok(vec![])), but pin the Display text so a future refactor can't
        // quietly collapse the two into the same message.
        let err = HistoryError::Unsupported {
            backend: Backend::Codex,
        };
        let empty: Result<Vec<HistorySession>, HistoryError> = Ok(Vec::new());
        assert!(matches!(err, HistoryError::Unsupported { .. }));
        assert!(matches!(empty, Ok(ref v) if v.is_empty()));
        assert!(err.to_string().contains("unsupported"));
    }

    #[test]
    fn invalid_id_distinct_from_not_found() {
        let invalid = HistoryError::InvalidId {
            session_id: "not-a-uuid".into(),
        };
        let not_found = HistoryError::NotFound {
            session_id: "550e8400-e29b-41d4-a716-446655440000".into(),
        };
        assert!(invalid.to_string().contains("invalid"));
        assert!(not_found.to_string().contains("not found"));
    }

    #[test]
    fn history_session_serde_round_trip() {
        let session = HistorySession {
            backend: Backend::Claude,
            session_id: "95ac29df-2f6f-47d4-a744-617632655ad1".into(),
            summary: "heromart".into(),
            first_prompt: Some("how heromart is accessible".into()),
            custom_title: Some("heromart".into()),
            tag: None,
            cwd: Some(PathBuf::from("/home/sysuser/ws001")),
            git_branch: Some("HEAD".into()),
            // 2026-range value (regression guard for the already-ms
            // passthrough — see spec 008 §5 / data-model.md).
            last_modified_ms: 1_782_398_632_826,
            created_at_ms: Some(1_781_423_077_944),
            message_count: Some(321),
            size_bytes: Some(4_630_391),
        };
        let json = serde_json::to_string(&session).unwrap();
        let back: HistorySession = serde_json::from_str(&json).unwrap();
        assert_eq!(session, back);
        // 2026-range guard: a millisecond epoch timestamp in 2026 is on the
        // order of 1.78e12; a value 1000x smaller (seconds mistakenly copied
        // as ms) or 1000x larger would fail this bound.
        assert!(back.last_modified_ms > 1_700_000_000_000);
        assert!(back.last_modified_ms < 2_000_000_000_000);
    }

    #[test]
    fn history_content_variants_serde_round_trip() {
        let text = HistoryContent::Text("hi".into());
        let blocks = HistoryContent::Blocks(vec![
            HistoryBlock::Text {
                text: "hello".into(),
            },
            HistoryBlock::ToolUse {
                id: "tu_1".into(),
                name: "Bash".into(),
                input: serde_json::json!({"command": "ls"}),
            },
        ]);
        let raw = HistoryContent::Raw(serde_json::json!({"weird": true}));

        for content in [text, blocks, raw] {
            let json = serde_json::to_string(&content).unwrap();
            let back: HistoryContent = serde_json::from_str(&json).unwrap();
            assert_eq!(content, back);
        }
    }
}
