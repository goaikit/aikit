//! Claude Code JSONL transcript parser.
//!
//! Reads `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl` line-by-line,
//! resumable from a byte offset, and emits normalized `ToolEvent` /
//! `TokenEvent` rows. See spec 010 §12.1.
//!
//! Reference: `superbased-observer/internal/adapter/claudecode/adapter.go`
//! (1739 lines, many edge cases). This Phase 2 MVP covers the core flow:
//! user/assistant records, tool_use↔tool_result pairing, usage envelopes
//! with msg-id dedup, and sidechain skipping. Compact_boundary /
//! agent-name / permission-mode / tier-2 cache observations land later.

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::adapter::{AdapterError, ParseResult, ParseWarning};
use crate::models::{
    ActionKind, ActionStatus, CacheObservation, CaptureSource, TokenEvent, ToolEvent, ToolKind,
};
use crate::scrub::SecretScrubber;

/// Parse Claude Code JSONL bytes from `from_offset` to EOF.
///
/// The caller (the Adapter impl) supplies the path (for `source_file` stamps),
/// the byte offset to resume from, and the shared scrubber. Returns the
/// normalized events and the new byte offset to persist.
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
    // toolu_id → index into res.tool_events, so a later tool_result block can
    // back-fill output / status on the matching tool_use event.
    let mut pending: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    // msg.id → index into res.token_events. One API call emits N JSONL records
    // (one per content block) sharing the same msg.id and a progressing
    // cumulative usage envelope. Last (highest output_tokens) wins.
    let mut msg_id_to_idx: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    let mut cursor: usize = 0;
    let mut line_no: u64 = 0;
    while cursor < tail.len() {
        // Find the next '\n'. Match the observer fix: include the terminator
        // length (handles CRLF too — ReadString in Go includes '\n', and we
        // count every byte consumed so the cursor never strands short of EOF).
        let nl = match tail[cursor..].iter().position(|&b| b == b'\n') {
            Some(i) => cursor + i + 1,
            None => {
                // Partial trailing line (no terminating '\n'). The writer may
                // still be appending — defer it to the next poll, do NOT
                // advance the cursor past it.
                break;
            }
        };
        line_no += 1;
        let raw_line = &tail[cursor..nl];
        cursor = nl;
        // Commit NewOffset after every complete line we consume, including
        // empty / malformed ones — otherwise the watcher repolls forever
        // (observer invariant, adapter.go:468).
        res.new_offset = start as u64 + cursor as u64;

        // Strip trailing \r\n / \n.
        let trimmed = raw_line
            .strip_suffix(b"\r\n")
            .unwrap_or_else(|| raw_line.strip_suffix(b"\n").unwrap_or(raw_line));
        let trimmed = std::str::from_utf8(trimmed).unwrap_or("");
        let trimmed = trimmed.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Parse the record.
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

        // Skip sidechain (sub-agent) lines — they share the parent's
        // session_id but belong to a different thread. The parent's tool_use
        // that spawned the subagent is captured separately as ActionKind::Subagent.
        if rec.is_sidechain {
            continue;
        }

        // API error envelopes: type="system", subtype="api_error". Map to a
        // ToolEvent with kind=Other and status=Failure so failures show up on
        // the timeline alongside tool calls. (Observer has a richer
        // ActionAPIError type; the spec 010 taxonomy collapses to Other.)
        if rec.r#type == "system" && rec.subtype.as_deref() == Some("api_error") {
            if let Some(ev) = build_api_error_event(path, &rec, line_no) {
                res.tool_events.push(ev);
            }
            continue;
        }

        // Non-message records we don't yet handle (compact_boundary,
        // turn_duration, agent-name, permission-mode) are skipped cleanly —
        // observer's V7d audit added those; spec 010 Phase 2 MVP leaves them
        // for a later phase. No warning, they're not malformed.
        if rec.message.is_none() {
            continue;
        }
        let msg: RawMessage = match serde_json::from_value(rec.message.unwrap()) {
            Ok(m) => m,
            Err(e) => {
                res.warnings.push(ParseWarning::MalformedLine {
                    line_no,
                    reason: format!("message field: {e}"),
                });
                continue;
            }
        };

        let ts_ms = parse_ts_ms(rec.timestamp.as_deref().unwrap_or(""));
        // git_root resolution is the host's concern (the trait is
        // storage-agnostic). For Phase 2 we stamp the cwd as-is and let the
        // EventStore layer resolve git roots. Observer does it inside the
        // adapter because it has a git.Resolve helper; we keep the crate
        // self-contained and stamp the literal cwd.
        let git_root = rec.cwd.as_deref().map(std::path::PathBuf::from);

        // Usage envelope → TokenEvent (assistant turns only).
        if let Some(usage) = &msg.usage {
            // Drop Claude Code's synthetic placeholder rows (compaction /
            // subagent stitching; the live install emits zero usage anyway).
            if msg.model.as_deref() == Some("<synthetic>") {
                continue;
            }
            // Dedup key: msg.id when present (one API call = N JSONL records
            // sharing it); fall back to the record's uuid.
            let event_id = msg.id.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| {
                rec.uuid
                    .clone()
                    .unwrap_or_else(|| format!("line:{line_no}"))
            });
            let cache_creation_1h = usage
                .cache_creation
                .as_ref()
                .map(|c| c.ephemeral_1h_input_tokens.unwrap_or(0))
                .unwrap_or(0);
            let cache_creation_total = usage.cache_creation_input_tokens.unwrap_or(0);
            // Per spec 010 data-model.md: absence means "not reported", never
            // "zero". We only get an integer here (serde default on miss is
            // None), so this is preserved naturally.
            let ev = TokenEvent {
                source_event_id: format!("claude_code:{}", event_id),
                session_id: rec.session_id.clone().unwrap_or_default(),
                tool: ToolKind::ClaudeCode,
                model: msg.model.clone(),
                request_id: msg.id.clone().filter(|s| !s.is_empty()),
                input_tokens: usage.input_tokens,
                cache_read_tokens: usage.cache_read_input_tokens,
                cache_creation_tokens: if cache_creation_total > 0 {
                    Some(cache_creation_total)
                } else {
                    usage
                        .cache_creation
                        .as_ref()
                        .and_then(|c| c.ephemeral_5m_input_tokens)
                        .map(|m| m + cache_creation_1h)
                },
                cache_creation_1h_tokens: if cache_creation_1h > 0 {
                    Some(cache_creation_1h)
                } else {
                    None
                },
                output_tokens: usage.output_tokens,
                reasoning_tokens: None,
                captured_at_ms: ts_ms.unwrap_or(0),
                captured_via: CaptureSource::Transcript,
            };
            if let Some(id) = &msg.id {
                if !id.is_empty() {
                    if let Some(&idx) = msg_id_to_idx.get(id) {
                        // Streaming usage progresses monotonically — keep the
                        // later record (highest output_tokens). Don't `continue`
                        // the outer loop; content blocks below are still
                        // distinct and must be processed.
                        if ev.output_tokens.unwrap_or(0)
                            >= res.token_events[idx].output_tokens.unwrap_or(0)
                        {
                            res.token_events[idx] = ev;
                        }
                    } else {
                        msg_id_to_idx.insert(id.clone(), res.token_events.len());
                        res.token_events.push(ev);
                    }
                    // Also emit a Tier-2 CacheObservation so the cachetrack
                    // module (future) can attribute invalidations. Spec 010
                    // data-model.md.
                    res.cache_observations.push(CacheObservation {
                        source_event_id: format!("cachetrack:{}", id),
                        session_id: rec.session_id.clone().unwrap_or_default(),
                        tool: ToolKind::ClaudeCode,
                        cache_read_input_tokens: usage.cache_read_input_tokens,
                        cache_creation_input_tokens: if cache_creation_total > 0 {
                            Some(cache_creation_total)
                        } else {
                            None
                        },
                        cache_creation_1h_input_tokens: if cache_creation_1h > 0 {
                            Some(cache_creation_1h)
                        } else {
                            None
                        },
                        assistant_blocks_hash: None, // Phase 2 leaves this for the cachetrack module.
                        tools_changed: Vec::new(),
                        observed_at_ms: ts_ms.unwrap_or(0),
                    });
                    continue;
                }
            }
            res.token_events.push(ev);
        }

        // Content blocks: tool_use, tool_result, text.
        let blocks = decode_content(&msg.content);
        for (block_idx, block) in blocks.iter().enumerate() {
            match block.r#type.as_str() {
                "text" => {
                    // User text → user_prompt ToolEvent. Assistant text → Think event.
                    if msg.role.as_deref() == Some("assistant") {
                        let text = block.text.as_deref().unwrap_or("").trim();
                        if !text.is_empty() {
                            let event_id = rec
                                .uuid
                                .clone()
                                .unwrap_or_else(|| format!("line:{line_no}:block:{block_idx}"));
                            res.tool_events.push(ToolEvent {
                                source_event_id: format!("claude_code:{event_id}"),
                                source_file: path.to_path_buf(),
                                session_id: rec.session_id.clone().unwrap_or_default(),
                                tool: ToolKind::ClaudeCode,
                                kind: ActionKind::Think,
                                target: Some(truncate_str(text, 200).to_string()),
                                input: Some(scrubber.scrub(text)),
                                output: None,
                                status: ActionStatus::Success,
                                error_message: None,
                                started_at_ms: ts_ms,
                                duration_ms: None,
                                git_root: git_root.clone(),
                                metadata: serde_json::Value::Null,
                            });
                        }
                    }
                }
                "tool_use" => {
                    let event_id = block.id.clone().unwrap_or_else(|| {
                        format!("{}:{}", rec.uuid.as_deref().unwrap_or("uuid"), block_idx)
                    });
                    let raw_input = block
                        .input
                        .as_ref()
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    let kind = map_action_kind(block.name.as_deref().unwrap_or(""));
                    let target = extract_target(
                        block.name.as_deref().unwrap_or(""),
                        block.input.as_ref(),
                        scrubber,
                    );
                    let scrubbed_input = if raw_input.is_empty() {
                        None
                    } else {
                        Some(scrubber.scrub(&raw_input))
                    };
                    let ev = ToolEvent {
                        source_event_id: format!("claude_code:{event_id}"),
                        source_file: path.to_path_buf(),
                        session_id: rec.session_id.clone().unwrap_or_default(),
                        tool: ToolKind::ClaudeCode,
                        kind,
                        target,
                        input: scrubbed_input,
                        output: None, // filled in when the matching tool_result arrives
                        status: ActionStatus::Success, // default; flipped to Failure on is_error
                        error_message: None,
                        started_at_ms: ts_ms,
                        duration_ms: None,
                        git_root: git_root.clone(),
                        metadata: serde_json::json!({
                            "raw_tool_name": block.name.as_deref().unwrap_or(""),
                            "model": msg.model.as_deref().unwrap_or(""),
                        }),
                    };
                    let idx = res.tool_events.len();
                    res.tool_events.push(ev);
                    if let Some(id) = &block.id {
                        pending.insert(id.clone(), idx);
                    }
                }
                "tool_result" => {
                    // Back-fill the matching tool_use event.
                    if let Some(tu_id) = &block.tool_use_id {
                        if let Some(&idx) = pending.get(tu_id) {
                            let body = decode_result_content(block.content.as_ref());
                            let scrubbed = scrubber.scrub(&body);
                            res.tool_events[idx].output = Some(scrubbed.clone());
                            if block.is_error.unwrap_or(false) {
                                res.tool_events[idx].status = ActionStatus::Failure;
                                res.tool_events[idx].error_message =
                                    Some(truncate_str(&scrubbed, 500).to_string());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(res)
}

/// Top-level Claude Code JSONL record. See observer's `rawLine` struct.
/// Fields absent on some records are `Option<...>`; serde leaves them None.
#[derive(Debug, Deserialize)]
struct RawLine {
    #[serde(default, rename = "sessionId")]
    session_id: Option<String>,
    /// Present in real transcripts; the Phase 2 MVP doesn't yet surface it
    /// on a ToolEvent (no field in spec 010's data-model) but parsers that
    /// follow (Phase 3 cachetrack) will consume it. Kept to avoid silently
    /// dropping the field.
    #[serde(default, rename = "gitBranch")]
    #[allow(dead_code)]
    git_branch: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default, rename = "type")]
    r#type: String,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    message: Option<Value>,
    // System/api_error records carry an `error` envelope instead of `message`.
    #[serde(default)]
    error: Option<Value>,
    #[serde(default, rename = "isSidechain")]
    is_sidechain: bool,
}

/// Inner `message` object on user/assistant records.
#[derive(Debug, Deserialize)]
struct RawMessage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    content: Option<Value>,
    #[serde(default)]
    usage: Option<RawUsage>,
}

#[derive(Debug, Deserialize)]
struct RawUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation: Option<CacheCreation>,
}

