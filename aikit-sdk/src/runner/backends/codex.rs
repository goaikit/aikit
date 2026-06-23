//! Codex backend: `codex exec --json` over a subprocess.
//!
//! Phase A relocates the existing decode/usage/quota/argv logic verbatim.
//! Phase B will port `aikit-agent-codex`'s `app-server` JSON-RPC client as a
//! bidirectional `Transport` (see spec 006).

use std::ffi::OsString;

use crate::runner::backends::argv_spec::{ArgvCtx, ArgvSpec, SessionMode};
use crate::runner::backends::quota_match::{match_quota, JsonPat, RawPat};
use crate::runner::capabilities::BackendCapabilities;
use crate::runner::types::{
    AgentEventPayload, AgentEventStream, MessageKind, MessagePhase, MessageRole, QuotaExceededInfo,
    StreamMessage, TokenUsage, UsageSource,
};

pub(crate) const KEY: &str = "codex";

pub(crate) const BINARY_CANDIDATES: &[&str] = &["codex"];

pub(crate) const CAPABILITIES: BackendCapabilities = BackendCapabilities::NONE
    .with_bidirectional()
    .with_structured_tools()
    .with_reasoning()
    .with_file_changes()
    .with_interruptible()
    .with_resumable_sessions();

const SPEC: ArgvSpec = ArgvSpec {
    binary: "codex",
    model_flag: "-m",
    yolo_flag: Some("--yolo"),
    session_mode: SessionMode::Positional,
};

pub(crate) fn decode(
    value: &serde_json::Value,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> Vec<StreamMessage> {
    let mut results = Vec::new();
    let line_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    let mk = |text: String, role: MessageRole, kind: MessageKind| StreamMessage {
        text,
        phase: MessagePhase::Final,
        role,
        kind,
        source: stream,
        raw_line_seq,
        turn_id: None,
    };

    match line_type {
        // ── Current codex-cli "thread/turn/item" schema (>= 0.13x) ──────────────
        // Emit on terminal item state only (`item.completed`) to avoid duplicating
        // the streamed `item.started` event for the same item.
        "item.completed" => {
            if let Some(item) = value.get("item") {
                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match item_type {
                    "agent_message" => {
                        if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                            results.push(mk(
                                t.to_string(),
                                MessageRole::Assistant,
                                MessageKind::Message,
                            ));
                        }
                    }
                    "reasoning" => {
                        if let Some(t) = item
                            .get("text")
                            .and_then(|v| v.as_str())
                            .or_else(|| item.get("summary").and_then(|v| v.as_str()))
                        {
                            results.push(mk(
                                t.to_string(),
                                MessageRole::Assistant,
                                MessageKind::Reasoning,
                            ));
                        }
                    }
                    "command_execution" => {
                        if let Some(cmd) = item.get("command").and_then(|v| v.as_str()) {
                            results.push(mk(
                                cmd.to_string(),
                                MessageRole::Tool,
                                MessageKind::Message,
                            ));
                        }
                        if let Some(out) = item.get("aggregated_output").and_then(|v| v.as_str()) {
                            if !out.trim().is_empty() {
                                results.push(mk(
                                    out.to_string(),
                                    MessageRole::Tool,
                                    MessageKind::ToolOutput,
                                ));
                            }
                        }
                    }
                    "file_change" => {
                        if let Some(arr) = item.get("changes").and_then(|c| c.as_array()) {
                            let summary = arr
                                .iter()
                                .filter_map(|c| {
                                    let path = c.get("path").and_then(|v| v.as_str())?;
                                    let kind =
                                        c.get("kind").and_then(|v| v.as_str()).unwrap_or("change");
                                    Some(format!("{kind} {path}"))
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            if !summary.is_empty() {
                                results.push(mk(
                                    format!("file_change: {summary}"),
                                    MessageRole::Tool,
                                    MessageKind::Message,
                                ));
                            }
                        }
                    }
                    // Unknown item type: surface any text it carries.
                    _ => {
                        if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                            results.push(mk(
                                t.to_string(),
                                MessageRole::Assistant,
                                MessageKind::Message,
                            ));
                        }
                    }
                }
            }
        }
        // ── Failure events — surface so a failed turn is never a silent empty run ──
        "error" => {
            if let Some(msg) = value.get("message").and_then(|v| v.as_str()) {
                results.push(mk(
                    msg.to_string(),
                    MessageRole::System,
                    MessageKind::Status,
                ));
            }
        }
        "turn.failed" => {
            if let Some(msg) = value
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|v| v.as_str())
            {
                results.push(mk(
                    msg.to_string(),
                    MessageRole::System,
                    MessageKind::Status,
                ));
            }
        }
        // ── Lifecycle frames carry no message text — intentionally ignored ──────
        "thread.started" | "turn.started" | "turn.completed" | "item.started" => {}
        // ── Legacy codex schema (older CLI): message / action / output ──────────
        "message" => {
            let role_str = value.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(content) = value.get("content").and_then(|v| v.as_str()) {
                let role = match role_str {
                    "system" => MessageRole::System,
                    "user" => MessageRole::User,
                    _ => MessageRole::Assistant,
                };
                let kind = if role_str == "system" {
                    MessageKind::Status
                } else {
                    MessageKind::Message
                };
                results.push(mk(content.to_string(), role, kind));
            }
        }
        "action" => {
            if let Some(cmd) = value.get("command").and_then(|v| v.as_str()) {
                results.push(mk(cmd.to_string(), MessageRole::Tool, MessageKind::Message));
            }
        }
        "output" => {
            if let Some(stdout) = value.get("stdout").and_then(|v| v.as_str()) {
                results.push(mk(
                    stdout.to_string(),
                    MessageRole::Tool,
                    MessageKind::ToolOutput,
                ));
            }
        }
        // ── Unknown line type: legacy fallback for a top-level `item.text` ──────
        _ => {
            if let Some(text) = value
                .get("item")
                .and_then(|item| item.get("text"))
                .and_then(|v| v.as_str())
            {
                results.push(mk(
                    text.to_string(),
                    MessageRole::Assistant,
                    MessageKind::Message,
                ));
            }
        }
    }

    results
}

