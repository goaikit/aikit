//! Pi backend: the `pi` coding agent driven over its RPC mode
//! (`pi --mode rpc`) as a bidirectional session (ADR 0017).
//!
//! Unlike the subprocess-stdout-lines Backends, Pi's prompt is a framed stdin
//! JSON-RPC command (no ARG_MAX ceiling), and a run drains until the session
//! settles (`agent_settled`) rather than merely until the process exits. The
//! decode / usage / quota helpers below are pure and shared by both the live
//! session path and the recorded-fixture test path, so there is exactly one
//! implementation (spec 014).

use std::ffi::OsString;

use crate::runner::backend::Decoded;
use crate::runner::backends::quota_match::{infer_quota_category, truncate_message};
use crate::runner::capabilities::BackendCapabilities;
use crate::runner::types::{
    AgentEventPayload, AgentEventStream, MessageKind, MessagePhase, MessageRole, QuotaExceededInfo,
    StreamMessage, TokenUsage, UsageSource,
};

pub(crate) const KEY: &str = "pi";

pub(crate) const BINARY_CANDIDATES: &[&str] = &["pi"];

// Capabilities are honest — each `true` is backed by an RPC event the decoder
// actually emits, or by a transport property the run path exercises:
//   - bidirectional / interruptible: the RPC transport + the run cancel handle
//     (abort = process-group kill, ADR 0014);
//   - structured_tools: `tool_execution_start`/`tool_execution_end` decode to
//     `ToolUse`/`ToolResult`;
//   - reasoning: `thinking_delta` decodes to a `Reasoning` frame;
//   - resumable_sessions: the `--session` spawn flag resumes a prior session.
// `file_changes`/`context_compression` are intentionally `false` for v1
// (edits fold into `ToolResult`; compaction surfaces as a status message).
// `false -> true` is a non-breaking change later.
pub(crate) const CAPABILITIES: BackendCapabilities = BackendCapabilities::NONE
    .with_bidirectional()
    .with_structured_tools()
    .with_reasoning()
    .with_interruptible()
    .with_resumable_sessions();

/// Keywords that signal a quota / rate-limit condition, matched
/// case-insensitively against a line's text. No Pi-specific error object is
/// known upstream, so the generic OpenAI/Anthropic vocabulary is reused.
const QUOTA_KEYWORDS: &[&str] = &[
    "rate_limit_error",
    "rate limit",
    "429",
    "overloaded",
    "insufficient_quota",
    "quota exceeded",
];