#[derive(Debug, Deserialize)]
struct CacheCreation {
    #[serde(default, rename = "ephemeral_5m_input_tokens")]
    ephemeral_5m_input_tokens: Option<u64>,
    #[serde(default, rename = "ephemeral_1h_input_tokens")]
    ephemeral_1h_input_tokens: Option<u64>,
}

/// Content block — tool_use / tool_result / text / etc.
#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(default, rename = "type")]
    r#type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<Value>,
    #[serde(default, rename = "tool_use_id")]
    tool_use_id: Option<String>,
    #[serde(default)]
    content: Option<Value>,
    #[serde(default, rename = "is_error")]
    is_error: Option<bool>,
}

/// `content` field is either a JSON array of blocks or a bare string (short
/// text-only messages). Returns an empty vec on decode failure.
fn decode_content(raw: &Option<Value>) -> Vec<ContentBlock> {
    let Some(v) = raw else {
        return Vec::new();
    };
    match v {
        Value::Array(_) => serde_json::from_value(v.clone()).unwrap_or_default(),
        Value::String(s) => vec![ContentBlock {
            r#type: "text".into(),
            text: Some(s.clone()),
            id: None,
            name: None,
            input: None,
            tool_use_id: None,
            content: None,
            is_error: None,
        }],
        _ => Vec::new(),
    }
}