pub(crate) fn extract_usage(line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
    if line.get("type")?.as_str()? != "turn.completed" {
        return None;
    }
    let usage = line.get("usage")?;
    let input_tokens = usage.get("input_tokens")?.as_u64()?;
    let output_tokens = usage.get("output_tokens")?.as_u64()?;
    let cache_read_tokens = usage.get("cached_input_tokens").and_then(|v| v.as_u64());
    Some((
        TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens: None,
            cache_read_tokens,
            cache_creation_tokens: None,
            reasoning_tokens: None,
        },
        UsageSource::Codex,
    ))
}

static RAW_PATS: &[RawPat] = &[RawPat::Any(&[
    "rate limit reached",
    "tokens per min",
    "429 too many requests",
    "rate_limit_exceeded",
])];

static JSON_PATS: &[JsonPat] = &[JsonPat::CodexJsonError];

pub(crate) fn extract_quota(payload: &AgentEventPayload) -> Option<QuotaExceededInfo> {
    match_quota(KEY, RAW_PATS, JSON_PATS, payload)
}

pub(crate) fn argv(ctx: ArgvCtx) -> Vec<OsString> {
    let mut argv = match ctx.session_id {
        Some(id) => vec![
            OsString::from(SPEC.binary),
            OsString::from("resume"),
            OsString::from(id),
        ],
        None => vec![OsString::from(SPEC.binary), OsString::from("exec")],
    };
    SPEC.push_model(&mut argv, ctx.model);
    SPEC.push_yolo(&mut argv, ctx.yolo);
    argv.extend_from_slice(&[
        OsString::from("--json"),
        OsString::from("--"),
        OsString::from("-"),
    ]);
    argv
}
