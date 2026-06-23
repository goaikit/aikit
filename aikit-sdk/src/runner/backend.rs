//! The `Backend` enum: the closed identity of every agent aikit can run.
//!
//! Parsed once from a key string at the runner boundary, then carried as a
//! typed value. Exhaustive `match` drives every per-Backend concern — key
//! string, binary candidates, capabilities, decode, token-usage and quota
//! extraction, and argv — so adding a Backend is a compile error until all of
//! them are supplied (ADR 0008). The set is closed: there is no generic/fallback
//! Backend; an unknown key fails to parse.

use std::ffi::OsString;

use super::backends::argv_spec::ArgvCtx;
use super::backends::{aikit, claude, codex, cursor, gemini, opencode};
use super::capabilities::BackendCapabilities;
use super::types::{
    AgentEventPayload, AgentEventStream, QuotaExceededInfo, StreamMessage, TokenUsage, UsageSource,
};

/// One canonical frame produced by decoding a line of a Backend's Dialect.
///
/// Phase A backends emit only [`Decoded::Stream`]. Richer Backends (Claude via
/// `claude-agent-sdk`) also emit structured tool frames. The run loop maps each
/// variant to the corresponding [`AgentEventPayload`]; the order within the
/// returned `Vec` is the emission order.
#[derive(Debug, Clone, PartialEq)]
pub enum Decoded {
    /// Canonical text/reasoning/status message.
    Stream(StreamMessage),
    /// A structured tool call.
    ToolUse {
        call_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    /// A structured tool result, correlated to a prior `ToolUse` by `call_id`.
    ToolResult {
        call_id: String,
        output: serde_json::Value,
        is_error: bool,
    },
}

/// A runnable agent. Closed set; parse from a key with [`Backend::from_key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backend {
    Claude,
    Codex,
    Gemini,
    OpenCode,
    /// The Cursor Agent CLI (runner key `cursor`; see ADR 0006).
    Cursor,
    /// The built-in in-process agent (ADR 0009).
    Aikit,
}

/// Every Backend, in canonical order. The single source of truth for the
/// runnable-agent list.
pub const ALL: &[Backend] = &[
    Backend::Claude,
    Backend::Codex,
    Backend::Gemini,
    Backend::OpenCode,
    Backend::Cursor,
    Backend::Aikit,
];

/// Wrap a backend's `StreamMessage`s as `Decoded::Stream` frames.
fn wrap(messages: Vec<StreamMessage>) -> Vec<Decoded> {
    messages.into_iter().map(Decoded::Stream).collect()
}

impl Backend {
    /// Parse a key string into a Backend. Returns `None` for unknown keys.
    pub fn from_key(key: &str) -> Option<Backend> {
        match key {
            claude::KEY => Some(Backend::Claude),
            codex::KEY => Some(Backend::Codex),
            gemini::KEY => Some(Backend::Gemini),
            opencode::KEY => Some(Backend::OpenCode),
            cursor::KEY => Some(Backend::Cursor),
            aikit::KEY => Some(Backend::Aikit),
            _ => None,
        }
    }