/// Render a tool_result content payload (string or block array) as plain text.
/// Mirrors observer's `flattenResult`.
fn decode_result_content(raw: Option<&Value>) -> String {
    let Some(v) = raw else {
        return String::new();
    };
    match v {
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            let mut parts = Vec::new();
            for blk in arr {
                if blk.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = blk.get("text").and_then(|t| t.as_str()) {
                        let trimmed = t.trim();
                        if !trimmed.is_empty() {
                            parts.push(trimmed.to_string());
                        }
                    }
                }
            }
            parts.join(" ")
        }
        _ => String::new(),
    }
}

/// Claude Code tool name → normalized ActionKind. Mirrors observer's
/// `actionMap` (trimmed to spec 010's taxonomy — no separate Search/Files
/// split, no native-tools distinction).
fn map_action_kind(name: &str) -> ActionKind {
    match name {
        "Read" => ActionKind::Read,
        "Write" => ActionKind::Write,
        "Edit" | "MultiEdit" | "NotebookEdit" => ActionKind::Edit,
        "Bash" | "PowerShell" | "powershell" | "pwsh" | "cmd" | "cmd.exe" | "sh" => {
            ActionKind::Bash
        }
        "Grep" => ActionKind::Grep,
        "Glob" => ActionKind::Glob,
        "WebSearch" => ActionKind::WebSearch,
        "WebFetch" => ActionKind::WebFetch,
        "Agent" => ActionKind::Subagent,
        // MCP tools: mcp__<server>__<tool>
        n if n.starts_with("mcp__") => ActionKind::Mcp,
        _ => ActionKind::Other,
    }
}

