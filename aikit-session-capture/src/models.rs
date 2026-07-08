//! Normalized event types emitted by every adapter.
//!
//! See spec 010 `data-model.md` for the full field tables and invariants.
//! All enums/structs are `#[non_exhaustive]` and
//! `#[serde(rename_all = "snake_case")]`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Stable identifier for one AI coding tool's session format. Stored in the
/// `tool` column of the host's event store. Mirrors
/// `superbased-observer/internal/models.Tool*`.
// This enum intentionally reserves arms for adapters not implemented in this
// spec (Cursor, Gemini, Cline, …) so adding a new adapter is a non-breaking
// change for downstream consumers.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    ClaudeCode,
    Codex,
    OpenCode,
    /// Reserved for a future Cursor hook-driven adapter (spec 011).
    Cursor,
    /// Reserved for a future Gemini adapter.
    Gemini,
}

impl ToolKind {
    /// Stable string identifier (matches the serde representation).
    pub fn as_str(self) -> &'static str {
        match self {
            ToolKind::ClaudeCode => "claude_code",
            ToolKind::Codex => "codex",
            ToolKind::OpenCode => "open_code",
            ToolKind::Cursor => "cursor",
            ToolKind::Gemini => "gemini",
        }
    }
}

/// Normalized action category. Mirrors observer's 28-category taxonomy,
/// trimmed to the kinds adapters in this spec emit.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Read,
    Write,
    Edit,
    Delete,
    Bash,
    Glob,
    Grep,
    Search,
    WebFetch,
    WebSearch,
    Mcp,
    Think,
    Plan,
    Subagent,
    Other,
}

impl ActionKind {
    /// Stable string tag; used as a sort key where `Discriminant` is not `Ord`.
    pub fn as_str(self) -> &'static str {
        match self {
            ActionKind::Read => "read",
            ActionKind::Write => "write",
            ActionKind::Edit => "edit",
            ActionKind::Delete => "delete",
            ActionKind::Bash => "bash",
            ActionKind::Glob => "glob",
            ActionKind::Grep => "grep",
            ActionKind::Search => "search",
            ActionKind::WebFetch => "web_fetch",
            ActionKind::WebSearch => "web_search",
            ActionKind::Mcp => "mcp",
            ActionKind::Think => "think",
            ActionKind::Plan => "plan",
            ActionKind::Subagent => "subagent",
            ActionKind::Other => "other",
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Success,
    Failure,
    Cancelled,
    Unknown,
}

/// Where a [`TokenEvent`] was captured. Distinguishes exact upstream
/// numbers (proxy) from approximations derived from on-disk transcripts.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSource {
    /// Sourced from the on-disk JSONL/SQLite transcript. Approximate.
    Transcript,
    /// Sourced from the API reverse proxy (future spec). Exact.
    Proxy,
}

/// One normalized tool call. The unit the host stores, queries, and surfaces
/// over MCP.
///
/// **Contract**: `input` and `output` MUST be scrubbed before this struct
/// leaves the adapter. The injected [`SecretScrubber`][crate::SecretScrubber]
/// is the single chokepoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEvent {
    pub source_event_id: String,
    pub source_file: PathBuf,
    pub session_id: String,
    pub tool: ToolKind,
    pub kind: ActionKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    pub status: ActionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,
}

/// Usage envelope for one assistant turn. All token fields are `Option`:
/// absence means "not reported", never "zero". See spec 010 data-model.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenEvent {
    pub source_event_id: String,
    pub session_id: String,
    pub tool: ToolKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
    /// Anthropic 1h ephemeral tier; absent on non-Anthropic traffic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_1h_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    pub captured_at_ms: i64,
    pub captured_via: CaptureSource,
}

/// Tier-2 (transcript-derived) prompt-cache view emitted by adapters that
/// can see content blocks + usage envelopes. Additive — adapters that don't
/// populate it leave it empty.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheObservation {
    pub source_event_id: String,
    pub session_id: String,
    pub tool: ToolKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_1h_input_tokens: Option<u64>,
    /// Content-hash of assistant-side blocks; the host uses this for
    /// prefix-cache attribution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant_blocks_hash: Option<String>,
    /// Tool names that differ from the prior turn (a cache invalidation cause).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tools_changed: Vec<String>,
    pub observed_at_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolkind_serde_roundtrip() {
        for k in [
            ToolKind::ClaudeCode,
            ToolKind::Codex,
            ToolKind::OpenCode,
            ToolKind::Cursor,
            ToolKind::Gemini,
        ] {
            let s = serde_json::to_string(&k).unwrap();
            let back: ToolKind = serde_json::from_str(&s).unwrap();
            assert_eq!(k, back, "serde roundtrip failed for {:?}", k);
            assert_eq!(k.as_str(), s.trim_matches('"'));
        }
    }

    #[test]
    fn token_event_absence_is_not_zero() {
        // Absent fields deserialize to None, not Some(0). This is the
        // invariant from spec 010 data-model.md and spec 009 §5.
        let ev: TokenEvent = serde_json::from_str(
            r#"{"source_event_id":"x","session_id":"s","tool":"codex",
                "captured_at_ms":1,"captured_via":"transcript"}"#,
        )
        .unwrap();
        assert_eq!(ev.input_tokens, None);
        assert_eq!(ev.cache_read_tokens, None);
    }
}