/// Build a canonical `StreamMessage` frame.
fn sm(
    text: String,
    phase: MessagePhase,
    role: MessageRole,
    kind: MessageKind,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> StreamMessage {
    StreamMessage {
        text,
        phase,
        role,
        kind,
        source: stream,
        raw_line_seq,
        turn_id: None,
    }
}

fn str_field(value: &serde_json::Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Concatenate the `text` content blocks of an assistant message
/// (`message.content[type=text].text`), with a bare-string fallback.
fn concat_text_content(message: &serde_json::Value) -> String {
    if let Some(content) = message.get("content") {
        if let Some(arr) = content.as_array() {
            let mut buf = String::new();
            for block in arr {
                if block.get("type").and_then(|v| v.as_str()) == Some("text") {
                    if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                        buf.push_str(t);
                    }
                }
            }
            return buf;
        }
        if let Some(s) = content.as_str() {
            return s.to_string();
        }
    }
    String::new()
}

/// Decode one inbound RPC event into canonical [`Decoded`] frames.
///
/// Field-level mapping (spec 014):
/// - `message_update` `text_delta`     → streamed assistant text (Delta)
/// - `message_update` `thinking_delta` → streamed reasoning (Delta)
/// - `turn_end.message`                → final assistant text (Final)
/// - `tool_execution_start`            → `ToolUse`
/// - `tool_execution_end`              → `ToolResult`
/// - `compaction_end` / `auto_retry_*` / `extension_error` → status (Final)
///
/// Final assistant text is emitted from `turn_end.message` (one per turn);
/// `message_end` is used only to read usage (see [`extract_usage`]), never to
/// emit a second Final for the same content. `tool_execution_update` (streaming
/// partial output) is not required for v1 and is dropped.
pub(crate) fn decode(
    value: &serde_json::Value,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> Vec<Decoded> {
    let mut out = Vec::new();
    let ty = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match ty {
        "message_update" => {
            if let Some(ev) = value.get("assistantMessageEvent") {
                let sub = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let delta = ev.get("delta").and_then(|v| v.as_str()).unwrap_or("");
                match sub {
                    "text_delta" => out.push(Decoded::Stream(sm(
                        delta.to_string(),
                        MessagePhase::Delta,
                        MessageRole::Assistant,
                        MessageKind::Message,
                        stream,
                        raw_line_seq,
                    ))),
                    "thinking_delta" => out.push(Decoded::Stream(sm(
                        delta.to_string(),
                        MessagePhase::Delta,
                        MessageRole::Assistant,
                        MessageKind::Reasoning,
                        stream,
                        raw_line_seq,
                    ))),
                    _ => {}
                }
            }
        }
        "turn_end" => {
            if let Some(message) = value.get("message") {
                let text = concat_text_content(message);
                if !text.is_empty() {
                    out.push(Decoded::Stream(sm(
                        text,
                        MessagePhase::Final,
                        MessageRole::Assistant,
                        MessageKind::Message,
                        stream,
                        raw_line_seq,
                    )));
                }
            }
        }
        "tool_execution_start" => {
            if let (Some(call_id), Some(tool_name)) =
                (str_field(value, "toolCallId"), str_field(value, "toolName"))
            {
                let input = value
                    .get("args")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                out.push(Decoded::ToolUse {
                    call_id,
                    tool_name,
                    input,
                });
            }
        }
        "tool_execution_end" => {
            if let Some(call_id) = str_field(value, "toolCallId") {
                let output = value
                    .get("result")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let is_error = value
                    .get("isError")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                out.push(Decoded::ToolResult {
                    call_id,
                    output,
                    is_error,
                });
            }
        }
        "compaction_end" => {
            let mut msg = String::from("context compacted");
            if let Some(result) = value.get("result") {
                let before = result.get("tokensBefore").and_then(|v| v.as_u64());
                let after = result.get("estimatedTokensAfter").and_then(|v| v.as_u64());
                if before.is_some() || after.is_some() {
                    msg = format!("context compacted ({} -> {:?})", before.unwrap_or(0), after);
                }
            }
            out.push(Decoded::Stream(sm(
                msg,
                MessagePhase::Final,
                MessageRole::System,
                MessageKind::Status,
                stream,
                raw_line_seq,
            )));
        }
        "auto_retry_start" | "auto_retry_end" | "extension_error" => {
            let msg = str_field(value, "errorMessage")
                .or_else(|| str_field(value, "error"))
                .unwrap_or_else(|| format!("pi {ty}"));
            out.push(Decoded::Stream(sm(
                msg,
                MessagePhase::Final,
                MessageRole::System,
                MessageKind::Status,
                stream,
                raw_line_seq,
            )));
        }
        _ => {}
    }
    out
}

/// Extract per-message token usage from an RPC event carrying an
/// `AssistantMessage.usage`. Usage is read from `message_end` and
/// `turn_end` (the events that carry a complete assistant message).
pub(crate) fn extract_usage(line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
    let ty = line.get("type").and_then(|v| v.as_str())?;
    if !matches!(ty, "message_end" | "turn_end") {
        return None;
    }
    let usage = line.get("message").and_then(|m| m.get("usage"))?;
    let input_tokens = usage.get("input").and_then(|v| v.as_u64())?;
    let output_tokens = usage.get("output").and_then(|v| v.as_u64())?;
    let cache_read_tokens = usage.get("cacheRead").and_then(|v| v.as_u64());
    let cache_creation_tokens = usage.get("cacheWrite").and_then(|v| v.as_u64());
    Some((
        TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens: None,
            cache_read_tokens,
            cache_creation_tokens,
            reasoning_tokens: None,
        },
        UsageSource::Pi,
    ))
}

/// True when an event signals the run is fully settled — automatic retries,
/// compaction, and queued follow-ups have completed (spec 014). The live run
/// drains until this fires, then tears the session down.
pub(crate) fn is_settled_event(value: &serde_json::Value) -> bool {
    matches!(
        value.get("type").and_then(|v| v.as_str()),
        Some("agent_settled") | Some("agent_end")
    )
}

fn is_quota_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    QUOTA_KEYWORDS.iter().any(|k| lower.contains(k))
}