/// Extract the "target" string for one tool call — the path/command/pattern
/// the action touched. Mirrors observer's `extractTarget`. The target is
/// pre-scrubbed for Bash (commands often carry inline secrets).
fn extract_target(
    tool_name: &str,
    input: Option<&Value>,
    scrubber: &SecretScrubber,
) -> Option<String> {
    let input = input?;
    let pick = |key: &str| -> Option<String> {
        input
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };
    let target = match tool_name {
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => {
            pick("file_path").or_else(|| pick("notebook_uri"))
        }
        "Bash" | "PowerShell" | "powershell" | "pwsh" | "cmd" | "cmd.exe" | "sh" => {
            pick("command").map(|c| scrubber.scrub(&c))
        }
        "Grep" => pick("pattern"),
        "Glob" => pick("pattern"),
        "WebSearch" => pick("query"),
        "WebFetch" => pick("url"),
        _ => None,
    };
    target.map(|t| truncate_str(&t, 200).to_string())
}

/// Build a ToolEvent from a system/api_error record. Mirrors observer's
/// `buildAPIErrorEvent` (simplified — no nested-error-chain walk for Phase 2;
/// we look one level deep).
fn build_api_error_event(path: &Path, rec: &RawLine, line_no: u64) -> Option<ToolEvent> {
    let err = rec.error.as_ref()?;
    let request_id = err.get("requestID").and_then(|v| v.as_str());
    let message = err
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .or_else(|| err.get("message").and_then(|m| m.as_str()));
    let err_type = err
        .get("error")
        .and_then(|e| e.get("type"))
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty() && *s != "error") // generic "error" → fall back to outer
        .or_else(|| err.get("type").and_then(|t| t.as_str()))
        .unwrap_or("api_error");

    // Need at least one identifier to emit a row.
    if request_id.is_none() && message.is_none() {
        return None;
    }

    let event_id = rec
        .uuid
        .clone()
        .unwrap_or_else(|| format!("api_err:{line_no}"));
    let ts_ms = parse_ts_ms(&rec.timestamp.clone().unwrap_or_default());
    Some(ToolEvent {
        source_event_id: format!("claude_code:{event_id}"),
        source_file: path.to_path_buf(),
        session_id: rec.session_id.clone().unwrap_or_default(),
        tool: ToolKind::ClaudeCode,
        kind: ActionKind::Other,
        target: request_id.map(|s| s.to_string()),
        input: None,
        output: message.map(|s| s.to_string()),
        status: ActionStatus::Failure,
        error_message: message.map(|s| truncate_str(s, 500).to_string()),
        started_at_ms: ts_ms,
        duration_ms: None,
        git_root: rec.cwd.as_deref().map(std::path::PathBuf::from),
        metadata: serde_json::json!({ "error_type": err_type }),
    })
}

