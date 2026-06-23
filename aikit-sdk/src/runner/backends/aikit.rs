//! The built-in `aikit` backend (ADR 0009).
//!
//! aikit is the *in-process* Backend: it spawns no subprocess and has no
//! Dialect to decode — it emits canonical events directly via
//! `aikit_agent_adapter` (see the in-process Transport and the `aikit` branch of
//! `run_agent_events`). The decode/usage/quota hooks below are therefore inert
//! (identity), and it has no argv or binary candidates. It declares the richest
//! capability set.

use crate::runner::capabilities::BackendCapabilities;
use crate::runner::types::{
    AgentEventPayload, AgentEventStream, QuotaExceededInfo, StreamMessage, TokenUsage, UsageSource,
};

pub(crate) const KEY: &str = "aikit";

/// In-process — no external binary to probe.
pub(crate) const BINARY_CANDIDATES: &[&str] = &[];

pub(crate) const CAPABILITIES: BackendCapabilities = BackendCapabilities::NONE
    .with_bidirectional()
    .with_structured_tools()
    .with_file_changes()
    .with_interruptible()
    .with_resumable_sessions()
    .with_mcp_routing()
    .with_subagents()
    .with_context_compression();

/// aikit emits canonical events directly; there is no line Dialect to decode.
pub(crate) fn decode(
    _value: &serde_json::Value,
    _stream: AgentEventStream,
    _raw_line_seq: u64,
) -> Vec<StreamMessage> {
    Vec::new()
}

/// Usage is carried on the native `Aikit*` event payloads, not extracted from
/// JSON lines.
pub(crate) fn extract_usage(_line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
    None
}

/// Quota signalling for the in-process agent is surfaced by the adapter, not by
/// line scanning.
pub(crate) fn extract_quota(_payload: &AgentEventPayload) -> Option<QuotaExceededInfo> {
    None
}
