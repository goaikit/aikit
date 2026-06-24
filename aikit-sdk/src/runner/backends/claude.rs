//! Claude backend: `claude --output-format stream-json` over a subprocess.
//!
//! Decode delegates to the typed `claude-agent-sdk` parser (`parse_message`)
//! when the `claude-sdk` feature is enabled (default), emitting structured
//! tool/thinking/server-tool frames. Bidirectional control (interrupts, hooks,
//! SDK-MCP, fork/resume) is wired via `runner/claude_session.rs` (spec 007
//! B2/B3, feature `claude-control`).

use std::ffi::OsString;

use crate::runner::backend::Decoded;
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
    .with_bidirectional()
    .with_structured_tools()
    .with_reasoning()
    .with_interruptible()
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

/// Build a canonical `StreamMessage` frame.
fn sm(
    text: String,
    phase: MessagePhase,
    kind: MessageKind,
    turn_id: Option<String>,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> StreamMessage {
    StreamMessage {
        text,
        phase,
        role: MessageRole::Assistant,
        kind,
        source: stream,
        raw_line_seq,
        turn_id,
    }
}

pub(crate) fn decode(
    value: &serde_json::Value,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> Vec<Decoded> {
    #[cfg(feature = "claude-sdk")]
    {
        sdk_decode(value, stream, raw_line_seq)
    }
    #[cfg(not(feature = "claude-sdk"))]
    {
        lenient_decode(value, stream, raw_line_seq)
    }
}

/// Typed decode via `claude-agent-sdk`. Text/result behaviour matches the
/// fallback; additionally surfaces thinking (as `Reasoning`) and structured
/// tool calls/results. Parse errors and unknown message types yield no frames
/// (same as the legacy decoder).
#[cfg(feature = "claude-sdk")]
fn sdk_decode(
    value: &serde_json::Value,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> Vec<Decoded> {
    // The SDK parser is strict (requires e.g. `model` on assistant, full
    // `result` fields). On a parse miss (older/partial CLI output, or an
    // unrecognized type) fall back to the lenient text extraction so we never
    // regress below the legacy decoder's floor.
    match claude_agent_sdk::parse_message(value) {
        Ok(Some(m)) => map_message(m, stream, raw_line_seq),
        _ => lenient_decode(value, stream, raw_line_seq),
    }
}

/// Map a typed `claude-agent-sdk` [`Message`] to canonical [`Decoded`] frames.
/// Shared by line decode ([`sdk_decode`]) and the Phase-B2 session bridge.
#[cfg(feature = "claude-sdk")]
pub(crate) fn map_message(
    msg: claude_agent_sdk::Message,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> Vec<Decoded> {
    use claude_agent_sdk::{ContentBlock, Message, UserContent};

    let mut out = Vec::new();
    match msg {
        Message::Assistant(a) => {
            for block in a.content {
                match block {
                    ContentBlock::Text(t) => out.push(Decoded::Stream(sm(
                        t.text,
                        MessagePhase::Delta,
                        MessageKind::Message,
                        None,
                        stream,
                        raw_line_seq,
                    ))),
                    ContentBlock::Thinking(t) => out.push(Decoded::Stream(sm(
                        t.thinking,
                        MessagePhase::Delta,
                        MessageKind::Reasoning,
                        None,
                        stream,
                        raw_line_seq,
                    ))),
                    ContentBlock::ToolUse(t) => out.push(Decoded::ToolUse {
                        call_id: t.id,
                        tool_name: t.name,
                        input: serde_json::Value::Object(t.input),
                    }),
                    ContentBlock::ToolResult(t) => out.push(Decoded::ToolResult {
                        call_id: t.tool_use_id,
                        output: t.content.unwrap_or(serde_json::Value::Null),
                        is_error: t.is_error.unwrap_or(false),
                    }),
                    ContentBlock::ServerToolUse(s) => out.push(Decoded::ToolUse {
                        call_id: s.id,
                        tool_name: server_tool_name(&s.name),
                        input: serde_json::Value::Object(s.input),
                    }),
                    ContentBlock::ServerToolResult(s) => out.push(Decoded::ToolResult {
                        call_id: s.tool_use_id,
                        output: serde_json::Value::Object(s.content),
                        is_error: false,
                    }),
                }
            }
        }
        // `user` lines carry tool_result blocks paired to prior tool calls.
        Message::User(u) => {
            if let UserContent::Blocks(blocks) = u.content {
                for block in blocks {
                    if let ContentBlock::ToolResult(t) = block {
                        out.push(Decoded::ToolResult {
                            call_id: t.tool_use_id,
                            output: t.content.unwrap_or(serde_json::Value::Null),
                            is_error: t.is_error.unwrap_or(false),
                        });
                    }
                }
            }
        }
        Message::Result(r) => {
            if let Some(text) = r.result {
                out.push(Decoded::Stream(sm(
                    text,
                    MessagePhase::Final,
                    MessageKind::Message,
                    Some(r.session_id),
                    stream,
                    raw_line_seq,
                )));
            }
        }
        // System / StreamEvent / RateLimit / Task* / Hook / Mirror carry no
        // message text in the legacy contract: rate-limit is surfaced by
        // `extract_quota` and usage by `extract_usage` (both unchanged).
        _ => {}
    }
    out
}

#[cfg(feature = "claude-sdk")]
fn server_tool_name(name: &claude_agent_sdk::ServerToolName) -> String {
    serde_json::to_value(name)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{name:?}"))
}

/// Hand-rolled text-only decode. The sole decoder when the `claude-sdk` feature
/// is off, and the lenient fallback for lines the SDK parser rejects when it is
/// on. Handles only `assistant` text and `result` — the legacy floor.
fn lenient_decode(
    value: &serde_json::Value,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> Vec<Decoded> {
    let mut results = Vec::new();
    let line_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if line_type == "assistant" {
        if let Some(content) = value.get("message").and_then(|m| m.get("content")) {
            if let Some(arr) = content.as_array() {
                for item in arr {
                    if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                            results.push(Decoded::Stream(sm(
                                text.to_string(),
                                MessagePhase::Delta,
                                MessageKind::Message,
                                None,
                                stream,
                                raw_line_seq,
                            )));
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
            results.push(Decoded::Stream(sm(
                result_text.to_string(),
                MessagePhase::Final,
                MessageKind::Message,
                turn_id,
                stream,
                raw_line_seq,
            )));
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

#[cfg(all(test, feature = "claude-sdk"))]
mod sdk_tests {
    use super::*;
    use crate::runner::backend::Decoded;

    fn dec(line: serde_json::Value) -> Vec<Decoded> {
        decode(&line, AgentEventStream::Stdout, 7)
    }

    #[test]
    fn assistant_text_preserved_as_stream_delta() {
        let out = dec(serde_json::json!({
            "type": "assistant",
            "message": {"model": "claude-x", "content": [{"type": "text", "text": "hi"}]}
        }));
        assert_eq!(out.len(), 1);
        match &out[0] {
            Decoded::Stream(m) => {
                assert_eq!(m.text, "hi");
                assert_eq!(m.phase, MessagePhase::Delta);
                assert_eq!(m.kind, MessageKind::Message);
                assert_eq!(m.raw_line_seq, 7);
            }
            other => panic!("expected Stream, got {other:?}"),
        }
    }

    #[test]
    fn assistant_thinking_becomes_reasoning() {
        let out = dec(serde_json::json!({
            "type": "assistant",
            "message": {"model": "m", "content": [
                {"type": "thinking", "thinking": "let me think", "signature": "sig"}
            ]}
        }));
        assert_eq!(out.len(), 1);
        match &out[0] {
            Decoded::Stream(m) => {
                assert_eq!(m.text, "let me think");
                assert_eq!(m.kind, MessageKind::Reasoning);
            }
            other => panic!("expected Reasoning Stream, got {other:?}"),
        }
    }

    #[test]
    fn assistant_tool_use_is_structured() {
        let out = dec(serde_json::json!({
            "type": "assistant",
            "message": {"model": "m", "content": [
                {"type": "tool_use", "id": "tu_1", "name": "Bash", "input": {"command": "ls"}}
            ]}
        }));
        assert_eq!(out.len(), 1);
        match &out[0] {
            Decoded::ToolUse {
                call_id,
                tool_name,
                input,
            } => {
                assert_eq!(call_id, "tu_1");
                assert_eq!(tool_name, "Bash");
                assert_eq!(input.get("command").unwrap(), "ls");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn user_tool_result_is_structured() {
        let out = dec(serde_json::json!({
            "type": "user",
            "message": {"content": [
                {"type": "tool_result", "tool_use_id": "tu_1", "content": "file.txt", "is_error": false}
            ]}
        }));
        assert_eq!(out.len(), 1);
        match &out[0] {
            Decoded::ToolResult {
                call_id,
                output,
                is_error,
            } => {
                assert_eq!(call_id, "tu_1");
                assert_eq!(output, "file.txt");
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn result_text_is_final_with_turn_id() {
        let out = dec(serde_json::json!({
            "type": "result", "subtype": "success", "duration_ms": 1, "duration_api_ms": 1,
            "is_error": false, "num_turns": 1, "session_id": "sess-9", "result": "done"
        }));
        assert_eq!(out.len(), 1);
        match &out[0] {
            Decoded::Stream(m) => {
                assert_eq!(m.text, "done");
                assert_eq!(m.phase, MessagePhase::Final);
                assert_eq!(m.turn_id.as_deref(), Some("sess-9"));
            }
            other => panic!("expected Final Stream, got {other:?}"),
        }
    }

    #[test]
    fn unknown_and_malformed_yield_nothing() {
        assert!(
            dec(serde_json::json!({"type": "stream_event", "event": {"type": "ping"}})).is_empty()
        );
        assert!(dec(serde_json::json!({"type": "system", "subtype": "init"})).is_empty());
        assert!(dec(serde_json::json!({})).is_empty());
    }
}
