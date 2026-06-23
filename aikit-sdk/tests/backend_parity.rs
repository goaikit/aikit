//! Per-Backend decode / token-usage / quota parity tests.
//!
//! Relocated from the former `runner/normalize.rs`, `runner/token_usage.rs`,
//! and `runner/quota.rs` unit-test modules when those files were consolidated
//! into `runner/backends/*` (spec 006). Exercised through the public API so they
//! act as golden-vector guards proving behaviour is preserved by the refactor.
//! The Cursor key is `cursor` (was `agent`, ADR 0006).

use aikit_sdk::runner::extract_quota_signal;
use aikit_sdk::{
    extract_usage_from_line, normalize_json_line, AgentEventPayload, AgentEventStream, MessageKind,
    MessagePhase, MessageRole, QuotaCategory, QuotaExceededInfo, RunError, RunResult, UsageSource,
};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;

// ---------------------------------------------------------------------------
// decode (was normalize.rs)
// ---------------------------------------------------------------------------

#[test]
fn test_decode_codex_item_agent_message() {
    let line = serde_json::json!({
        "type": "item.completed",
        "item": {"id": "item_0", "type": "agent_message", "text": "Done."}
    });
    let out = normalize_json_line("codex", AgentEventStream::Stdout, &line, 0);
    assert_eq!(out.len(), 1, "got {:?}", out);
    assert_eq!(out[0].text, "Done.");
    assert_eq!(out[0].role, MessageRole::Assistant);
    assert_eq!(out[0].kind, MessageKind::Message);
    assert_eq!(out[0].phase, MessagePhase::Final);
}

#[test]
fn test_decode_codex_item_command_execution() {
    let line = serde_json::json!({
        "type": "item.completed",
        "item": {
            "id": "item_1", "type": "command_execution",
            "command": "ls -la", "aggregated_output": "file.txt\n",
            "exit_code": 0, "status": "completed"
        }
    });
    let out = normalize_json_line("codex", AgentEventStream::Stdout, &line, 0);
    assert_eq!(out.len(), 2, "command + output; got {:?}", out);
    assert_eq!(out[0].text, "ls -la");
    assert_eq!(out[0].role, MessageRole::Tool);
    assert_eq!(out[0].kind, MessageKind::Message);
    assert_eq!(out[1].text, "file.txt\n");
    assert_eq!(out[1].kind, MessageKind::ToolOutput);
}

#[test]
fn test_decode_codex_item_file_change() {
    let line = serde_json::json!({
        "type": "item.completed",
        "item": {
            "id": "item_2", "type": "file_change", "status": "completed",
            "changes": [{"path": "/tmp/a.md", "kind": "add"}]
        }
    });
    let out = normalize_json_line("codex", AgentEventStream::Stdout, &line, 0);
    assert_eq!(out.len(), 1, "got {:?}", out);
    assert_eq!(out[0].text, "file_change: add /tmp/a.md");
    assert_eq!(out[0].role, MessageRole::Tool);
}

#[test]
fn test_decode_codex_error_and_turn_failed_are_surfaced() {
    let err = serde_json::json!({"type": "error", "message": "The '' model is not supported"});
    let out = normalize_json_line("codex", AgentEventStream::Stdout, &err, 0);
    assert_eq!(out.len(), 1, "error must surface; got {:?}", out);
    assert_eq!(out[0].role, MessageRole::System);
    assert_eq!(out[0].kind, MessageKind::Status);
    assert!(out[0].text.contains("not supported"));

    let failed = serde_json::json!({"type": "turn.failed", "error": {"message": "boom"}});
    let out2 = normalize_json_line("codex", AgentEventStream::Stdout, &failed, 0);
    assert_eq!(out2.len(), 1, "turn.failed must surface; got {:?}", out2);
    assert_eq!(out2[0].text, "boom");
}

