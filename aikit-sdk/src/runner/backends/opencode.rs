//! OpenCode backend: `opencode run --format json -` over a subprocess.
//!
//! The prompt is written to stdin; `-` tells `opencode run` to read the message
//! from stdin (same pattern as `codex exec --json -- -`).

use std::ffi::OsString;

use crate::runner::backends::argv_spec::{ArgvCtx, ArgvSpec, SessionMode};
use crate::runner::backends::quota_match::{match_quota, JsonPat, RawPat};
use crate::runner::capabilities::BackendCapabilities;
use crate::runner::types::{
    AgentEventPayload, AgentEventStream, MessageKind, MessagePhase, MessageRole, QuotaExceededInfo,
    StreamMessage, TokenUsage, UsageSource,
};

pub(crate) const KEY: &str = "opencode";

pub(crate) const BINARY_CANDIDATES: &[&str] = &["opencode", "opencode-desktop"];

pub(crate) const CAPABILITIES: BackendCapabilities =
    BackendCapabilities::NONE.with_structured_tools();

const SPEC: ArgvSpec = ArgvSpec {
    binary: "opencode",
    model_flag: "-m",
    yolo_flag: Some("--dangerously-skip-permissions"),
    session_mode: SessionMode::Flag("--session"),
};

pub(crate) fn decode(
    value: &serde_json::Value,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> Vec<StreamMessage> {
    let mut results = Vec::new();
    let event_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if event_type == "text" {
        if let Some(text) = value
            .get("part")
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
        {
            results.push(StreamMessage {
                text: text.to_string(),
                phase: MessagePhase::Final,
                role: MessageRole::Assistant,
                kind: MessageKind::Message,
                source: stream,
                raw_line_seq,
                turn_id: None,
            });
        }
    }

    if event_type == "tool_use" {
        if let Some(output) = value
            .get("part")
            .and_then(|p| p.get("output"))
            .and_then(|o| o.as_str())
        {
            results.push(StreamMessage {
                text: output.to_string(),
                phase: MessagePhase::Final,
                role: MessageRole::Tool,
                kind: MessageKind::ToolOutput,
                source: stream,
                raw_line_seq,
                turn_id: None,
            });
        }
    }

    if event_type == "message" {
        let role_str = value.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(content) = value.get("content").and_then(|v| v.as_str()) {
            let role = if role_str == "assistant" {
                MessageRole::Assistant
            } else if role_str == "system" {
                MessageRole::System
            } else {
                MessageRole::Assistant
            };
            results.push(StreamMessage {
                text: content.to_string(),
                phase: MessagePhase::Final,
                role,
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
    if line.get("type")?.as_str()? != "step_finish" {
        return None;
    }
    let tokens = line.get("part")?.get("tokens")?;
    let input_tokens = tokens.get("input")?.as_u64()?;
    let output_tokens = tokens.get("output")?.as_u64()?;
    let total_tokens = tokens.get("total").and_then(|v| v.as_u64());
    let reasoning_tokens = tokens.get("reasoning").and_then(|v| v.as_u64());
    let cache_read_tokens = tokens
        .get("cache")
        .and_then(|c| c.get("read"))
        .and_then(|v| v.as_u64());
    let cache_creation_tokens = tokens
        .get("cache")
        .and_then(|c| c.get("write"))
        .and_then(|v| v.as_u64());
    Some((
        TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            reasoning_tokens,
        },
        UsageSource::OpenCode,
    ))
}

static RAW_PATS: &[RawPat] = &[
    RawPat::Any(&[
        "rate-limited",
        "daily token quota exceeded",
        "insufficient_quota",
    ]),
    RawPat::All(&["too many requests", "quota exceeded"]),
];

static JSON_PATS: &[JsonPat] = &[
    JsonPat::NestedCode {
        code: "insufficient_quota",
    },
    JsonPat::ErrorMsgAny(&["quota", "rate limit", "insufficient_quota", "429"]),
];

pub(crate) fn extract_quota(payload: &AgentEventPayload) -> Option<QuotaExceededInfo> {
    match_quota(KEY, RAW_PATS, JSON_PATS, payload)
}

pub(crate) fn argv(ctx: ArgvCtx) -> Vec<OsString> {
    let mut argv = vec![OsString::from(SPEC.binary)];
    SPEC.push_model(&mut argv, ctx.model);
    argv.push(OsString::from("run"));
    // `--dangerously-skip-permissions` (auto-approve tool use) must be passed
    // in BOTH plain and events mode. Without it opencode prompts for each
    // write/edit, so an unattended agent silently produces nothing.
    SPEC.push_yolo(&mut argv, ctx.yolo);
    if ctx.events_mode {
        argv.extend_from_slice(&[OsString::from("--format"), OsString::from("json")]);
    }
    SPEC.push_session_flag(&mut argv, ctx.session_id);
    argv.push(OsString::from("-"));
    argv
}