fn info_from_message(message: &str) -> QuotaExceededInfo {
    QuotaExceededInfo {
        agent_key: KEY.to_string(),
        category: infer_quota_category(message),
        raw_message: truncate_message(message, 500),
    }
}

/// Pull a human-readable error message out of a JSON error/response payload,
/// covering both `{"type":"error",...}` and Pi's
/// `{"type":"response","success":false,"error":{...}}` shapes.
fn quota_message_from_json(value: &serde_json::Value) -> Option<String> {
    let ty = value.get("type").and_then(|v| v.as_str())?;
    let error = match ty {
        "error" => value.get("error")?,
        "response" => {
            // Only a failed response carries an error worth inspecting.
            if value.get("success").and_then(|v| v.as_bool()) == Some(true) {
                return None;
            }
            value.get("error")?
        }
        _ => return None,
    };
    if let Some(s) = error.get("message").and_then(|v| v.as_str()) {
        return Some(s.to_string());
    }
    if let Some(s) = error.as_str() {
        return Some(s.to_string());
    }
    Some(error.to_string())
}

/// Detect a quota / rate-limit signal from one payload, if present.
pub(crate) fn extract_quota(payload: &AgentEventPayload) -> Option<QuotaExceededInfo> {
    match payload {
        AgentEventPayload::JsonLine(value) => {
            let msg = quota_message_from_json(value)?;
            is_quota_text(&msg).then(|| info_from_message(&msg))
        }
        AgentEventPayload::RawLine(text) => is_quota_text(text).then(|| info_from_message(text)),
        _ => None,
    }
}

/// The bytes to write to `pi`'s stdin to issue a one-shot prompt command,
/// newline-terminated (JSONL framing). Kept pure so it is shared by the live
/// path and the test path.
pub(crate) fn prompt_command(prompt: &str) -> String {
    let cmd = serde_json::json!({ "type": "prompt", "prompt": prompt });
    format!("{cmd}\n")
}