#[test]
fn test_decode_codex_lifecycle_frames_ignored() {
    for t in [
        "thread.started",
        "turn.started",
        "turn.completed",
        "item.started",
    ] {
        let line = serde_json::json!({"type": t});
        assert!(
            normalize_json_line("codex", AgentEventStream::Stdout, &line, 0).is_empty(),
            "lifecycle frame {t} should be ignored"
        );
    }
}

#[test]
fn test_decode_codex_legacy_message_shape_still_works() {
    let line = serde_json::json!({"type":"message","role":"assistant","content":"hi"});
    let out = normalize_json_line("codex", AgentEventStream::Stdout, &line, 0);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].text, "hi");
}

#[test]
fn test_decode_gemini_delta_message_is_delta() {
    let line = serde_json::json!({
        "type": "message",
        "role": "assistant",
        "content": "I'm doing well, thank you!",
        "delta": true
    });
    let out = normalize_json_line("gemini", AgentEventStream::Stdout, &line, 0);
    assert_eq!(out.len(), 1, "should emit one StreamMessage; got {:?}", out);
    let m = &out[0];
    assert_eq!(m.text, "I'm doing well, thank you!");
    assert_eq!(m.phase, MessagePhase::Delta);
    assert_eq!(m.role, MessageRole::Assistant);
    assert_eq!(m.kind, MessageKind::Message);
}

#[test]
fn test_decode_gemini_final_message_is_final() {
    let line = serde_json::json!({
        "type": "message",
        "role": "assistant",
        "content": "Done.",
    });
    let out = normalize_json_line("gemini", AgentEventStream::Stdout, &line, 0);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].phase, MessagePhase::Final);

    let line2 = serde_json::json!({
        "type": "message",
        "role": "assistant",
        "content": "Done.",
        "delta": false
    });
    let out2 = normalize_json_line("gemini", AgentEventStream::Stdout, &line2, 0);
    assert_eq!(out2[0].phase, MessagePhase::Final);
}

#[test]
fn test_decode_gemini_user_echo_and_init_are_ignored() {
    let user = serde_json::json!({
        "type": "message",
        "role": "user",
        "content": "Hi, how are you?"
    });
    assert!(normalize_json_line("gemini", AgentEventStream::Stdout, &user, 0).is_empty());

    let init = serde_json::json!({"type":"init","session_id":"abc","model":"gemini-3"});
    assert!(normalize_json_line("gemini", AgentEventStream::Stdout, &init, 0).is_empty());

    let result_with_stats = serde_json::json!({"type":"result","stats":{"total_tokens":10}});
    assert!(
        normalize_json_line("gemini", AgentEventStream::Stdout, &result_with_stats, 0).is_empty()
    );
}

#[test]
fn test_decode_gemini_legacy_candidates_shape_still_works() {
    let line = serde_json::json!({
        "candidates": [{
            "content": { "parts": [{"text": "hello"}] }
        }]
    });
    let out = normalize_json_line("gemini", AgentEventStream::Stdout, &line, 0);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].text, "hello");
    assert_eq!(out[0].phase, MessagePhase::Delta);
}

// ---------------------------------------------------------------------------
// token-usage extraction (was token_usage.rs)
// ---------------------------------------------------------------------------

#[test]
fn test_extract_codex_usage_from_turn_completed() {
    let line: serde_json::Value = serde_json::from_str(
        r#"{"type":"turn.completed","usage":{"input_tokens":8058,"cached_input_tokens":6912,"output_tokens":15}}"#,
    )
    .unwrap();
    let (usage, source) = extract_usage_from_line(&line, "codex").unwrap();
    assert_eq!(source, UsageSource::Codex);
    assert_eq!(usage.input_tokens, 8058);
    assert_eq!(usage.output_tokens, 15);
    assert_eq!(usage.cache_read_tokens, Some(6912));
    assert!(usage.total_tokens.is_none());
}

