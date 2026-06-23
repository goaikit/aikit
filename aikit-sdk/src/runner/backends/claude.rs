//! Claude backend: `claude --output-format stream-json` over a subprocess.
//!
//! Phase A relocates the existing decode/usage/quota/argv logic verbatim.
//! Phase B will delegate decode to `claude-agent-sdk-rust::parse_message` and
//! light up the bidirectional control axis (see spec 006).

use std::ffi::OsString;

use crate::runner::backends::argv_spec::{ArgvCtx, ArgvSpec, SessionMode};
use crate::runner::backends::quota_match::{match_quota, JsonPat, RawPat};
use crate::runner::capabilities::BackendCapabilities;
use crate::runner::types::{
    AgentEventPayload, AgentEventStream, MessageKind, MessagePhase, MessageRole, QuotaExceededInfo,
    StreamMessage, TokenUsage, UsageSource,
};

pub(crate) const KEY: &str = "claude";

pub(crate) const BINARY_CANDIDATES: &[&str] = &["claude"];

pub(crate) const CAPABILITIES: BackendCapabilities = BackendCapabilities::NONE
    .with_structured_tools()
    .with_reasoning()
    .with_resumable_sessions()
    .with_mcp_routing()
    .with_hooks()
    .with_server_tools()
    .with_subagents();

const SPEC: ArgvSpec = ArgvSpec {
    binary: "claude",
    model_flag: "--model",
    yolo_flag: None,
    session_mode: SessionMode::Flag("--resume"),
};

pub(crate) fn decode(
    value: &serde_json::Value,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> Vec<StreamMessage> {
    let mut results = Vec::new();
    let line_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if line_type == "assistant" {
        if let Some(content) = value.get("message").and_then(|m| m.get("content")) {
            if let Some(arr) = content.as_array() {
                for item in arr {
                    if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
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
                }
            }
        }
    }

    if line_type == "result" {
        if let Some(result_text) = value.get("result").and_then(|v| v.as_str()) {
            let turn_id = value
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            results.push(StreamMessage {
                text: result_text.to_string(),
                phase: MessagePhase::Final,
                role: MessageRole::Assistant,
                kind: MessageKind::Message,
                source: stream,
                raw_line_seq,
                turn_id,
            });
        }
    }

    results
}

pub(crate) fn extract_usage(line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
    let line_type = line.get("type")?.as_str()?;

    let usage = if line_type == "result" {
        line.get("usage")?
    } else if line_type == "stream_event" {
        let event = line.get("event")?;
        let event_type = event.get("type")?.as_str()?;
        if event_type == "message_start" {
            event.get("message")?.get("usage")?
        } else if event_type == "message_delta" {
            event.get("usage")?
        } else {
            return None;
        }
    } else {
        return None;
    };

    let input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_read_tokens = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64());
    let cache_creation_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64());

    if input_tokens == 0
        && output_tokens == 0
        && cache_read_tokens.is_none()
        && cache_creation_tokens.is_none()
    {
        return None;
    }

    Some((
        TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens: None,
            cache_read_tokens,
            cache_creation_tokens,
            reasoning_tokens: None,
        },
        UsageSource::Claude,
    ))
}

static RAW_PATS: &[RawPat] = &[
    RawPat::KeywordJsonFallback("Failed to load usage data"),
    RawPat::Contains429JsonRateLimit,
    RawPat::All(&["api error:", "rate limit reached"]),
    RawPat::All(&["api error:", "rate limited"]),
    RawPat::All(&["api error:", "request rejected", "429"]),
    RawPat::Any(&["you've hit your limit", "you've hit your usage limit"]),
    RawPat::All(&["hit your limit", "reset"]),
    RawPat::Any(&["http 429"]),
    RawPat::All(&["429", "rate_limit_error"]),
    RawPat::StartsWithJsonFallback("Error: 429"),
    RawPat::Any(&["usage limit", "rate limit"]),
];

static JSON_PATS: &[JsonPat] = &[
    JsonPat::ErrorRateLimit,
    JsonPat::ResultErrorMsgAny(&["usage", "limit"]),
    JsonPat::ArrayErrorRateLimit,
];

pub(crate) fn extract_quota(payload: &AgentEventPayload) -> Option<QuotaExceededInfo> {
    match_quota(KEY, RAW_PATS, JSON_PATS, payload)
}

pub(crate) fn argv(ctx: ArgvCtx) -> Vec<OsString> {
    let mut argv = vec![
        OsString::from(SPEC.binary),
        OsString::from("-p"),
        OsString::from("-"),
        OsString::from("--dangerously-skip-permissions"),
    ];
    SPEC.push_model(&mut argv, ctx.model);
    let fmt = if ctx.events_mode {
        if ctx.stream {
            "stream-json"
        } else {
            "json"
        }
    } else if ctx.stream {
        "stream-json"
    } else {
        "text"
    };
    argv.extend_from_slice(&[OsString::from("--output-format"), OsString::from(fmt)]);
    if ctx.events_mode && ctx.stream {
        argv.push(OsString::from("--verbose"));
    }
    SPEC.push_session_flag(&mut argv, ctx.session_id);
    argv
}
