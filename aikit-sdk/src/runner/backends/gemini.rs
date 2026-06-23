//! Gemini backend: `gemini --output-format stream-json` over a subprocess.

use std::ffi::OsString;

use crate::runner::backends::argv_spec::{ArgvCtx, ArgvSpec, SessionMode};
use crate::runner::backends::quota_match::{match_quota, JsonPat, RawPat};
use crate::runner::capabilities::BackendCapabilities;
use crate::runner::types::{
    AgentEventPayload, AgentEventStream, MessageKind, MessagePhase, MessageRole, QuotaExceededInfo,
    StreamMessage, TokenUsage, UsageSource,
};

pub(crate) const KEY: &str = "gemini";

pub(crate) const BINARY_CANDIDATES: &[&str] = &["gemini"];

pub(crate) const CAPABILITIES: BackendCapabilities = BackendCapabilities::NONE;

const SPEC: ArgvSpec = ArgvSpec {
    binary: "gemini",
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

    // Current gemini CLI stream-json shape:
    //   {"type":"message","role":"assistant","content":"...","delta":true}
    //   {"type":"message","role":"assistant","content":"..."}                  (final)
    //   {"type":"result","stats":{...}}                                        (run done)
    //   {"type":"init","session_id":"..."}                                     (ignored)
    //   {"type":"message","role":"user","content":"..."}                       (echo, skip)
    if line_type == "message" && value.get("role").and_then(|v| v.as_str()) == Some("assistant") {
        if let Some(text) = value.get("content").and_then(|v| v.as_str()) {
            let is_delta = value
                .get("delta")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let turn_id = value
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            results.push(StreamMessage {
                text: text.to_string(),
                phase: if is_delta {
                    MessagePhase::Delta
                } else {
                    MessagePhase::Final
                },
                role: MessageRole::Assistant,
                kind: MessageKind::Message,
                source: stream,
                raw_line_seq,
                turn_id,
            });
        }
    }

    // Legacy/alternative gemini shape (Gemini API direct):
    //   {"candidates":[{"content":{"parts":[{"text":"..."}]}}]}
    if let Some(candidates) = value.get("candidates").and_then(|v| v.as_array()) {
        for candidate in candidates {
            if let Some(parts) = candidate
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
            {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
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

    // Original `{"type":"result","result":"..."}` shape (some gemini versions)
    if line_type == "result" {
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
    let stats = line.get("stats")?;
    let input_tokens = stats.get("input_tokens")?.as_u64()?;
    let output_tokens = stats.get("output_tokens")?.as_u64()?;
    let total_tokens = stats.get("total_tokens").and_then(|v| v.as_u64());
    let cache_read_tokens = stats.get("cached").and_then(|v| v.as_u64());
    Some((
        TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens,
            cache_read_tokens,
            cache_creation_tokens: None,
            reasoning_tokens: None,
        },
        UsageSource::Gemini,
    ))
}

static RAW_PATS: &[RawPat] = &[
    RawPat::Any(&["resource_exhausted"]),
    RawPat::Any(&["rate limit exceeded"]),
    RawPat::All(&["429", "quota exceeded"]),
    RawPat::All(&["429", "rate limit"]),
    RawPat::All(&["error", "429", "rate limit"]),
    RawPat::All(&["error", "429", "'code'"]),
];

static JSON_PATS: &[JsonPat] = &[JsonPat::GeminiErrorObject];

pub(crate) fn extract_quota(payload: &AgentEventPayload) -> Option<QuotaExceededInfo> {
    match_quota(KEY, RAW_PATS, JSON_PATS, payload)
}

pub(crate) fn argv(ctx: ArgvCtx) -> Vec<OsString> {
    let mut argv = vec![
        OsString::from(SPEC.binary),
        OsString::from("--prompt"),
        OsString::from("-"),
    ];
    if ctx.events_mode {
        argv.extend_from_slice(&[
            OsString::from("--output-format"),
            OsString::from("stream-json"),
            OsString::from("--yolo"),
        ]);
    }
    SPEC.push_model(&mut argv, ctx.model);
    SPEC.push_session_flag(&mut argv, ctx.session_id);
    argv
}