/// The spawn argv for `pi --mode rpc`. `model` and `session_id` are passed
/// through verbatim as spawn flags (spec 014); `yolo`/`stream`/`events_mode`
/// are unused — RPC mode always emits structured events.
pub(crate) fn argv(ctx: crate::runner::backends::argv_spec::ArgvCtx) -> Vec<OsString> {
    let mut argv = vec![
        OsString::from(KEY),
        OsString::from("--mode"),
        OsString::from("rpc"),
    ];
    if let Some(model) = ctx.model {
        if !model.trim().is_empty() {
            argv.push(OsString::from("--model"));
            argv.push(OsString::from(model));
        }
    }
    if let Some(id) = ctx.session_id {
        argv.push(OsString::from("--session"));
        argv.push(OsString::from(id));
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::backends::argv_spec::ArgvCtx;
    use crate::runner::types::AgentEventStream;

    const STDOUT: AgentEventStream = AgentEventStream::Stdout;

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    // ---- decode -----------------------------------------------------------

    #[test]
    fn decode_text_delta_is_streamed_assistant_message() {
        let v = json(
            r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"Hel"}}"#,
        );
        let out = decode(&v, STDOUT, 7);
        assert_eq!(out.len(), 1);
        match &out[0] {
            Decoded::Stream(m) => {
                assert_eq!(m.text, "Hel");
                assert_eq!(m.phase, MessagePhase::Delta);
                assert_eq!(m.role, MessageRole::Assistant);
                assert_eq!(m.kind, MessageKind::Message);
                assert_eq!(m.raw_line_seq, 7);
            }
            other => panic!("expected Stream, got {other:?}"),
        }
    }

    #[test]
    fn decode_thinking_delta_is_streamed_reasoning() {
        let v = json(
            r#"{"type":"message_update","assistantMessageEvent":{"type":"thinking_delta","delta":"hmm"}}"#,
        );
        let out = decode(&v, STDOUT, 0);
        assert_eq!(out.len(), 1);
        if let Decoded::Stream(m) = &out[0] {
            assert_eq!(m.kind, MessageKind::Reasoning);
            assert_eq!(m.phase, MessagePhase::Delta);
            assert_eq!(m.text, "hmm");
        } else {
            panic!("expected Stream");
        }
    }

    #[test]
    fn decode_message_update_with_unknown_subtype_yields_nothing() {
        let v = json(
            r#"{"type":"message_update","assistantMessageEvent":{"type":"image_delta","delta":"x"}}"#,
        );
        assert!(decode(&v, STDOUT, 0).is_empty());
    }

    #[test]
    fn decode_turn_end_concatenates_text_blocks_as_final() {
        let v = json(
            r#"{"type":"turn_end","message":{"content":[{"type":"text","text":"Hello "},{"type":"text","text":"world"}]}}"#,
        );
        let out = decode(&v, STDOUT, 0);
        assert_eq!(out.len(), 1);
        if let Decoded::Stream(m) = &out[0] {
            assert_eq!(m.text, "Hello world");
            assert_eq!(m.phase, MessagePhase::Final);
            assert_eq!(m.role, MessageRole::Assistant);
        } else {
            panic!("expected Stream");
        }
    }

    #[test]
    fn decode_turn_end_bare_string_content() {
        let v = json(r#"{"type":"turn_end","message":{"content":"plain"}}"#);
        let out = decode(&v, STDOUT, 0);
        assert_eq!(out.len(), 1);
        if let Decoded::Stream(m) = &out[0] {
            assert_eq!(m.text, "plain");
        } else {
            panic!("expected Stream");
        }
    }

    #[test]
    fn decode_turn_end_empty_text_yields_nothing() {
        let v = json(r#"{"type":"turn_end","message":{"content":[]}}"#);
        assert!(decode(&v, STDOUT, 0).is_empty());
    }

    #[test]
    fn decode_tool_execution_start_is_structured_tool_use() {
        let v = json(
            r#"{"type":"tool_execution_start","toolCallId":"call_1","toolName":"read_file","args":{"path":"a.rs"}}"#,
        );
        let out = decode(&v, STDOUT, 0);
        assert_eq!(out.len(), 1);
        match &out[0] {
            Decoded::ToolUse {
                call_id,
                tool_name,
                input,
            } => {
                assert_eq!(call_id, "call_1");
                assert_eq!(tool_name, "read_file");
                assert_eq!(input["path"], "a.rs");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn decode_tool_execution_end_is_structured_tool_result() {
        let v = json(
            r#"{"type":"tool_execution_end","toolCallId":"call_1","result":{"ok":true},"isError":false}"#,
        );
        let out = decode(&v, STDOUT, 0);
        assert_eq!(out.len(), 1);
        match &out[0] {
            Decoded::ToolResult {
                call_id,
                output,
                is_error,
            } => {
                assert_eq!(call_id, "call_1");
                assert_eq!(output["ok"], true);
                assert!(!*is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn decode_tool_execution_end_error_flag() {
        let v = json(
            r#"{"type":"tool_execution_end","toolCallId":"c2","result":"boom","isError":true}"#,
        );
        let out = decode(&v, STDOUT, 0);
        if let Decoded::ToolResult { is_error, .. } = &out[0] {
            assert!(*is_error);
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn decode_compaction_auto_retry_extension_are_status() {
        let compaction = json(
            r#"{"type":"compaction_end","result":{"tokensBefore":1000,"estimatedTokensAfter":500}}"#,
        );
        let m = decode(&compaction, STDOUT, 0);
        assert_eq!(m.len(), 1);
        if let Decoded::Stream(s) = &m[0] {
            assert_eq!(s.kind, MessageKind::Status);
            assert_eq!(s.role, MessageRole::System);
            assert!(s.text.contains("compacted"));
        } else {
            panic!("expected Stream");
        }

        let retry = json(r#"{"type":"auto_retry_start","errorMessage":"transient 429"}"#);
        let m = decode(&retry, STDOUT, 0);
        if let Decoded::Stream(s) = &m[0] {
            assert_eq!(s.text, "transient 429");
        } else {
            panic!("expected Stream");
        }

        let ext = json(r#"{"type":"extension_error","error":"ext failed"}"#);
        let m = decode(&ext, STDOUT, 0);
        if let Decoded::Stream(s) = &m[0] {
            assert_eq!(s.text, "ext failed");
        } else {
            panic!("expected Stream");
        }
    }

    #[test]
    fn decode_lifecycle_and_unknown_events_yield_nothing() {
        for t in [
            "agent_settled",
            "agent_end",
            "response",
            "message_end",
            "tool_execution_update",
            "bogus",
        ] {
            let v = json(&format!(r#"{{"type":"{t}"}}"#));
            assert!(
                decode(&v, STDOUT, 0).is_empty(),
                "{t} should not produce decode frames"
            );
        }
    }

    // ---- usage ------------------------------------------------------------

    #[test]
    fn extract_usage_from_turn_end() {
        let v = json(
            r#"{"type":"turn_end","message":{"usage":{"input":12,"output":34,"cacheRead":5,"cacheWrite":7}}}"#,
        );
        let (usage, source) = extract_usage(&v).unwrap();
        assert_eq!(source, UsageSource::Pi);
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 34);
        assert_eq!(usage.cache_read_tokens, Some(5));
        assert_eq!(usage.cache_creation_tokens, Some(7));
        assert!(usage.total_tokens.is_none());
    }

    #[test]
    fn extract_usage_from_message_end() {
        let v = json(r#"{"type":"message_end","message":{"usage":{"input":1,"output":2}}}"#);
        let (usage, source) = extract_usage(&v).unwrap();
        assert_eq!(source, UsageSource::Pi);
        assert_eq!(usage.input_tokens, 1);
        assert_eq!(usage.output_tokens, 2);
        assert!(usage.cache_read_tokens.is_none());
    }

    #[test]
    fn extract_usage_ignores_other_event_types() {
        assert!(extract_usage(&json(r#"{"type":"message_update"}"#)).is_none());
        assert!(extract_usage(&json(r#"{"type":"turn_end","message":{}}"#)).is_none());
        assert!(extract_usage(&json(r#"{"type":"turn_end"}"#)).is_none());
    }

    // ---- settle detection -------------------------------------------------

    #[test]
    fn is_settled_detects_terminal_events() {
        assert!(is_settled_event(&json(r#"{"type":"agent_settled"}"#)));
        assert!(is_settled_event(&json(r#"{"type":"agent_end"}"#)));
        assert!(!is_settled_event(&json(r#"{"type":"turn_end"}"#)));
        assert!(!is_settled_event(&json(r#"{"type":"response"}"#)));
        assert!(!is_settled_event(&json(r#"{}"#)));
    }

    // ---- quota ------------------------------------------------------------

    #[test]
    fn extract_quota_json_error_rate_limit() {
        let p = AgentEventPayload::JsonLine(json(
            r#"{"type":"error","error":{"type":"rate_limit_error","message":"Rate limited. Try later."}}"#,
        ));
        let info = extract_quota(&p).unwrap();
        assert_eq!(info.agent_key, "pi");
        assert!(info.raw_message.contains("Rate limited"));
    }

    #[test]
    fn extract_quota_failed_response_shape() {
        let p = AgentEventPayload::JsonLine(json(
            r#"{"type":"response","success":false,"error":{"message":"overloaded, retry later"}}"#,
        ));
        let info = extract_quota(&p).unwrap();
        assert_eq!(info.agent_key, "pi");
        assert!(info.raw_message.contains("overloaded"));
    }

    #[test]
    fn extract_quota_successful_response_is_none() {
        let p =
            AgentEventPayload::JsonLine(json(r#"{"type":"response","success":true,"data":{}}"#));
        assert!(extract_quota(&p).is_none());
    }

    #[test]
    fn extract_quota_rawline_429() {
        let p = AgentEventPayload::RawLine("HTTP 429 Too Many Requests".to_string());
        let info = extract_quota(&p).unwrap();
        assert_eq!(info.agent_key, "pi");
    }

    #[test]
    fn extract_quota_no_match_is_none() {
        let p = AgentEventPayload::RawLine("just normal output".to_string());
        assert!(extract_quota(&p).is_none());
        let p = AgentEventPayload::JsonLine(json(r#"{"type":"tool_execution_end","result":"ok"}"#));
        assert!(extract_quota(&p).is_none());
    }

    // ---- prompt framing + argv -------------------------------------------

    #[test]
    fn prompt_command_is_newline_terminated_jsonl() {
        let cmd = prompt_command("write a haiku");
        assert!(cmd.ends_with('\n'));
        let line = cmd.trim_end();
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["type"], "prompt");
        assert_eq!(v["prompt"], "write a haiku");
    }

    #[test]
    fn prompt_command_escapes_embedded_newlines_and_quotes() {
        let cmd = prompt_command("line\nwith \"quotes\"");
        assert!(cmd.ends_with('\n'));
        // The whole payload (minus the trailing command newline) must be a
        // single valid JSON value — embedded newline must not split records.
        let line = cmd.trim_end_matches('\n');
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["prompt"], "line\nwith \"quotes\"");
    }

    #[test]
    fn argv_minimal_is_mode_rpc() {
        let argv = argv(ArgvCtx {
            model: None,
            yolo: true,
            stream: true,
            events_mode: true,
            session_id: None,
            envelope: None,
        });
        let s: Vec<&str> = argv.iter().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(s, vec!["pi", "--mode", "rpc"]);
    }

    #[test]
    fn argv_with_model_and_session() {
        let argv = argv(ArgvCtx {
            model: Some(&"anthropic/claude-sonnet-4".to_string()),
            yolo: false,
            stream: false,
            events_mode: false,
            session_id: Some("sess-9"),
            envelope: None,
        });
        let s: Vec<&str> = argv.iter().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(
            s,
            vec![
                "pi",
                "--mode",
                "rpc",
                "--model",
                "anthropic/claude-sonnet-4",
                "--session",
                "sess-9"
            ]
        );
    }

    #[test]
    fn argv_ignores_empty_model() {
        let argv = argv(ArgvCtx {
            model: Some(&"   ".to_string()),
            yolo: false,
            stream: false,
            events_mode: false,
            session_id: None,
            envelope: None,
        });
        let s: Vec<&str> = argv.iter().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(s, vec!["pi", "--mode", "rpc"]);
    }

    // ---- capability honesty (spec 014 testing decisions) -----------------

    #[test]
    fn capabilities_match_spec_vector() {
        let c = CAPABILITIES;
        assert!(
            c.bidirectional
                && c.interruptible
                && c.structured_tools
                && c.reasoning
                && c.resumable_sessions
        );
        assert!(!c.file_changes);
        assert!(!c.context_compression);
        assert!(!c.mcp_routing);
        assert!(!c.hooks);
        assert!(!c.server_tools);
        assert!(!c.subagents);
        assert!(!c.passive_capture);
        assert!(!c.supports_tool_policy);
    }

    /// Each `true` capability is backed by a decoder fixture that exercises it,
    /// so a capability can never drift ahead of what the decoder emits.
    #[test]
    fn every_true_capability_is_backed_by_decode_output() {
        // structured_tools -> ToolUse / ToolResult
        let tool = decode(
            &json(r#"{"type":"tool_execution_start","toolCallId":"t","toolName":"x","args":{}}"#),
            STDOUT,
            0,
        );
        assert!(tool.iter().any(|d| matches!(d, Decoded::ToolUse { .. })));
        // reasoning -> a Reasoning StreamMessage
        let reason = decode(
            &json(
                r#"{"type":"message_update","assistantMessageEvent":{"type":"thinking_delta","delta":"y"}}"#,
            ),
            STDOUT,
            0,
        );
        assert!(reason.iter().any(|d| matches!(
            d,
            Decoded::Stream(StreamMessage {
                kind: MessageKind::Reasoning,
                ..
            })
        )));
    }
}