    /// The canonical key string for this Backend.
    pub fn key(self) -> &'static str {
        match self {
            Backend::Claude => claude::KEY,
            Backend::Codex => codex::KEY,
            Backend::Gemini => gemini::KEY,
            Backend::OpenCode => opencode::KEY,
            Backend::Cursor => cursor::KEY,
            Backend::Aikit => aikit::KEY,
        }
    }

    /// Whether this Backend runs in-process (no subprocess / Dialect).
    pub fn is_in_process(self) -> bool {
        matches!(self, Backend::Aikit)
    }

    /// Binary candidates to probe for availability. Empty for in-process.
    pub fn binary_candidates(self) -> &'static [&'static str] {
        match self {
            Backend::Claude => claude::BINARY_CANDIDATES,
            Backend::Codex => codex::BINARY_CANDIDATES,
            Backend::Gemini => gemini::BINARY_CANDIDATES,
            Backend::OpenCode => opencode::BINARY_CANDIDATES,
            Backend::Cursor => cursor::BINARY_CANDIDATES,
            Backend::Aikit => aikit::BINARY_CANDIDATES,
        }
    }

    /// What this Backend can emit/do.
    pub fn capabilities(self) -> BackendCapabilities {
        match self {
            Backend::Claude => claude::CAPABILITIES,
            Backend::Codex => codex::CAPABILITIES,
            Backend::Gemini => gemini::CAPABILITIES,
            Backend::OpenCode => opencode::CAPABILITIES,
            Backend::Cursor => cursor::CAPABILITIES,
            Backend::Aikit => aikit::CAPABILITIES,
        }
    }

    /// Decode one inbound JSON line into canonical [`Decoded`] frames.
    ///
    /// `Decoded::Stream` frames with empty text are filtered out (an invariant
    /// no decoder may break); structured tool frames always pass through.
    pub fn decode(
        self,
        value: &serde_json::Value,
        stream: AgentEventStream,
        raw_line_seq: u64,
    ) -> Vec<Decoded> {
        // Claude (with the `claude-sdk` feature) emits rich frames directly;
        // the other backends emit StreamMessages that we wrap as Decoded::Stream.
        let decoded: Vec<Decoded> = match self {
            Backend::Claude => claude::decode(value, stream, raw_line_seq),
            Backend::Codex => wrap(codex::decode(value, stream, raw_line_seq)),
            Backend::Gemini => wrap(gemini::decode(value, stream, raw_line_seq)),
            Backend::OpenCode => wrap(opencode::decode(value, stream, raw_line_seq)),
            Backend::Cursor => wrap(cursor::decode(value, stream, raw_line_seq)),
            Backend::Aikit => wrap(aikit::decode(value, stream, raw_line_seq)),
        };
        decoded
            .into_iter()
            .filter(|d| match d {
                Decoded::Stream(m) if m.text.trim().is_empty() => {
                    tracing::debug!(
                        target: "aikit_sdk::runner::decode",
                        "E_DECODE_EMPTY_TEXT: matched rule but text is empty"
                    );
                    false
                }
                _ => true,
            })
            .collect()
    }

    /// Extract token usage from one inbound JSON line, if present.
    pub fn extract_usage(self, line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
        match self {
            Backend::Claude => claude::extract_usage(line),
            Backend::Codex => codex::extract_usage(line),
            Backend::Gemini => gemini::extract_usage(line),
            Backend::OpenCode => opencode::extract_usage(line),
            Backend::Cursor => cursor::extract_usage(line),
            Backend::Aikit => aikit::extract_usage(line),
        }
    }

    /// Detect a quota / rate-limit signal from one payload, if present.
    pub fn extract_quota(self, payload: &AgentEventPayload) -> Option<QuotaExceededInfo> {
        match self {
            Backend::Claude => claude::extract_quota(payload),
            Backend::Codex => codex::extract_quota(payload),
            Backend::Gemini => gemini::extract_quota(payload),
            Backend::OpenCode => opencode::extract_quota(payload),
            Backend::Cursor => cursor::extract_quota(payload),
            Backend::Aikit => aikit::extract_quota(payload),
        }
    }

    /// Build the subprocess argv. Panics for the in-process [`Backend::Aikit`],
    /// which is never spawned.
    pub(crate) fn build_argv(
        self,
        model: Option<&String>,
        yolo: bool,
        stream: bool,
        events_mode: bool,
        session_id: Option<&str>,
    ) -> Vec<OsString> {
        let ctx = ArgvCtx {
            model,
            yolo,
            stream,
            events_mode,
            session_id,
        };
        match self {
            Backend::Claude => claude::argv(ctx),
            Backend::Codex => codex::argv(ctx),
            Backend::Gemini => gemini::argv(ctx),
            Backend::OpenCode => opencode::argv(ctx),
            Backend::Cursor => cursor::argv(ctx),
            Backend::Aikit => unreachable!("aikit is in-process and is never spawned via argv"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_key_roundtrip_for_all() {
        for &b in ALL {
            assert_eq!(Backend::from_key(b.key()), Some(b), "roundtrip for {b:?}");
        }
    }

    #[test]
    fn from_key_unknown_is_none() {
        assert_eq!(Backend::from_key("copilot"), None);
        assert_eq!(Backend::from_key("cursor-agent"), None);
        assert_eq!(Backend::from_key("agent"), None); // renamed to "cursor" (ADR 0006)
        assert_eq!(Backend::from_key(""), None);
    }

    #[test]
    fn keys_are_unique() {
        let mut keys: Vec<&str> = ALL.iter().map(|b| b.key()).collect();
        let n = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), n, "duplicate backend key");
    }

    #[test]
    fn cursor_key_is_cursor_not_agent() {
        assert_eq!(Backend::Cursor.key(), "cursor");
        assert_eq!(Backend::from_key("cursor"), Some(Backend::Cursor));
    }

    #[test]
    fn cursor_probes_cursor_agent_then_agent() {
        assert_eq!(
            Backend::Cursor.binary_candidates(),
            &["cursor-agent", "agent"]
        );
    }

    #[test]
    fn only_aikit_is_in_process() {
        for &b in ALL {
            assert_eq!(b.is_in_process(), b == Backend::Aikit);
        }
        assert!(Backend::Aikit.binary_candidates().is_empty());
    }

    // ---- shared invariant harness over every Backend (spec 006 §6) ----

    #[test]
    fn decode_empty_object_is_empty_for_all() {
        let empty = serde_json::json!({});
        for &b in ALL {
            assert!(
                b.decode(&empty, AgentEventStream::Stdout, 0).is_empty(),
                "decode of empty object must be [] for {b:?}"
            );
        }
    }

    #[test]
    fn decode_never_yields_empty_text() {
        // A line that some decoders match but with empty text must be filtered.
        let cases = [
            serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":""}]}}),
            serde_json::json!({"type":"result","result":""}),
            serde_json::json!({"type":"message","role":"assistant","content":""}),
            serde_json::json!({"event":"message","text":""}),
        ];
        for &b in ALL {
            for c in &cases {
                for d in b.decode(c, AgentEventStream::Stdout, 0) {
                    if let Decoded::Stream(m) = d {
                        assert!(
                            !m.text.trim().is_empty(),
                            "{b:?} produced an empty-text StreamMessage"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn extract_usage_is_deterministic() {
        let line =
            serde_json::json!({"type":"result","usage":{"input_tokens":1,"output_tokens":2}});
        for &b in ALL {
            let a = b.extract_usage(&line);
            let c = b.extract_usage(&line);
            assert_eq!(a, c, "extract_usage not deterministic for {b:?}");
        }
    }
}
