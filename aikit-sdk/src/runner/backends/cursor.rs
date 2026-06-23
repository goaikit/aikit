//! Cursor backend: the Cursor Agent CLI over a subprocess.
//!
//! Runner key is `cursor` (ADR 0006 — was `agent`). The spawn binary remains
//! `agent` (Cursor ships its CLI as `agent`/`agent.cmd`, with the
//! `AIKIT_CURSOR_AGENT` override handled in `command_resolve`); availability
//! probing additionally tries `cursor-agent`. Usage is attributed to
//! [`UsageSource::Cursor`].
//!
//! The decoder is the former generic `event`/`message`/`result` shape — an
//! under-specified Cursor decoder that Phase B will sharpen.

use std::ffi::OsString;

use crate::runner::backends::argv_spec::{ArgvCtx, ArgvSpec, SessionMode};
use crate::runner::backends::quota_match::{match_quota, JsonPat, RawPat};
use crate::runner::capabilities::BackendCapabilities;
use crate::runner::types::{
    AgentEventPayload, AgentEventStream, MessageKind, MessagePhase, MessageRole, QuotaExceededInfo,
    StreamMessage, TokenUsage, UsageSource,
};

pub(crate) const KEY: &str = "cursor";

/// `cursor-agent` is Cursor's published binary name; `agent` is the historical
/// name (and the spawn binary `command_resolve` special-cases). Probe both.
pub(crate) const BINARY_CANDIDATES: &[&str] = &["cursor-agent", "agent"];

pub(crate) const CAPABILITIES: BackendCapabilities =
    BackendCapabilities::NONE.with_resumable_sessions();

const SPEC: ArgvSpec = ArgvSpec {
    // Spawn binary stays `agent` — see module docs / command_resolve.
    binary: "agent",
    model_flag: "--model",
    yolo_flag: Some("--force"),
    session_mode: SessionMode::Flag("--resume"),
};

pub(crate) fn decode(
    value: &serde_json::Value,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> Vec<StreamMessage> {
    let mut results = Vec::new();

    let event_key = value
        .get("event")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("type").and_then(|v| v.as_str()))
        .unwrap_or("");

    if event_key == "message" {
        if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
            results.push(StreamMessage {
                text: text.to_string(),
                phase: MessagePhase::Delta,
                role: MessageRole::Assistant,
                kind: MessageKind::Message,
                source: stream,
                raw_line_seq,
                turn_id: None,
            });
        }
    }

    if event_key == "result" {
        if let Some(result_text) = value.get("result").and_then(|v| v.as_str()) {
            results.push(StreamMessage {
                text: result_text.to_string(),
                phase: MessagePhase::Final,
                role: MessageRole::Assistant,
                kind: MessageKind::Message,
                source: stream,
                raw_line_seq,
                turn_id: None,
            });
        }
    }

    results
}

pub(crate) fn extract_usage(line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
    if line.get("type")?.as_str()? != "result" {
        return None;
    }
    let usage = line.get("usage")?;
    let input_tokens = usage.get("inputTokens")?.as_u64()?;
    let output_tokens = usage.get("outputTokens")?.as_u64()?;
    let cache_read_tokens = usage.get("cacheReadTokens").and_then(|v| v.as_u64());
    let cache_creation_tokens = usage.get("cacheWriteTokens").and_then(|v| v.as_u64());
    Some((
        TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens: None,
            cache_read_tokens,
            cache_creation_tokens,
            reasoning_tokens: None,
        },
        UsageSource::Cursor,
    ))
}

static RAW_PATS: &[RawPat] = &[
    RawPat::AgentStructuredLog,
    RawPat::Any(&["you've hit your usage limit", "usage limit for"]),
];

static JSON_PATS: &[JsonPat] = &[JsonPat::ErrorMsgAny(&[
    "rate limit",
    "quota exceeded",
    "usage limit",
])];

pub(crate) fn extract_quota(payload: &AgentEventPayload) -> Option<QuotaExceededInfo> {
    match_quota(KEY, RAW_PATS, JSON_PATS, payload)
}

pub(crate) fn argv(ctx: ArgvCtx) -> Vec<OsString> {
    let mut argv = vec![OsString::from(SPEC.binary), OsString::from("--print")];
    if ctx.events_mode {
        let fmt = if ctx.stream { "stream-json" } else { "json" };
        argv.extend_from_slice(&[OsString::from("--output-format"), OsString::from(fmt)]);
    } else if ctx.stream {
        argv.extend_from_slice(&[OsString::from("--output-format"), OsString::from("json")]);
    }
    SPEC.push_model(&mut argv, ctx.model);
    SPEC.push_yolo(&mut argv, ctx.yolo);
    SPEC.push_session_flag(&mut argv, ctx.session_id);
    argv
}
