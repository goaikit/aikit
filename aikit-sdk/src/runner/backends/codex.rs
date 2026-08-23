//! Codex backend: `codex exec --json` over a subprocess.
//!
//! Phase A relocates the existing decode/usage/quota/argv logic verbatim.
//! Phase B will port `aikit-agent-codex`'s `app-server` JSON-RPC client as a
//! bidirectional `Transport` (see spec 006).

use std::ffi::OsString;

use crate::runner::backend::Decoded;
use crate::runner::backends::argv_spec::{ArgvCtx, ArgvSpec, SessionMode};
use crate::runner::backends::quota_match::{match_quota, JsonPat, RawPat};
use crate::runner::capabilities::BackendCapabilities;
use crate::runner::types::{
    AgentEventPayload, AgentEventStream, MessageKind, MessagePhase, MessageRole, QuotaExceededInfo,
    SandboxPolicy, StreamMessage, TokenUsage, UsageSource,
};

pub(crate) const KEY: &str = "codex";

pub(crate) const BINARY_CANDIDATES: &[&str] = &["codex"];

// `passive_capture` flips on only when both `agent-adapters` and the
// `codex` adapter feature are enabled. Spec 010 §17.2.
#[cfg(all(feature = "agent-adapters", feature = "codex"))]
pub(crate) const CAPABILITIES: BackendCapabilities = BackendCapabilities::NONE
    .with_bidirectional()
    .with_structured_tools()
    .with_reasoning()
    .with_file_changes()
    .with_interruptible()
    .with_resumable_sessions()
    .with_passive_capture();

#[cfg(not(all(feature = "agent-adapters", feature = "codex")))]
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
) -> Vec<Decoded> {
    let mut results = Vec::new();
    let line_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    let mk = |text: String, role: MessageRole, kind: MessageKind| {
        Decoded::Stream(StreamMessage {
            text,
            phase: MessagePhase::Final,
            role,
            kind,
            source: stream,
            raw_line_seq,
            turn_id: None,
        })
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
                            let call_id = item_call_id(item, raw_line_seq);
                            results.push(Decoded::ToolUse {
                                call_id: call_id.clone(),
                                tool_name: "shell".to_string(),
                                input: serde_json::json!({ "command": cmd }),
                            });
                            if let Some(out) =
                                item.get("aggregated_output").and_then(|v| v.as_str())
                            {
                                if !out.trim().is_empty() {
                                    results.push(Decoded::ToolResult {
                                        call_id,
                                        output: serde_json::json!(out),
                                        is_error: item_is_error(item),
                                    });
                                }
                            }
                        }
                    }
                    "file_change" => {
                        if let Some(arr) = item.get("changes").and_then(|c| c.as_array()) {
                            if !arr.is_empty() {
                                results.push(Decoded::ToolUse {
                                    call_id: item_call_id(item, raw_line_seq),
                                    tool_name: "file_change".to_string(),
                                    input: serde_json::json!({ "changes": arr }),
                                });
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
            if value.get("action").and_then(|v| v.as_str()) == Some("shell") {
                if let Some(cmd) = value.get("command").and_then(|v| v.as_str()) {
                    results.push(Decoded::ToolUse {
                        call_id: legacy_call_id(value, "shell"),
                        tool_name: "shell".to_string(),
                        input: serde_json::json!({ "command": cmd }),
                    });
                }
            }
        }
        "output" => {
            let stdout = value.get("stdout").and_then(|v| v.as_str());
            let stderr = value.get("stderr").and_then(|v| v.as_str());
            if stdout.is_some_and(|s| !s.trim().is_empty())
                || stderr.is_some_and(|s| !s.trim().is_empty())
            {
                results.push(Decoded::ToolResult {
                    call_id: legacy_call_id(value, "shell"),
                    output: serde_json::json!({
                        "stdout": stdout.unwrap_or(""),
                        "stderr": stderr.unwrap_or("")
                    }),
                    is_error: stderr.is_some_and(|s| !s.trim().is_empty()),
                });
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

fn item_call_id(item: &serde_json::Value, raw_line_seq: u64) -> String {
    item.get("id")
        .and_then(|v| v.as_str())
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("codex-item-{raw_line_seq}"))
}

fn legacy_call_id(value: &serde_json::Value, tool_name: &str) -> String {
    value
        .get("id")
        .or_else(|| value.get("call_id"))
        .and_then(|v| v.as_str())
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("codex-legacy-{tool_name}"))
}

fn item_is_error(item: &serde_json::Value) -> bool {
    if let Some(exit_code) = item.get("exit_code").and_then(|v| v.as_i64()) {
        return exit_code != 0;
    }
    matches!(
        item.get("status").and_then(|v| v.as_str()),
        Some("failed" | "error" | "cancelled" | "canceled")
    )
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

    // spec 013 D1: an explicit `--sandbox` subsumes `--yolo` — codex's
    // workspace-write / danger-full-access already auto-approve within bounds.
    // When no envelope sandbox is requested, preserve the legacy `--yolo` path
    // (every existing argv test exercises this branch).
    let explicit_sandbox = ctx.envelope.and_then(|e| e.sandbox);
    if let Some(policy) = explicit_sandbox {
        argv.push(OsString::from("--sandbox"));
        argv.push(OsString::from(codex_sandbox_token(policy)));
    } else {
        SPEC.push_yolo(&mut argv, ctx.yolo);
    }

    // spec 013 D2/D6: honored knobs → native flags.
    if let Some(e) = ctx.envelope {
        if let Some(dir) = e.working_dir.as_deref() {
            argv.push(OsString::from("--cd"));
            argv.push(dir.as_os_str().to_owned());
        }
        for root in &e.extra_writable_roots {
            argv.push(OsString::from("--add-dir"));
            argv.push(root.as_os_str().to_owned());
        }
        if e.skip_git_repo_check {
            argv.push(OsString::from("--skip-git-repo-check"));
        }
        if e.ephemeral {
            argv.push(OsString::from("--ephemeral"));
        }
        if e.bare {
            argv.push(OsString::from("--ignore-user-config"));
        }
    }

    argv.extend_from_slice(&[
        OsString::from("--json"),
        OsString::from("--"),
        OsString::from("-"),
    ]);
    argv
}

/// Map a common [`SandboxPolicy`] onto codex's native `--sandbox` vocabulary
/// (spec 013 D1 sandbox-mapping table).
fn codex_sandbox_token(policy: SandboxPolicy) -> &'static str {
    match policy {
        SandboxPolicy::ReadOnly => "read-only",
        SandboxPolicy::BoundedWrite => "workspace-write",
        SandboxPolicy::Unrestricted => "danger-full-access",
    }
}
