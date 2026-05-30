use super::types::{TokenUsage, UsageSource};

// ---------------------------------------------------------------------------
// Token usage extraction
// ---------------------------------------------------------------------------

pub(super) fn sum_optional<'a>(vals: impl Iterator<Item = &'a Option<u64>>) -> Option<u64> {
    let collected: Vec<_> = vals.collect();
    if collected.iter().any(|v| v.is_some()) {
        Some(collected.iter().map(|v| v.unwrap_or(0)).sum())
    } else {
        None
    }
}

pub(super) fn extract_codex_usage(line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
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

pub(super) fn extract_claude_usage(line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
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

pub(super) fn extract_gemini_usage(line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
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

pub(super) fn extract_opencode_usage(
    line: &serde_json::Value,
) -> Option<(TokenUsage, UsageSource)> {
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

pub(super) fn extract_cursor_usage(line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
    if line.get("type")?.as_str()? != "result" {
        return None;
    }
    let usage = line.get("usage")?;
    let input_tokens = usage.get("inputTokens")?.as_u64()?;
    let output_tokens = usage.get("outputTokens")?.as_u64()?;
    let cache_read_tokens = usage.get("cacheReadTokens").and_then(|v| v.as_u64());
    let cache_creation_tokens = usage.get("cacheWriteTokens").and_then(|v| v.as_u64());
    Some((
        TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens: None,
            cache_read_tokens,
            cache_creation_tokens,
            reasoning_tokens: None,
        },
        UsageSource::Cursor,
    ))
}

/// Extract and normalize token usage from a single agent output line.
///
/// Returns `None` for lines that do not carry usage data or for unknown agent keys.
pub fn extract_usage_from_line(
    line: &serde_json::Value,
    agent_key: &str,
) -> Option<(TokenUsage, UsageSource)> {
    match agent_key {
        "codex" => extract_codex_usage(line),
        "claude" => extract_claude_usage(line),
        "gemini" => extract_gemini_usage(line),
        "opencode" => extract_opencode_usage(line),
        "agent" => extract_cursor_usage(line),
        _ => None,
    }
}

/// Aggregate a sequence of token usage entries using the per-agent rule.
///
/// - **Codex**: sum all entries (multiple `turn.completed` messages)
/// - **All others**: take the last entry (final `result` / `step_finish`)
///
/// Returns `None` when `usage_entries` is empty.
pub fn aggregate_token_usage(
    usage_entries: &[(TokenUsage, UsageSource)],
    source: UsageSource,
) -> Option<TokenUsage> {
    if usage_entries.is_empty() {
        return None;
    }
    match source {
        UsageSource::Codex => {
            let input_tokens = usage_entries.iter().map(|(u, _)| u.input_tokens).sum();
            let output_tokens = usage_entries.iter().map(|(u, _)| u.output_tokens).sum();
            let total_tokens = sum_optional(usage_entries.iter().map(|(u, _)| &u.total_tokens));
            let cache_read_tokens =
                sum_optional(usage_entries.iter().map(|(u, _)| &u.cache_read_tokens));
            let cache_creation_tokens =
                sum_optional(usage_entries.iter().map(|(u, _)| &u.cache_creation_tokens));
            let reasoning_tokens =
                sum_optional(usage_entries.iter().map(|(u, _)| &u.reasoning_tokens));
            Some(TokenUsage {
                input_tokens,
                output_tokens,
                total_tokens,
                cache_read_tokens,
                cache_creation_tokens,
                reasoning_tokens,
            })
        }
        _ => usage_entries.last().map(|(u, _)| u.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::types::{RunOptions, UsageSource};

    #[test]
    fn test_extract_codex_usage_from_turn_completed() {
        let line: serde_json::Value = serde_json::from_str(
            r#"{"type":"turn.completed","usage":{"input_tokens":8058,"cached_input_tokens":6912,"output_tokens":15}}"#,
        )
        .unwrap();
        let result = extract_usage_from_line(&line, "codex");
        assert!(result.is_some());
        let (usage, source) = result.unwrap();
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
        let result = extract_usage_from_line(&line, "claude");
        assert!(result.is_some());
        let (usage, source) = result.unwrap();
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
        let result = extract_usage_from_line(&line, "claude");
        assert!(result.is_some());
        let (usage, source) = result.unwrap();
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
        let result = extract_usage_from_line(&line, "gemini");
        assert!(result.is_some());
        let (usage, source) = result.unwrap();
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
        let result = extract_usage_from_line(&line, "opencode");
        assert!(result.is_some());
        let (usage, source) = result.unwrap();
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
        let result = extract_usage_from_line(&line, "agent");
        assert!(result.is_some());
        let (usage, source) = result.unwrap();
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
    }

    #[test]
    fn test_aggregate_codex_sums_all_entries() {
        let entries = vec![
            (
                TokenUsage {
                    input_tokens: 100,
                    output_tokens: 10,
                    total_tokens: None,
                    cache_read_tokens: Some(50),
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                UsageSource::Codex,
            ),
            (
                TokenUsage {
                    input_tokens: 200,
                    output_tokens: 20,
                    total_tokens: None,
                    cache_read_tokens: Some(75),
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                UsageSource::Codex,
            ),
        ];
        let result = aggregate_token_usage(&entries, UsageSource::Codex).unwrap();
        assert_eq!(result.input_tokens, 300);
        assert_eq!(result.output_tokens, 30);
        assert_eq!(result.cache_read_tokens, Some(125));
    }

    #[test]
    fn test_aggregate_claude_takes_last() {
        let entries = vec![
            (
                TokenUsage {
                    input_tokens: 10,
                    output_tokens: 1,
                    total_tokens: None,
                    cache_read_tokens: None,
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                UsageSource::Claude,
            ),
            (
                TokenUsage {
                    input_tokens: 99,
                    output_tokens: 7,
                    total_tokens: None,
                    cache_read_tokens: Some(500),
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                UsageSource::Claude,
            ),
        ];
        let result = aggregate_token_usage(&entries, UsageSource::Claude).unwrap();
        assert_eq!(result.input_tokens, 99);
        assert_eq!(result.output_tokens, 7);
        assert_eq!(result.cache_read_tokens, Some(500));
    }

    #[test]
    fn test_aggregate_empty_returns_none() {
        assert!(aggregate_token_usage(&[], UsageSource::Codex).is_none());
        assert!(aggregate_token_usage(&[], UsageSource::Claude).is_none());
    }

    #[test]
    fn test_run_options_default_emit_token_usage_events_true() {
        let opts = RunOptions::default();
        assert!(opts.emit_token_usage_events);
    }

    #[test]
    fn test_run_options_with_emit_token_usage_events() {
        let opts = RunOptions::new().with_emit_token_usage_events(false);
        assert!(!opts.emit_token_usage_events);
    }

    #[test]
    fn test_recorded_case01_codex_fixture() {
        let fixture = include_str!("../../tests/fixtures/recorded_case01/codex.jsonl");
        let mut found = false;
        for line in fixture.lines().filter(|l: &&str| !l.is_empty()) {
            let val: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some((usage, source)) = extract_usage_from_line(&val, "codex") {
                assert_eq!(source, UsageSource::Codex);
                assert!(usage.input_tokens > 0);
                found = true;
            }
        }
        assert!(
            found,
            "Should find at least one token usage line in codex fixture"
        );
    }

    #[test]
    fn test_recorded_case01_claude_fixture() {
        let fixture = include_str!("../../tests/fixtures/recorded_case01/claude.jsonl");
        let mut found = false;
        for line in fixture.lines().filter(|l: &&str| !l.is_empty()) {
            let val: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some((usage, source)) = extract_usage_from_line(&val, "claude") {
                assert_eq!(source, UsageSource::Claude);
                assert!(usage.input_tokens > 0 || usage.cache_read_tokens.is_some());
                found = true;
            }
        }
        assert!(
            found,
            "Should find at least one token usage line in claude fixture"
        );
    }

    #[test]
    fn test_recorded_case01_gemini_fixture() {
        let fixture = include_str!("../../tests/fixtures/recorded_case01/gemini.jsonl");
        let mut found = false;
        for line in fixture.lines().filter(|l: &&str| !l.is_empty()) {
            let val: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some((usage, source)) = extract_usage_from_line(&val, "gemini") {
                assert_eq!(source, UsageSource::Gemini);
                assert!(usage.input_tokens > 0);
                found = true;
            }
        }
        assert!(
            found,
            "Should find at least one token usage line in gemini fixture"
        );
    }

    #[test]
    fn test_recorded_case01_opencode_fixture() {
        let fixture = include_str!("../../tests/fixtures/recorded_case01/opencode.jsonl");
        let mut found = false;
        for line in fixture.lines().filter(|l: &&str| !l.is_empty()) {
            let val: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some((usage, source)) = extract_usage_from_line(&val, "opencode") {
                assert_eq!(source, UsageSource::OpenCode);
                assert!(usage.input_tokens > 0);
                found = true;
            }
        }
        assert!(
            found,
            "Should find at least one token usage line in opencode fixture"
        );
    }

    #[test]
    fn test_recorded_case01_cursor_fixture() {
        let fixture = include_str!("../../tests/fixtures/recorded_case01/cursor-agent.jsonl");
        let mut found = false;
        for line in fixture.lines().filter(|l: &&str| !l.is_empty()) {
            let val: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some((usage, source)) = extract_usage_from_line(&val, "agent") {
                assert_eq!(source, UsageSource::Cursor);
                assert!(usage.input_tokens > 0);
                found = true;
            }
        }
        assert!(
            found,
            "Should find at least one token usage line in cursor fixture"
        );
    }
}