#[test]
fn test_extract_codex_usage_ignores_other_types() {
    let line: serde_json::Value = serde_json::from_str(r#"{"type":"turn.started"}"#).unwrap();
    assert!(extract_usage_from_line(&line, "codex").is_none());
}

#[test]
fn test_extract_claude_usage_from_result() {
    let line: serde_json::Value = serde_json::from_str(
        r#"{"type":"result","subtype":"success","usage":{"input_tokens":3,"cache_creation_input_tokens":4807,"cache_read_input_tokens":11219,"output_tokens":5}}"#,
    )
    .unwrap();
    let (usage, source) = extract_usage_from_line(&line, "claude").unwrap();
    assert_eq!(source, UsageSource::Claude);
    assert_eq!(usage.input_tokens, 3);
    assert_eq!(usage.output_tokens, 5);
    assert_eq!(usage.cache_read_tokens, Some(11219));
    assert_eq!(usage.cache_creation_tokens, Some(4807));
}

#[test]
fn test_extract_claude_usage_from_stream_event_message_start() {
    let line: serde_json::Value = serde_json::from_str(
        r#"{"type":"stream_event","event":{"type":"message_start","message":{"usage":{"input_tokens":3,"cache_creation_input_tokens":4807,"cache_read_input_tokens":11219,"output_tokens":1}}}}"#,
    )
    .unwrap();
    let (usage, source) = extract_usage_from_line(&line, "claude").unwrap();
    assert_eq!(source, UsageSource::Claude);
    assert_eq!(usage.input_tokens, 3);
    assert_eq!(usage.cache_creation_tokens, Some(4807));
}

#[test]
fn test_extract_gemini_usage_from_result_stats() {
    let line: serde_json::Value = serde_json::from_str(
        r#"{"type":"result","status":"success","stats":{"total_tokens":7039,"input_tokens":7003,"output_tokens":2,"cached":6637,"input":366,"duration_ms":7615,"tool_calls":0}}"#,
    )
    .unwrap();
    let (usage, source) = extract_usage_from_line(&line, "gemini").unwrap();
    assert_eq!(source, UsageSource::Gemini);
    assert_eq!(usage.input_tokens, 7003);
    assert_eq!(usage.output_tokens, 2);
    assert_eq!(usage.total_tokens, Some(7039));
    assert_eq!(usage.cache_read_tokens, Some(6637));
}

#[test]
fn test_extract_gemini_usage_ignores_non_result() {
    let line: serde_json::Value =
        serde_json::from_str(r#"{"type":"message","role":"user","content":"ok"}"#).unwrap();
    assert!(extract_usage_from_line(&line, "gemini").is_none());
}

#[test]
fn test_extract_opencode_usage_from_step_finish() {
    let line: serde_json::Value = serde_json::from_str(
        r#"{"type":"step_finish","timestamp":1775657635524,"part":{"id":"prt_1","reason":"stop","type":"step-finish","tokens":{"total":11287,"input":11162,"output":42,"reasoning":39,"cache":{"write":0,"read":83}},"cost":0}}"#,
    )
    .unwrap();
    let (usage, source) = extract_usage_from_line(&line, "opencode").unwrap();
    assert_eq!(source, UsageSource::OpenCode);
    assert_eq!(usage.input_tokens, 11162);
    assert_eq!(usage.output_tokens, 42);
    assert_eq!(usage.total_tokens, Some(11287));
    assert_eq!(usage.reasoning_tokens, Some(39));
    assert_eq!(usage.cache_read_tokens, Some(83));
    assert_eq!(usage.cache_creation_tokens, Some(0));
}

#[test]
fn test_extract_cursor_usage_from_result_camelcase() {
    let line: serde_json::Value = serde_json::from_str(
        r#"{"type":"result","subtype":"success","usage":{"inputTokens":2,"outputTokens":4,"cacheReadTokens":12228,"cacheWriteTokens":2392}}"#,
    )
    .unwrap();
    let (usage, source) = extract_usage_from_line(&line, "cursor").unwrap();
    assert_eq!(source, UsageSource::Cursor);
    assert_eq!(usage.input_tokens, 2);
    assert_eq!(usage.output_tokens, 4);
    assert_eq!(usage.cache_read_tokens, Some(12228));
    assert_eq!(usage.cache_creation_tokens, Some(2392));
}

#[test]
fn test_extract_usage_unknown_agent_returns_none() {
    let line: serde_json::Value =
        serde_json::from_str(r#"{"type":"result","usage":{"input_tokens":1}}"#).unwrap();
    assert!(extract_usage_from_line(&line, "copilot").is_none());
    assert!(extract_usage_from_line(&line, "unknown").is_none());
    assert!(extract_usage_from_line(&line, "agent").is_none()); // renamed to "cursor"
}

// ---- recorded fixtures (golden vectors) ----

fn assert_fixture_has_usage(fixture: &str, key: &str, expected: UsageSource) {
    let mut found = false;
    for line in fixture.lines().filter(|l: &&str| !l.is_empty()) {
        let val: serde_json::Value = serde_json::from_str(line).unwrap();
        if let Some((usage, source)) = extract_usage_from_line(&val, key) {
            assert_eq!(source, expected);
            assert!(usage.input_tokens > 0 || usage.cache_read_tokens.is_some());
            found = true;
        }
    }
    assert!(found, "Should find token usage in {key} fixture");
}

#[test]
fn test_recorded_case01_codex_fixture() {
    assert_fixture_has_usage(
        include_str!("fixtures/recorded_case01/codex.jsonl"),
        "codex",
        UsageSource::Codex,
    );
}

#[test]
fn test_recorded_case01_claude_fixture() {
    assert_fixture_has_usage(
        include_str!("fixtures/recorded_case01/claude.jsonl"),
        "claude",
        UsageSource::Claude,
    );
}

#[test]
fn test_recorded_case01_gemini_fixture() {
    assert_fixture_has_usage(
        include_str!("fixtures/recorded_case01/gemini.jsonl"),
        "gemini",
        UsageSource::Gemini,
    );
}

#[test]
fn test_recorded_case01_opencode_fixture() {
    assert_fixture_has_usage(
        include_str!("fixtures/recorded_case01/opencode.jsonl"),
        "opencode",
        UsageSource::OpenCode,
    );
}

#[test]
fn test_recorded_case01_cursor_fixture() {
    assert_fixture_has_usage(
        include_str!("fixtures/recorded_case01/cursor-agent.jsonl"),
        "cursor",
        UsageSource::Cursor,
    );
}

// ---------------------------------------------------------------------------
// quota detection (was quota.rs)
// ---------------------------------------------------------------------------

#[test]
fn test_quota_category_serde_roundtrip() {
    for cat in [
        QuotaCategory::Hourly,
        QuotaCategory::Daily,
        QuotaCategory::Weekly,
        QuotaCategory::Requests,
        QuotaCategory::Tokens,
        QuotaCategory::Unknown,
    ] {
        let json = serde_json::to_string(&cat).unwrap();
        let back: QuotaCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(cat, back);
    }
}

#[test]
fn test_quota_exceeded_info_serde_roundtrip() {
    let info = QuotaExceededInfo {
        agent_key: "claude".to_string(),
        category: QuotaCategory::Hourly,
        raw_message: "usage limit".to_string(),
    };
    let json = serde_json::to_string(&info).unwrap();
    let back: QuotaExceededInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, back);
}

#[test]
fn test_run_result_new_has_quota_exceeded_none() {
    let status = std::process::ExitStatus::from_raw(0);
    let result = RunResult::new(status, vec![], vec![]);
    assert!(result.quota_exceeded.is_none());
}

#[test]
fn test_run_error_quota_exceeded_display() {
    let info = QuotaExceededInfo {
        agent_key: "claude".to_string(),
        category: QuotaCategory::Hourly,
        raw_message: "limit reached".to_string(),
    };
    let err = RunError::QuotaExceeded(info);
    let msg = format!("{}", err);
    assert!(msg.contains("claude"));
    assert!(msg.contains("quota exceeded"));
    assert!(msg.contains("limit reached"));
}

fn raw(s: &str) -> AgentEventPayload {
    AgentEventPayload::RawLine(s.to_string())
}
fn json(s: &str) -> AgentEventPayload {
    AgentEventPayload::JsonLine(serde_json::from_str(s).unwrap())
}

#[test]
fn test_quota_claude_rawline_usage_limit() {
    let info = extract_quota_signal(
        "claude",
        &raw("Claude usage limit reached. Your limit will reset at 5 PM."),
    )
    .unwrap();
    assert_eq!(info.agent_key, "claude");
    assert_eq!(info.category, QuotaCategory::Unknown);
}

#[test]
fn test_quota_claude_rawline_rate_limit_hourly() {
    let info = extract_quota_signal("claude", &raw("Rate limit hit for hourly usage")).unwrap();
    assert_eq!(info.agent_key, "claude");
    assert_eq!(info.category, QuotaCategory::Hourly);
}

#[test]
fn test_quota_claude_failed_to_load_usage_data() {
    let info = extract_quota_signal(
        "claude",
        &raw(r#"Error: Failed to load usage data: {"error":{"type":"rate_limit_error","message":"Rate limited. Please try again later."}}"#),
    )
    .unwrap();
    assert_eq!(info.agent_key, "claude");
    assert!(info.raw_message.contains("Rate limited"));
}

#[test]
fn test_quota_claude_api_error_rate_limit_reached() {
    let info = extract_quota_signal("claude", &raw("API Error: Rate limit reached")).unwrap();
    assert_eq!(info.agent_key, "claude");
}

#[test]
fn test_quota_claude_api_error_429() {
    let info = extract_quota_signal(
        "claude",
        &raw("API Error: Request rejected (429) · Rate limited"),
    )
    .unwrap();
    assert_eq!(info.agent_key, "claude");
}

#[test]
fn test_quota_claude_hit_your_limit() {
    let info = extract_quota_signal(
        "claude",
        &raw("⎿ You've hit your limit · resets 10am (Asia/Manila)"),
    )
    .unwrap();
    assert_eq!(info.agent_key, "claude");
}

#[test]
fn test_quota_claude_http_429_rate_limit_error() {
    let info = extract_quota_signal(
        "claude",
        &raw("HTTP 429: rate_limit_error: This request would exceed your account's rate limit."),
    )
    .unwrap();
    assert_eq!(info.agent_key, "claude");
}

#[test]
fn test_quota_claude_error_429_json() {
    let info = extract_quota_signal(
        "claude",
        &raw(r#"Error: 429 {"type":"error","error":{"type":"rate_limit_error","message":"Extra usage is required for long context requests."},"request_id":"req_abc123"}"#),
    )
    .unwrap();
    assert_eq!(info.agent_key, "claude");
    assert_eq!(info.category, QuotaCategory::Tokens);
}

#[test]
fn test_quota_claude_json_type_error_rate_limit() {
    let info = extract_quota_signal(
        "claude",
        &json(r#"{"type":"error","error":{"type":"rate_limit_error","message":"Rate limited. Please try again later."}}"#),
    )
    .unwrap();
    assert_eq!(info.agent_key, "claude");
}

#[test]
fn test_quota_claude_json_result_error_usage() {
    let info = extract_quota_signal(
        "claude",
        &json(r#"{"type":"result","subtype":"error","message":"usage limit reached"}"#),
    )
    .unwrap();
    assert_eq!(info.agent_key, "claude");
}

#[test]
fn test_quota_codex_rate_limit_code() {
    let info = extract_quota_signal(
        "codex",
        &json(r#"{"type":"error","code":"rate_limit_exceeded","message":"You have exceeded your request rate limit"}"#),
    )
    .unwrap();
    assert_eq!(info.agent_key, "codex");
}

#[test]
fn test_quota_codex_rawline_tpm() {
    let info = extract_quota_signal(
        "codex",
        &raw("stream disconnected before completion: Rate limit reached for organization org-abc on tokens per min (TPM): Limit 250000, Used 250000"),
    )
    .unwrap();
    assert_eq!(info.agent_key, "codex");
}

#[test]
fn test_quota_codex_rawline_429() {
    let info = extract_quota_signal(
        "codex",
        &raw("error: http 429 Too Many Requests: rate_limit_exceeded"),
    )
    .unwrap();
    assert_eq!(info.agent_key, "codex");
}

#[test]
fn test_quota_gemini_resource_exhausted() {
    let info = extract_quota_signal(
        "gemini",
        &json(r#"{"error":{"code":429,"status":"RESOURCE_EXHAUSTED","message":"Quota exceeded"}}"#),
    )
    .unwrap();
    assert_eq!(info.agent_key, "gemini");
    assert_eq!(info.category, QuotaCategory::Unknown);
}

#[test]
fn test_quota_gemini_rawline_error_429() {
    let info = extract_quota_signal(
        "gemini",
        &raw("prompt 1: ERROR {'code': 429, 'message': 'Rate limit exceeded. Try again later.'}"),
    )
    .unwrap();
    assert_eq!(info.agent_key, "gemini");
}

#[test]
fn test_quota_opencode_quota_message() {
    let info = extract_quota_signal(
        "opencode",
        &json(r#"{"type":"error","message":"weekly quota exceeded"}"#),
    )
    .unwrap();
    assert_eq!(info.agent_key, "opencode");
    assert_eq!(info.category, QuotaCategory::Weekly);
}

#[test]
fn test_quota_opencode_insufficient_quota_json() {
    let info = extract_quota_signal(
        "opencode",
        &json(r#"{"type":"error","sequence_number":2,"error":{"type":"insufficient_quota","code":"insufficient_quota","message":"You exceeded your current quota.","param":null}}"#),
    )
    .unwrap();
    assert_eq!(info.agent_key, "opencode");
}

#[test]
fn test_quota_opencode_rawline_daily_token() {
    let info = extract_quota_signal("opencode", &raw("Your daily token quota exceeded")).unwrap();
    assert_eq!(info.agent_key, "opencode");
    assert_eq!(info.category, QuotaCategory::Daily);
}

#[test]
fn test_quota_opencode_rawline_rate_limited() {
    let info = extract_quota_signal("opencode", &raw("You are rate-limited")).unwrap();
    assert_eq!(info.agent_key, "opencode");
}

#[test]
fn test_quota_cursor_rate_limit() {
    let info = extract_quota_signal(
        "cursor",
        &json(r#"{"type":"error","message":"Rate limit exceeded for hourly requests"}"#),
    )
    .unwrap();
    assert_eq!(info.agent_key, "cursor");
    assert_eq!(info.category, QuotaCategory::Hourly);
}

#[test]
fn test_quota_cursor_structured_log_resource_exhausted() {
    let info = extract_quota_signal(
        "cursor",
        &raw(r#"structured-log.info {"message":"agent_cli.turn.outcome","metadata":{"outcome":"error","grpc_code":"resource_exhausted","error_text":"Usage limit for slow pool"}}"#),
    )
    .unwrap();
    assert_eq!(info.agent_key, "cursor");
}

#[test]
fn test_quota_cursor_rawline_usage_limit() {
    let info = extract_quota_signal(
        "cursor",
        &raw("b: You've hit your usage limit for Opus. Switch to Auto."),
    )
    .unwrap();
    assert_eq!(info.agent_key, "cursor");
}

#[test]
fn test_quota_no_match_returns_none() {
    let payload = raw("Normal output line");
    assert!(extract_quota_signal("claude", &payload).is_none());
    assert!(extract_quota_signal("codex", &payload).is_none());
    assert!(extract_quota_signal("gemini", &payload).is_none());
    assert!(extract_quota_signal("opencode", &payload).is_none());
    assert!(extract_quota_signal("cursor", &payload).is_none());
}

#[test]
fn test_quota_unknown_agent_returns_none() {
    assert!(extract_quota_signal("copilot", &raw("Rate limit reached")).is_none());
    assert!(extract_quota_signal("agent", &raw("Rate limit reached")).is_none());
    // renamed
}
