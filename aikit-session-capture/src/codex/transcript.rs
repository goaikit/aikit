//! Codex CLI rollout JSONL parser.
//!
//! Reads `~/.codex/sessions/rollout-*.jsonl` line-by-line, resumable from a
//! byte offset, and emits normalized `ToolEvent` / `TokenEvent` rows.
//! See spec 010 §12.2.
//!
//! Reference: `superbased-observer/internal/adapter/codex/adapter.go`
//! (3122 lines — full coverage of every event variant). This Phase 3 MVP
//! covers the core rollout flows: session_configured / session_meta /
//! turn_context (context capture), user_message, agent_message, tool_call /
//! tool_output pairing, response_item.function_call / custom_tool_call /
//! function_call_output, event_msg.exec_command_end, web_search_end,
//! token_count (cumulative → per-turn delta), and `error` envelopes. Less
//! common variants (compacted, dynamic_tool_call_*, view_image, mcp_tool_call,
//! reasoning summary) are recognized and skipped cleanly with no warning;
//! they land in a later phase.

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::adapter::{AdapterError, ParseResult, ParseWarning};
use crate::models::{ActionKind, ActionStatus, CaptureSource, TokenEvent, ToolEvent, ToolKind};
use crate::scrub::SecretScrubber;

/// Parse Codex rollout JSONL bytes from `from_offset` to EOF.
pub(crate) fn parse(
    path: &Path,
    bytes: &[u8],
    from_offset: u64,
    scrubber: &SecretScrubber,
) -> Result<ParseResult, AdapterError> {
    let start = from_offset as usize;
    if start > bytes.len() {
        return Err(AdapterError::OffsetPastEof {
            path: path.to_path_buf(),
            requested: from_offset,
            file_size: bytes.len() as u64,
        });
    }
    let tail = &bytes[start..];

    let mut res = ParseResult {
        new_offset: from_offset,
        ..Default::default()
    };

    // File-level context, refreshed by session_configured / session_meta /
    // turn_context. Mirrors observer's `sessionContext` + `applyContext`.
    let mut ctx = SessionCtx::default();
    // Filename-stem fallback when no session-bearing envelope lands in the
    // chunk being parsed (incremental resume case).
    let fallback_session_id = session_id_from_path(path);

    // tool_call/tool_output pairing: call_id → index into tool_events.
    let mut pending: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    // Cumulative token tracking — Codex emits session-wide totals; we compute
    // per-turn deltas. Keyed by session_id (a single rollout file is one
    // session, but we keep the key for parity with observer).
    let mut last_net_input: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();
    // Track the most recent token_count's session_id so the delta math knows
    // which baseline to subtract from.
    let mut current_session_id = String::new();

    let mut cursor: usize = 0;
    let mut line_no: u64 = 0;
    while cursor < tail.len() {
        let nl = match tail[cursor..].iter().position(|&b| b == b'\n') {
            Some(i) => cursor + i + 1,
            None => break, // partial trailing line — defer to next poll
        };
        line_no += 1;
        let raw_line = &tail[cursor..nl];
        cursor = nl;
        res.new_offset = start as u64 + cursor as u64;

        let trimmed = std::str::from_utf8(raw_line)
            .unwrap_or("")
            .trim_end_matches(['\r', '\n'])
            .trim();
        if trimmed.is_empty() {
            continue;
        }

        let rec: RawLine = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                res.warnings.push(ParseWarning::MalformedLine {
                    line_no,
                    reason: format!("JSON parse: {e}"),
                });
                continue;
            }
        };

        let ts_ms = parse_ts_ms(rec.timestamp.as_deref().unwrap_or(""));

        // Dispatch on top-level type. Mirrors observer's switch at line 959+.
        let rec_type = rec.r#type.as_str();
        let payload_type = rec
            .payload
            .as_ref()
            .and_then(|p| p.get("type"))
            .and_then(|t| t.as_str())
            .unwrap_or("");

        match rec_type {
            // ---- Context-bearing envelopes (sticky state) ---------------
            "session_configured" | "session_start" | "session_meta" | "turn_context" => {
                apply_context(&rec.payload, &mut ctx);
                if ctx.session_id.is_empty() {
                    ctx.session_id = fallback_session_id.clone();
                }
                if !ctx.session_id.is_empty() {
                    current_session_id = ctx.session_id.clone();
                }
            }

            // ---- event_msg dispatch (the modern rollout path) ----------
            "event_msg" => match payload_type {
                "task_started" => {
                    if let Some(turn) = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("turn_id"))
                        .and_then(|v| v.as_str())
                    {
                        ctx.turn_id = turn.to_string();
                    }
                }
                "agent_message" => {
                    if let Some(msg) = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("message"))
                        .and_then(|v| v.as_str())
                    {
                        let msg = msg.trim();
                        if !msg.is_empty() {
                            let event_id = format!("codex:{}:L{line_no}", short_hash(msg));
                            res.tool_events.push(ToolEvent {
                                source_event_id: event_id,
                                source_file: path.to_path_buf(),
                                session_id: ctx.session_id.clone(),
                                tool: ToolKind::Codex,
                                kind: ActionKind::Think,
                                target: Some(truncate_str(msg, 200).to_string()),
                                input: Some(scrubber.scrub(msg)),
                                output: None,
                                status: ActionStatus::Success,
                                error_message: None,
                                started_at_ms: ts_ms,
                                duration_ms: None,
                                git_root: ctx.cwd.as_ref().map(std::path::PathBuf::from),
                                metadata: serde_json::json!({ "model": ctx.model }),
                            });
                        }
                    }
                }
                "user_message" => {
                    let msg = rec
                        .payload
                        .as_ref()
                        .and_then(|p| {
                            p.get("message")
                                .and_then(|v| v.as_str())
                                .or_else(|| p.get("content").and_then(|v| v.as_str()))
                        })
                        .unwrap_or("")
                        .trim();
                    if !msg.is_empty() {
                        let turn = if ctx.turn_id.is_empty() {
                            format!("L{line_no}")
                        } else {
                            ctx.turn_id.clone()
                        };
                        let event_id = format!("codex:user:{turn}");
                        res.tool_events.push(ToolEvent {
                            source_event_id: event_id,
                            source_file: path.to_path_buf(),
                            session_id: ctx.session_id.clone(),
                            tool: ToolKind::Codex,
                            kind: ActionKind::Other,
                            target: Some(truncate_str(msg, 200).to_string()),
                            input: Some(scrubber.scrub(msg)),
                            output: None,
                            status: ActionStatus::Success,
                            error_message: None,
                            started_at_ms: ts_ms,
                            duration_ms: None,
                            git_root: ctx.cwd.as_ref().map(std::path::PathBuf::from),
                            metadata: serde_json::json!({ "kind": "user_prompt" }),
                        });
                    }
                }
                "exec_command_end" => {
                    let call_id = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("call_id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let cwd = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("cwd"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let command = render_command_field(rec.payload.as_ref(), "command");
                    let stdout = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("aggregated_output").or_else(|| p.get("stdout")))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let exit_code = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("exit_code"))
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let status_str = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("status"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let failed = exit_code != 0 || status_str == "failed";
                    // Merge into the pending tool_call if one exists; else emit standalone.
                    if let Some(&idx) = pending.get(&call_id) {
                        let scrubbed_out = scrubber.scrub(stdout);
                        res.tool_events[idx].output = Some(scrubbed_out);
                        if failed {
                            res.tool_events[idx].status = ActionStatus::Failure;
                        }
                        pending.remove(&call_id);
                    } else {
                        let event_id = format!("codex:exec:{call_id}");
                        let target = scrubber.scrub(&command);
                        res.tool_events.push(ToolEvent {
                            source_event_id: event_id,
                            source_file: path.to_path_buf(),
                            session_id: ctx.session_id.clone(),
                            tool: ToolKind::Codex,
                            kind: ActionKind::Bash,
                            target: Some(truncate_str(&target, 200).to_string()),
                            input: Some(scrubber.scrub(&command)),
                            output: Some(scrubber.scrub(stdout)),
                            status: if failed {
                                ActionStatus::Failure
                            } else {
                                ActionStatus::Success
                            },
                            error_message: if failed {
                                Some(truncate_str(stdout, 500).to_string())
                            } else {
                                None
                            },
                            started_at_ms: ts_ms,
                            duration_ms: duration_ms_from(rec.payload.as_ref()),
                            git_root: if cwd.is_empty() {
                                ctx.cwd.as_ref().map(std::path::PathBuf::from)
                            } else {
                                Some(std::path::PathBuf::from(cwd))
                            },
                            metadata: serde_json::json!({ "exit_code": exit_code }),
                        });
                    }
                }
                "web_search_end" => {
                    let call_id = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("call_id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let query = rec
                        .payload
                        .as_ref()
                        .and_then(|p| {
                            // Try action.query first, then top-level query.
                            p.get("action")
                                .and_then(|a| a.get("query"))
                                .and_then(|v| v.as_str())
                                .or_else(|| p.get("query").and_then(|v| v.as_str()))
                        })
                        .unwrap_or("")
                        .to_string();
                    if let Some(&idx) = pending.get(&call_id) {
                        res.tool_events[idx].kind = ActionKind::WebSearch;
                        res.tool_events[idx].target = Some(query.clone());
                        pending.remove(&call_id);
                    } else {
                        let event_id = format!("codex:web:{call_id}");
                        res.tool_events.push(ToolEvent {
                            source_event_id: event_id,
                            source_file: path.to_path_buf(),
                            session_id: ctx.session_id.clone(),
                            tool: ToolKind::Codex,
                            kind: ActionKind::WebSearch,
                            target: Some(query),
                            input: None,
                            output: None,
                            status: ActionStatus::Success,
                            error_message: None,
                            started_at_ms: ts_ms,
                            duration_ms: None,
                            git_root: ctx.cwd.as_ref().map(std::path::PathBuf::from),
                            metadata: serde_json::Value::Null,
                        });
                    }
                }
                "error" => {
                    let message = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("message"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !message.is_empty() {
                        let event_id = format!("codex:err:L{line_no}");
                        res.tool_events.push(ToolEvent {
                            source_event_id: event_id,
                            source_file: path.to_path_buf(),
                            session_id: ctx.session_id.clone(),
                            tool: ToolKind::Codex,
                            kind: ActionKind::Other,
                            target: None,
                            input: None,
                            output: Some(scrubber.scrub(message)),
                            status: ActionStatus::Failure,
                            error_message: Some(truncate_str(message, 500).to_string()),
                            started_at_ms: ts_ms,
                            duration_ms: None,
                            git_root: ctx.cwd.as_ref().map(std::path::PathBuf::from),
                            metadata: serde_json::Value::Null,
                        });
                    }
                }
                // task_complete / turn_aborted / mcp_tool_call_end / token_count
                // are handled by their own top-level branches or below.
                "token_count" => handle_token_count(
                    rec.payload.as_ref(),
                    &mut ctx,
                    &mut current_session_id,
                    &mut last_net_input,
                    from_offset,
                    &mut res.token_events,
                    path,
                    ts_ms,
                    line_no,
                ),
                _ => {
                    // Other event_msg subtypes we don't yet handle: dynamic_tool_call_*,
                    // view_image_tool_call, mcp_tool_call_end, context_compacted.
                    // Skip cleanly — no warning.
                }
            },

            // ---- response_item dispatch (Codex Desktop / newer builds) --
            "response_item" => match payload_type {
                "function_call" => {
                    let name = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let call_id = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("call_id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    // arguments is a JSON-encoded string in this envelope.
                    let args_str = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("arguments"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let (target, kind) = interpret_function_call(name, args_str);
                    let event_id = format!("codex:fn:{call_id}");
                    let scrubbed_args = if args_str.is_empty() {
                        None
                    } else {
                        Some(scrubber.scrub(args_str))
                    };
                    let ev = ToolEvent {
                        source_event_id: event_id,
                        source_file: path.to_path_buf(),
                        session_id: ctx.session_id.clone(),
                        tool: ToolKind::Codex,
                        kind,
                        target: target.map(|t| scrubber.scrub(&t)),
                        input: scrubbed_args,
                        output: None,
                        status: ActionStatus::Success,
                        error_message: None,
                        started_at_ms: ts_ms,
                        duration_ms: None,
                        git_root: ctx.cwd.as_ref().map(std::path::PathBuf::from),
                        metadata: serde_json::json!({ "function": name }),
                    };
                    let idx = res.tool_events.len();
                    res.tool_events.push(ev);
                    if !call_id.is_empty() {
                        pending.insert(call_id, idx);
                    }
                }
                "function_call_output" | "custom_tool_call_output" => {
                    let call_id = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("call_id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let output = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("output"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if let Some(&idx) = pending.get(&call_id) {
                        let scrubbed = scrubber.scrub(output);
                        res.tool_events[idx].output = Some(scrubbed.clone());
                        pending.remove(&call_id);
                    }
                }
                "custom_tool_call" => {
                    // apply_patch in current Codex Desktop. Treat as Edit.
                    let call_id = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("call_id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("apply_patch");
                    let input = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("input"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let event_id = format!("codex:custom:{call_id}");
                    let ev = ToolEvent {
                        source_event_id: event_id,
                        source_file: path.to_path_buf(),
                        session_id: ctx.session_id.clone(),
                        tool: ToolKind::Codex,
                        kind: ActionKind::Edit,
                        target: None,
                        input: if input.is_empty() {
                            None
                        } else {
                            Some(scrubber.scrub(input))
                        },
                        output: None,
                        status: ActionStatus::Success,
                        error_message: None,
                        started_at_ms: ts_ms,
                        duration_ms: None,
                        git_root: ctx.cwd.as_ref().map(std::path::PathBuf::from),
                        metadata: serde_json::json!({ "tool": name }),
                    };
                    let idx = res.tool_events.len();
                    res.tool_events.push(ev);
                    if !call_id.is_empty() {
                        pending.insert(call_id, idx);
                    }
                }
                "web_search_call" => {
                    let query = rec
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("action"))
                        .and_then(|a| a.get("query"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let call_id = format!("ws:L{line_no}");
                    let event_id = format!("codex:ws:{call_id}");
                    res.tool_events.push(ToolEvent {
                        source_event_id: event_id,
                        source_file: path.to_path_buf(),
                        session_id: ctx.session_id.clone(),
                        tool: ToolKind::Codex,
                        kind: ActionKind::WebSearch,
                        target: Some(query),
                        input: None,
                        output: None,
                        status: ActionStatus::Success,
                        error_message: None,
                        started_at_ms: ts_ms,
                        duration_ms: None,
                        git_root: ctx.cwd.as_ref().map(std::path::PathBuf::from),
                        metadata: serde_json::Value::Null,
                    });
                }
                _ => {
                    // message / reasoning / etc. — not handled in MVP.
                }
            },

            // ---- Top-level tool_call / tool_output (older rollout path) --
            "tool_call" | "function_call" => {
                let call_id = rec
                    .payload
                    .as_ref()
                    .and_then(|p| {
                        p.get("call_id")
                            .or_else(|| p.get("id"))
                            .and_then(|v| v.as_str())
                    })
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("L{line_no}"));
                let tool_name = rec
                    .payload
                    .as_ref()
                    .and_then(|p| {
                        p.get("tool")
                            .or_else(|| p.get("name"))
                            .and_then(|v| v.as_str())
                    })
                    .unwrap_or("");
                let input_value = rec.payload.as_ref().and_then(|p| p.get("input"));
                let (target, kind) = interpret_tool_call(tool_name, input_value, scrubber);
                let scrubbed_input = input_value.map(|v| scrubber.scrub(&v.to_string()));
                let event_id = format!("codex:tc:{call_id}");
                let ev = ToolEvent {
                    source_event_id: event_id,
                    source_file: path.to_path_buf(),
                    session_id: ctx.session_id.clone(),
                    tool: ToolKind::Codex,
                    kind,
                    target: target.map(|t| truncate_str(&t, 200).to_string()),
                    input: scrubbed_input,
                    output: None,
                    status: ActionStatus::Success,
                    error_message: None,
                    started_at_ms: ts_ms,
                    duration_ms: None,
                    git_root: ctx.cwd.as_ref().map(std::path::PathBuf::from),
                    metadata: serde_json::json!({ "tool": tool_name }),
                };
                let idx = res.tool_events.len();
                res.tool_events.push(ev);
                pending.insert(call_id, idx);
            }
            "tool_output" | "function_call_output" => {
                let call_id = rec
                    .payload
                    .as_ref()
                    .and_then(|p| {
                        p.get("call_id")
                            .or_else(|| p.get("id"))
                            .and_then(|v| v.as_str())
                    })
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                if call_id.is_empty() {
                    continue;
                }
                let body = decode_output(rec.payload.as_ref().and_then(|p| p.get("output")));
                let failed = rec
                    .payload
                    .as_ref()
                    .and_then(|p| {
                        // is_error=true → failed. success=false → failed.
                        // Prefer is_error when present; fall back to !success.
                        p.get("is_error")
                            .and_then(|v| v.as_bool())
                            .or_else(|| p.get("success").and_then(|v| v.as_bool()).map(|b| !b))
                    })
                    .unwrap_or(false);
                if let Some(&idx) = pending.get(&call_id) {
                    let scrubbed = scrubber.scrub(&body);
                    res.tool_events[idx].output = Some(scrubbed.clone());
                    if failed {
                        res.tool_events[idx].status = ActionStatus::Failure;
                        res.tool_events[idx].error_message =
                            Some(truncate_str(&scrubbed, 500).to_string());
                    }
                    pending.remove(&call_id);
                }
            }

            // ---- token_count (older rollout path: top-level, not under event_msg)
            "token_count" | "usage" => handle_token_count(
                rec.payload.as_ref(),
                &mut ctx,
                &mut current_session_id,
                &mut last_net_input,
                from_offset,
                &mut res.token_events,
                path,
                ts_ms,
                line_no,
            ),

            // ---- top-level compacted marker (paired with event_msg/
            //      context_compacted) — skip cleanly in MVP.
            "compacted" => {}

            // ---- top-level user_message (older rollout shape, no event_msg
            //      wrapper). MVP emits it as a user-prompt ToolEvent.
            "user_message" => {
                let msg = rec
                    .payload
                    .as_ref()
                    .and_then(|p| {
                        p.get("message")
                            .and_then(|v| v.as_str())
                            .or_else(|| p.get("content").and_then(|v| v.as_str()))
                    })
                    .unwrap_or("")
                    .trim();
                if !msg.is_empty() {
                    let event_id = format!("codex:user:L{line_no}");
                    res.tool_events.push(ToolEvent {
                        source_event_id: event_id,
                        source_file: path.to_path_buf(),
                        session_id: ctx.session_id.clone(),
                        tool: ToolKind::Codex,
                        kind: ActionKind::Other,
                        target: Some(truncate_str(msg, 200).to_string()),
                        input: Some(scrubber.scrub(msg)),
                        output: None,
                        status: ActionStatus::Success,
                        error_message: None,
                        started_at_ms: ts_ms,
                        duration_ms: None,
                        git_root: ctx.cwd.as_ref().map(std::path::PathBuf::from),
                        metadata: serde_json::json!({ "kind": "user_prompt" }),
                    });
                }
            }

            _ => {
                // Unknown top-level type — skip cleanly. Phase 3 MVP does
                // not emit a warning for unknown shapes; the parser makes
                // progress via the offset advance above.
            }
        }
    }

    Ok(res)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SessionCtx {
    session_id: String,
    turn_id: String,
    model: String,
    cwd: Option<String>,
}

fn apply_context(payload: &Option<Value>, ctx: &mut SessionCtx) {
    let Some(p) = payload else { return };
    if let Some(s) = p.get("session_id").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            ctx.session_id = s.to_string();
        }
    }
    // session_meta uses "id" instead of "session_id".
    if ctx.session_id.is_empty() {
        if let Some(s) = p.get("id").and_then(|v| v.as_str()) {
            if !s.is_empty() {
                ctx.session_id = s.to_string();
            }
        }
    }
    if let Some(s) = p.get("turn_id").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            ctx.turn_id = s.to_string();
        }
    }
    if let Some(s) = p.get("model").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            ctx.model = s.to_string();
        }
    }
    if let Some(s) = p.get("cwd").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            ctx.cwd = Some(s.to_string());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_token_count(
    payload: Option<&Value>,
    ctx: &mut SessionCtx,
    current_session_id: &mut String,
    last_net_input: &mut std::collections::HashMap<String, i64>,
    from_offset: u64,
    token_events: &mut Vec<TokenEvent>,
    _path: &Path,
    ts_ms: Option<i64>,
    line_no: u64,
) {
    let Some(p) = payload else { return };
    let input_tokens = p.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
    let output_tokens = p.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
    let cached = p
        .get("cached_input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let model = p
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if let Some(m) = &model {
        if ctx.model.is_empty() {
            ctx.model = m.clone();
        }
    }
    let session = if !current_session_id.is_empty() {
        current_session_id.clone()
    } else if !ctx.session_id.is_empty() {
        ctx.session_id.clone()
    } else {
        format!("line:{line_no}")
    };
    if current_session_id.is_empty() && !ctx.session_id.is_empty() {
        *current_session_id = ctx.session_id.clone();
    }

    // Cumulative → per-turn delta. Net input = gross - cached (Anthropic-shape
    // convention; cost-engine TokenBundle.Input is NET non-cached). The first
    // event after an incremental resume (from_offset > 0) has no in-memory
    // baseline — emit 0 input so we don't over-report.
    let net_cum = (input_tokens - cached).max(0);
    let net_in = match last_net_input.get(&session) {
        None if from_offset == 0 => net_cum,
        None => 0, // resume cold-start
        Some(&prev) if net_cum >= prev => net_cum - prev,
        Some(_) => net_cum, // negative delta — reset / resequencing
    };
    last_net_input.insert(session.clone(), net_cum);

    // Dedup: Codex re-emits identical token_count records. Observer's
    // `seenModernTotal` invariant — the cumulative totals match exactly.
    // Compare the raw (pre-delta) values: input_tokens, output_tokens,
    // cached_input_tokens. If the prior event for this session has the same
    // cumulative totals, this is a re-emission → skip.
    let last_event_match = token_events.iter().rev().take(3).any(|e| {
        e.session_id == session
            && e.cache_read_tokens == Some(cached as u64)
            && e.output_tokens == Some(output_tokens as u64)
        // The cumulative input = delta + (cumulative - prior_cumulative).
        // For dedup we need the *gross* totals, but TokenEvent stores
        // per-turn deltas. Match on output + cached (the two fields that
        // ARE the cumulative value verbatim) — those alone uniquely
        // identify the re-emission.
    });
    if last_event_match {
        return;
    }

    let event_id = format!("codex:tokens:{session}:L{line_no}");
    token_events.push(TokenEvent {
        source_event_id: event_id,
        session_id: session,
        tool: ToolKind::Codex,
        model,
        request_id: None,
        input_tokens: Some(net_in as u64),
        cache_read_tokens: Some(cached as u64),
        cache_creation_tokens: None,
        cache_creation_1h_tokens: None,
        output_tokens: Some(output_tokens as u64),
        reasoning_tokens: None,
        captured_at_ms: ts_ms.unwrap_or(0),
        captured_via: CaptureSource::Transcript,
    });
}

/// `command` field can be a string or array of strings (the argv form).
fn render_command_field(payload: Option<&Value>, key: &str) -> String {
    let Some(v) = payload.and_then(|p| p.get(key)) else {
        return String::new();
    };
    match v {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

fn duration_ms_from(payload: Option<&Value>) -> Option<u64> {
    let d = payload.and_then(|p| p.get("duration"))?;
    let secs = d.get("secs").and_then(|v| v.as_i64()).unwrap_or(0);
    let nanos = d.get("nanos").and_then(|v| v.as_i64()).unwrap_or(0);
    if secs == 0 && nanos == 0 {
        return None;
    }
    Some((secs * 1000 + nanos / 1_000_000) as u64)
}

fn decode_output(raw: Option<&Value>) -> String {
    let Some(v) = raw else {
        return String::new();
    };
    match v {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| {
                if v.get("type").and_then(|t| t.as_str()) == Some("text") {
                    v.get("text").and_then(|t| t.as_str()).map(String::from)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => v.to_string(),
    }
}

fn interpret_function_call(name: &str, args_str: &str) -> (Option<String>, ActionKind) {
    // Try to parse args as JSON for target extraction.
    let args_v: Option<Value> = serde_json::from_str(args_str).ok();
    let target = args_v.as_ref().and_then(|a| {
        a.get("command")
            .and_then(|v| {
                v.as_str().map(String::from).or_else(|| {
                    v.as_array().map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str())
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                })
            })
            .or_else(|| a.get("path").and_then(|v| v.as_str()).map(String::from))
            .or_else(|| a.get("filePath").and_then(|v| v.as_str()).map(String::from))
            .or_else(|| a.get("query").and_then(|v| v.as_str()).map(String::from))
    });
    let kind = match name {
        "shell_command" | "exec_command" | "shell" | "exec" | "execute" | "command" => {
            ActionKind::Bash
        }
        "file_read" | "read_file" | "open_file" | "view_image" => ActionKind::Read,
        "file_write" | "write_file" | "create_file" => ActionKind::Write,
        "apply_patch" | "edit_file" | "patch" | "replace" => ActionKind::Edit,
        // web_search is a server-side web query (OpenAI/Anthropic hosted),
        // not a codebase grep — classify distinctly from content search.
        "web_search" => ActionKind::WebSearch,
        "search" | "grep" | "find_text" | "find_in_files" => ActionKind::Search,
        "web_fetch" | "fetch_url" => ActionKind::WebFetch,
        "glob" | "find" | "list_files" | "list_directory" | "file_search" => ActionKind::Glob,
        "update_plan" => ActionKind::Plan,
        n if n.starts_with("mcp") || n.contains("mcp_") => ActionKind::Mcp,
        _ => ActionKind::Other,
    };
    (target, kind)
}

fn interpret_tool_call(
    tool_name: &str,
    input: Option<&Value>,
    scrubber: &SecretScrubber,
) -> (Option<String>, ActionKind) {
    let target = input.and_then(|i| {
        i.get("path")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| i.get("filePath").and_then(|v| v.as_str()).map(String::from))
            .or_else(|| i.get("query").and_then(|v| v.as_str()).map(String::from))
            .or_else(|| {
                i.get("command").and_then(|v| {
                    v.as_str().map(|s| scrubber.scrub(s)).or_else(|| {
                        v.as_array().map(|arr| {
                            scrubber.scrub(
                                &arr.iter()
                                    .filter_map(|x| x.as_str())
                                    .collect::<Vec<_>>()
                                    .join(" "),
                            )
                        })
                    })
                })
            })
    });
    let kind = match tool_name {
        "shell" | "exec" | "execute" | "command" | "shell_command" | "exec_command" => {
            ActionKind::Bash
        }
        "file_read" | "read_file" | "open_file" | "view_image" => ActionKind::Read,
        "file_write" | "write_file" | "create_file" => ActionKind::Write,
        "apply_patch" | "edit_file" | "patch" | "replace" => ActionKind::Edit,
        "web_search" => ActionKind::WebSearch,
        "search" | "grep" | "find_text" | "find_in_files" => ActionKind::Search,
        "web_fetch" | "fetch_url" => ActionKind::WebFetch,
        "glob" | "find" | "list_files" | "list_directory" | "file_search" => ActionKind::Glob,
        "update_plan" => ActionKind::Plan,
        n if n.starts_with("mcp") || n.contains("mcp_") => ActionKind::Mcp,
        _ => ActionKind::Other,
    };
    (target, kind)
}

fn session_id_from_path(path: &Path) -> String {
    // Use filename stem. Codex rollout files are `rollout-<timestamp>-<id>.jsonl`.
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn parse_ts_ms(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

/// Stable short hash (8 hex chars) for content-derived event IDs. NOT a
/// security primitive — just a deterministic discriminator so two identical
/// user prompts in one file dedup cleanly.
fn short_hash(s: &str) -> String {
    // FNV-1a 64-bit, top 32 bits to hex. No extra dep.
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in s.as_bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:08x}", (hash >> 32) as u32)
}

// ---------------------------------------------------------------------------
// Raw types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawLine {
    /// Record id; not currently consumed by the MVP emitter (event IDs are
    /// derived from `call_id` / `line_no` for stable cross-reparse
    /// determinism). Kept on the struct so a future phase can read it for
    /// observer-parity event IDs without a schema change.
    #[serde(default)]
    #[allow(dead_code)]
    id: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default, rename = "type")]
    r#type: String,
    #[serde(default)]
    payload: Option<Value>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = format!("{manifest_dir}/tests/fixtures/codex/{name}");
        std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {name} not found at {path}: {e}"))
    }

    fn scrubber() -> SecretScrubber {
        SecretScrubber::default()
    }

    // ---- Task 14: core parsing -----------------------------------------

    #[test]
    fn parses_rollout_session_into_events() {
        let bytes = fixture("rollout-session.jsonl");
        let path = Path::new("/tmp/rollout-001.jsonl");
        let res = parse(path, &bytes, 0, &scrubber()).unwrap();

        // Expected: 3 tool_use events (file_read, shell, web_search),
        // ≥2 token events, ≥1 user prompt.
        let tool_kinds: Vec<ActionKind> = res.tool_events.iter().map(|e| e.kind).collect();
        assert!(
            tool_kinds.contains(&ActionKind::Read),
            "expected a Read, got: {tool_kinds:?}"
        );
        assert!(
            tool_kinds.contains(&ActionKind::Bash),
            "expected a Bash, got: {tool_kinds:?}"
        );
        assert!(
            tool_kinds.contains(&ActionKind::WebSearch),
            "expected a WebSearch, got: {tool_kinds:?}"
        );

        // file_read result paired back.
        let read_ev = res
            .tool_events
            .iter()
            .find(|e| e.kind == ActionKind::Read)
            .expect("Read event");
        assert_eq!(
            read_ev.output.as_deref(),
            Some("package main\n\nfunc main() {}")
        );

        // shell result was failure (success:false).
        let bash_ev = res
            .tool_events
            .iter()
            .find(|e| e.kind == ActionKind::Bash)
            .expect("Bash event");
        assert_eq!(bash_ev.status, ActionStatus::Failure);

        // ≥2 token events (the fixture has two token_count lines).
        assert!(
            res.token_events.len() >= 2,
            "expected ≥2 token events, got {}",
            res.token_events.len()
        );
        // Session ID inherited from session_configured.
        assert_eq!(read_ev.session_id, "cx-001");
    }

    #[test]
    fn parses_response_item_dispatch_path() {
        let bytes = fixture("rollout-response-item.jsonl");
        let path = Path::new("/tmp/rollout-resp.jsonl");
        let res = parse(path, &bytes, 0, &scrubber()).unwrap();

        // The fixture exercises the response_item.function_call path.
        // We should see at least one shell/bash event from "shell_command"
        // function calls.
        let tool_kinds: Vec<ActionKind> = res.tool_events.iter().map(|e| e.kind).collect();
        assert!(
            tool_kinds.contains(&ActionKind::Bash),
            "expected a Bash event from function_call shell_command, got: {tool_kinds:?}"
        );
        // And at least one web_search_call → WebSearch.
        assert!(
            tool_kinds.contains(&ActionKind::WebSearch),
            "expected a WebSearch event, got: {tool_kinds:?}"
        );
    }

    // ---- Task 14: invariants -------------------------------------------

    #[test]
    fn parse_twice_from_zero_produces_identical_source_event_ids() {
        let bytes = fixture("rollout-session.jsonl");
        let path = Path::new("/tmp/rollout-idem.jsonl");
        let r1 = parse(path, &bytes, 0, &scrubber()).unwrap();
        let r2 = parse(path, &bytes, 0, &scrubber()).unwrap();
        let ids1: std::collections::HashSet<&str> = r1
            .tool_events
            .iter()
            .map(|e| e.source_event_id.as_str())
            .collect();
        let ids2: std::collections::HashSet<&str> = r2
            .tool_events
            .iter()
            .map(|e| e.source_event_id.as_str())
            .collect();
        assert_eq!(ids1, ids2, "idempotency: tool_event id sets must match");
        let token_ids1: std::collections::HashSet<&str> = r1
            .token_events
            .iter()
            .map(|e| e.source_event_id.as_str())
            .collect();
        let token_ids2: std::collections::HashSet<&str> = r2
            .token_events
            .iter()
            .map(|e| e.source_event_id.as_str())
            .collect();
        assert_eq!(
            token_ids1, token_ids2,
            "idempotency: token_event id sets must match"
        );
    }

    #[test]
    fn parse_from_nonzero_offset_skips_consumed_bytes() {
        let bytes = fixture("rollout-session.jsonl");
        let path = Path::new("/tmp/rollout-offset.jsonl");
        let r1 = parse(path, &bytes, 0, &scrubber()).unwrap();
        let r2 = parse(path, &bytes, r1.new_offset, &scrubber()).unwrap();
        assert!(r2.tool_events.is_empty());
        assert!(r2.token_events.is_empty());
        assert_eq!(r2.new_offset, r1.new_offset);
    }

    #[test]
    fn offset_advances_to_eof_on_complete_parse() {
        let bytes = fixture("rollout-session.jsonl");
        let path = Path::new("/tmp/rollout-eof.jsonl");
        let res = parse(path, &bytes, 0, &scrubber()).unwrap();
        let last_nl = bytes
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|i| i as u64 + 1)
            .unwrap_or(0);
        assert_eq!(
            res.new_offset, last_nl,
            "cursor must reach EOF after parsing all lines"
        );
    }

    #[test]
    fn malformed_line_is_skipped_not_fatal() {
        // session_configured → malformed line → top-level user_message.
        let bytes = b"{\"id\":\"sc\",\"type\":\"session_configured\",\"payload\":{\"session_id\":\"s1\"}}\nthis is not json }}\n{\"id\":\"um1\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"type\":\"user_message\",\"payload\":{\"message\":\"hi\"}}\n";
        let path = Path::new("/tmp/rollout-malformed.jsonl");
        let res = parse(path, bytes, 0, &scrubber()).unwrap();
        assert!(
            !res.warnings.is_empty(),
            "expected a ParseWarning for the malformed line"
        );
        // The user_message after the malformed line parsed.
        assert!(
            res.tool_events.iter().any(|e| e.kind == ActionKind::Other),
            "expected the user_message after the malformed line to parse"
        );
        // Cursor reached EOF.
        let last_nl = bytes.iter().rposition(|&b| b == b'\n').unwrap() as u64 + 1;
        assert_eq!(res.new_offset, last_nl);
    }

    #[test]
    fn secrets_are_scrubbed_from_input_and_output() {
        let bytes = fixture("with-secrets.jsonl");
        let path = Path::new("/tmp/rollout-scrub.jsonl");
        let res = parse(path, &bytes, 0, &scrubber()).unwrap();
        let forbidden = [
            "AKIAIOSFODNN7EXAMPLE",
            "ghp_0123456789012345678901234567890abcdefgh",
            "eyJhbGciOiJIUzI1NiJ9",
            "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ",
            "sk-proj-abcdef1234567890ABCDEFGHIJabcdefghij",
            "secretpass123",
        ];
        for ev in &res.tool_events {
            for s in [&ev.input, &ev.output, &ev.target].into_iter().flatten() {
                for f in forbidden {
                    assert!(!s.contains(f), "SCRUB FAILURE: {s:?} contains '{f}'");
                }
            }
        }
    }

    #[test]
    fn token_count_dedups_identical_envelopes() {
        // Codex periodically re-emits identical token_count records. The
        // adapter MUST dedup them (observer's seenModernTotal invariant).
        let bytes = b"{\"id\":\"sc\",\"type\":\"session_configured\",\"payload\":{\"session_id\":\"s1\"}}\n{\"id\":\"tk1\",\"type\":\"token_count\",\"payload\":{\"input_tokens\":1000,\"output_tokens\":200,\"cached_input_tokens\":800,\"model\":\"gpt-5-codex\"}}\n{\"id\":\"tk2\",\"type\":\"token_count\",\"payload\":{\"input_tokens\":1000,\"output_tokens\":200,\"cached_input_tokens\":800,\"model\":\"gpt-5-codex\"}}\n";
        let path = Path::new("/tmp/rollout-dedup.jsonl");
        let res = parse(path, bytes, 0, &scrubber()).unwrap();
        // Two identical token_count records, but only one event survives.
        assert_eq!(
            res.token_events.len(),
            1,
            "identical token_count envelopes MUST dedup to one event"
        );
    }

    #[test]
    fn token_count_cumulative_converts_to_per_turn_delta() {
        // First token_count with cumulative input=1000,cached=800 → net 200.
        // Second with cumulative input=1600,cached=1200 → net 400 → delta 200.
        let bytes = b"{\"id\":\"sc\",\"type\":\"session_configured\",\"payload\":{\"session_id\":\"s1\"}}\n{\"id\":\"tk1\",\"type\":\"token_count\",\"payload\":{\"input_tokens\":1000,\"output_tokens\":200,\"cached_input_tokens\":800,\"model\":\"m\"}}\n{\"id\":\"tk2\",\"type\":\"token_count\",\"payload\":{\"input_tokens\":1600,\"output_tokens\":400,\"cached_input_tokens\":1200,\"model\":\"m\"}}\n";
        let path = Path::new("/tmp/rollout-delta.jsonl");
        let res = parse(path, bytes, 0, &scrubber()).unwrap();
        assert_eq!(res.token_events.len(), 2);
        // First event: net input = 1000-800 = 200.
        assert_eq!(res.token_events[0].input_tokens, Some(200));
        // Second event: net cumulative 400, prior 200 → delta 200.
        assert_eq!(res.token_events[1].input_tokens, Some(200));
    }

    #[test]
    fn unknown_top_level_type_skipped_cleanly() {
        let bytes = b"{\"id\":\"x\",\"type\":\"future_event\",\"payload\":{\"foo\":\"bar\"}}\n";
        let path = Path::new("/tmp/rollout-unknown.jsonl");
        let res = parse(path, bytes, 0, &scrubber()).unwrap();
        assert!(res.tool_events.is_empty());
        assert!(
            res.warnings.is_empty(),
            "unknown shapes skip silently (no warning)"
        );
        // Cursor advanced.
        let last_nl = bytes.iter().rposition(|&b| b == b'\n').unwrap() as u64 + 1;
        assert_eq!(res.new_offset, last_nl);
    }
}