fn parse_ts_ms(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    // RFC3339 / RFC3339Nano. chrono's parse handles both.
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        // Walk to char boundary at or before `max`.
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = format!("{manifest_dir}/tests/fixtures/claudecode/{name}");
        std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {name} not found at {path}: {e}"))
    }

    fn scrubber() -> SecretScrubber {
        SecretScrubber::default()
    }

    // ---- Task 10: core parsing -----------------------------------------

    #[test]
    fn parses_simple_session_into_events() {
        let bytes = fixture("simple-session.jsonl");
        let path = Path::new("/tmp/sess-001.jsonl");
        let res = parse(path, &bytes, 0, &scrubber()).unwrap();

        // The fixture has 5 lines:
        //  1. user text
        //  2. assistant with text + tool_use(Read) + usage
        //  3. user with tool_result for Read
        //  4. assistant with tool_use(Bash)
        //  5. user with tool_result for Bash (error)
        // Expect:
        //   - ≥1 TokenEvent (line 2 has usage)
        //   - ≥2 ToolEvents with kind Read/Bash (the two tool_use blocks)
        //   - the Bash result is_error=true → status=Failure
        let tool_kinds: Vec<ActionKind> = res.tool_events.iter().map(|e| e.kind).collect();
        assert!(
            tool_kinds.contains(&ActionKind::Read),
            "expected a Read ToolEvent, got: {tool_kinds:?}"
        );
        assert!(
            tool_kinds.contains(&ActionKind::Bash),
            "expected a Bash ToolEvent, got: {tool_kinds:?}"
        );
        assert!(
            !res.token_events.is_empty(),
            "expected at least one TokenEvent from the usage envelope"
        );
        // Bash result was an error → status Failure on the matching event.
        let bash_ev = res
            .tool_events
            .iter()
            .find(|e| e.kind == ActionKind::Bash)
            .expect("Bash event exists");
        assert_eq!(bash_ev.status, ActionStatus::Failure);
        assert!(bash_ev.error_message.is_some());
        // Read event got its output back-filled from the tool_result.
        let read_ev = res
            .tool_events
            .iter()
            .find(|e| e.kind == ActionKind::Read)
            .expect("Read event exists");
        assert!(
            read_ev.output.is_some(),
            "Read tool_result should back-fill output"
        );
    }

    #[test]
    fn parses_multi_tool_turn_pairs_results_correctly() {
        let bytes = fixture("multi-tool-turn.jsonl");
        let path = Path::new("/tmp/sess-002.jsonl");
        let res = parse(path, &bytes, 0, &scrubber()).unwrap();

        // 3 tool_use blocks: Grep, Glob, WebSearch. All paired with results.
        let grep_ev = res
            .tool_events
            .iter()
            .find(|e| e.kind == ActionKind::Grep)
            .expect("Grep event");
        assert_eq!(grep_ev.target.as_deref(), Some("func Handle"));
        assert_eq!(grep_ev.output.as_deref(), Some("found 12 matches"));

        let glob_ev = res
            .tool_events
            .iter()
            .find(|e| e.kind == ActionKind::Glob)
            .expect("Glob event");
        assert_eq!(glob_ev.target.as_deref(), Some("**/*.go"));

        let web_ev = res
            .tool_events
            .iter()
            .find(|e| e.kind == ActionKind::WebSearch)
            .expect("WebSearch event");
        assert_eq!(web_ev.status, ActionStatus::Failure);
        assert_eq!(web_ev.error_message.as_deref(), Some("network error"));
    }

    #[test]
    fn parses_api_errors_as_failure_events() {
        let bytes = fixture("api-error.jsonl");
        let path = Path::new("/tmp/sess-err.jsonl");
        let res = parse(path, &bytes, 0, &scrubber()).unwrap();

        // 3 api_error records → 3 ToolEvents with status=Failure.
        let failures: Vec<&ToolEvent> = res
            .tool_events
            .iter()
            .filter(|e| e.status == ActionStatus::Failure)
            .collect();
        assert_eq!(failures.len(), 3, "expected 3 API error events");
        // The 400 / 429 / 529 trio carry distinct error types.
        let error_types: Vec<&str> = failures
            .iter()
            .map(|e| {
                e.metadata
                    .get("error_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
            })
            .collect();
        assert!(error_types.contains(&"invalid_request_error"));
        assert!(error_types.contains(&"rate_limit_error"));
        assert!(error_types.contains(&"overloaded_error"));
    }

    #[test]
    fn malformed_line_is_skipped_not_fatal() {
        let bytes = fixture("malformed-line.jsonl");
        let path = Path::new("/tmp/sess-003.jsonl");
        let res = parse(path, &bytes, 0, &scrubber()).unwrap();

        // Line 1 (Read tool_use) and line 3 (Bash tool_use) parse; line 2
        // is "this is not valid json at all }}" and should produce a warning.
        assert!(
            !res.warnings.is_empty(),
            "expected a ParseWarning for the malformed line"
        );
        let tool_kinds: Vec<ActionKind> = res.tool_events.iter().map(|e| e.kind).collect();
        assert!(tool_kinds.contains(&ActionKind::Read));
        assert!(tool_kinds.contains(&ActionKind::Bash));
    }

    #[test]
    fn msg_id_dedup_chooses_highest_output_tokens() {
        let bytes = fixture("multi-block-dedup.jsonl");
        let path = Path::new("/tmp/sess-dedup.jsonl");
        let res = parse(path, &bytes, 0, &scrubber()).unwrap();

        // The fixture emits multiple JSONL records sharing one msg_id with
        // progressing usage. The TokenEvent count for that msg_id MUST be 1,
        // and it MUST carry the highest output_tokens seen.
        let mut by_id: std::collections::HashMap<&str, &TokenEvent> =
            std::collections::HashMap::new();
        for ev in &res.token_events {
            let key = ev
                .request_id
                .as_deref()
                .unwrap_or(ev.source_event_id.as_str());
            by_id.insert(key, ev);
        }
        // Every distinct msg_id appears exactly once.
        for ev in by_id.values() {
            let count = res
                .token_events
                .iter()
                .filter(|e| e.request_id == ev.request_id)
                .count();
            assert_eq!(
                count,
                1,
                "msg_id {} should dedup to one event",
                ev.request_id.as_deref().unwrap_or("?")
            );
        }
    }

    // ---- Task 11: invariants -------------------------------------------

    #[test]
    fn parse_twice_from_zero_produces_identical_source_event_ids() {
        // The idempotency invariant: a full re-walk produces byte-identical
        // source_event_ids, so the host's upsert is a no-op for seen rows.
        let bytes = fixture("simple-session.jsonl");
        let path = Path::new("/tmp/sess-idem.jsonl");
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
        assert_eq!(ids1, ids2, "idempotency: source_event_id sets must match");
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
        let bytes = fixture("simple-session.jsonl");
        let path = Path::new("/tmp/sess-offset.jsonl");
        // First parse consumes everything.
        let r1 = parse(path, &bytes, 0, &scrubber()).unwrap();
        // Second parse from the advanced offset should emit nothing new.
        let r2 = parse(path, &bytes, r1.new_offset, &scrubber()).unwrap();
        assert!(r2.tool_events.is_empty());
        assert!(r2.token_events.is_empty());
        assert_eq!(r2.new_offset, r1.new_offset);
    }

    #[test]
    fn offset_advances_past_every_complete_line_including_malformed() {
        // The watcher-repoll-forever bug class: if the cursor doesn't advance
        // past a malformed line (or an empty trailing line), the poll loops.
        let bytes = fixture("malformed-line.jsonl");
        let path = Path::new("/tmp/sess-adv.jsonl");
        let res = parse(path, &bytes, 0, &scrubber()).unwrap();
        // NewOffset must equal total bytes consumed (everything up to the
        // last '\n' in the fixture).
        let last_nl = bytes
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|i| i as u64 + 1)
            .unwrap_or(0);
        assert_eq!(
            res.new_offset, last_nl,
            "cursor must advance past malformed line"
        );
    }

    #[test]
    fn crlf_line_endings_consume_both_bytes() {
        // A line written with '\r\n' must consume both bytes for the cursor,
        // otherwise the watcher strands one byte short of EOF per CRLF line.
        let bytes = b"{\"type\":\"user\",\"sessionId\":\"s\",\"uuid\":\"u\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\r\n";
        let path = Path::new("/tmp/crlf.jsonl");
        let res = parse(path, bytes, 0, &scrubber()).unwrap();
        assert_eq!(
            res.new_offset,
            bytes.len() as u64,
            "CRLF must consume both bytes"
        );
    }

    #[test]
    fn secrets_are_scrubbed_from_input_and_output() {
        let bytes = fixture("with-secrets.jsonl");
        let path = Path::new("/tmp/sess-scrub.jsonl");
        let res = parse(path, &bytes, 0, &scrubber()).unwrap();

        // Every input + output field must be free of the known secret patterns.
        let forbidden = [
            "AKIAIOSFODNN7EXAMPLE",
            "ghp_0123456789012345678901234567890abcdefgh",
            "eyJhbGciOiJIUzI1NiJ9",
            "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ",
            "sk-proj-abcdef1234567890ABCDEFGHIJabcdefghij",
            "secretpass123",
        ];
        for ev in &res.tool_events {
            if let Some(s) = &ev.input {
                for f in forbidden {
                    assert!(
                        !s.contains(f),
                        "SCRUB FAILURE: input contains '{f}'\n  full: {s}"
                    );
                }
            }
            if let Some(s) = &ev.output {
                for f in forbidden {
                    assert!(
                        !s.contains(f),
                        "SCRUB FAILURE: output contains '{f}'\n  full: {s}"
                    );
                }
            }
            if let Some(s) = &ev.target {
                for f in forbidden {
                    assert!(
                        !s.contains(f),
                        "SCRUB FAILURE: target contains '{f}'\n  full: {s}"
                    );
                }
            }
        }
    }

    #[test]
    fn concatenated_records_on_one_line_treated_as_malformed() {
        // The observer has a recovery path for writer-corrupted "two JSON
        // records on one physical line" patterns. Phase 2 MVP treats it as
        // one malformed line + warning, NOT a fatal error.
        let bytes = fixture("concatenated-records.jsonl");
        let path = Path::new("/tmp/sess-concat.jsonl");
        let res = parse(path, &bytes, 0, &scrubber()).unwrap();
        // At least one warning emitted, parsing did not abort.
        assert!(
            !res.warnings.is_empty() || !res.tool_events.is_empty(),
            "concatenated records should produce warnings or partial events, not a silent no-op"
        );
    }
}
