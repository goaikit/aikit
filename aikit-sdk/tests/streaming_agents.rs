//! Integration tests for run_agent_events using in-process agent stubs.
//!
//! Each test writes a small shell script to a temp directory, prepends it to
//! PATH, and calls run_agent_events exactly as production code would. No
//! Docker or external infrastructure is required.

// Stub tests shell out to `#!/bin/sh` scripts on PATH (Unix only). Windows uses different
// PATH separators and binary resolution; fixture tests below still run on all targets.
#[cfg(unix)]
mod unix_stubs {
    use aikit_sdk::{
        run_agent, run_agent_events, AgentEventPayload, AgentEventStream, RunError, RunOptions,
    };
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    /// Serialize PATH-mutation tests to avoid races between parallel test threads.
    static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn write_stub(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "#!/bin/sh\n{}", body).unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
        path
    }

    fn with_stub_path<F, R>(dir: &std::path::Path, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let orig = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", dir.display(), orig));
        let result = f();
        std::env::set_var("PATH", orig);
        result
    }

    #[test]
    fn test_stub_codex_streaming() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "codex",
            r#"printf '{"type":"message","role":"system","content":"Codex session started"}\n'
printf '{"type":"message","role":"assistant","content":"Processing..."}\n'
printf '{"type":"message","role":"assistant","content":"Done."}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("codex", "test prompt", RunOptions::default(), |ev| {
                events.push(ev)
            })
        });

        assert!(result.is_ok(), "Expected Ok, got: {:?}", result.err());
        assert_eq!(events.len(), 3);
        for (i, ev) in events.iter().enumerate() {
            assert_eq!(ev.seq, i as u64);
            assert_eq!(ev.stream, AgentEventStream::Stdout);
            assert!(matches!(ev.payload, AgentEventPayload::JsonLine(_)));
        }
    }

    #[test]
    fn test_stub_claude_streaming() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "claude",
            r#"printf '{"type":"system","subtype":"init","session_id":"stub001"}\n'
printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Stub response."}]}}\n'
printf '{"type":"result","subtype":"success","result":"OK"}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events(
                "claude",
                "test prompt",
                RunOptions::new().with_stream(true),
                |ev| events.push(ev),
            )
        });

        assert!(result.is_ok());
        assert_eq!(events.len(), 3);
        assert!(events
            .iter()
            .all(|ev| matches!(ev.payload, AgentEventPayload::JsonLine(_))));
    }

    #[test]
    fn test_stub_gemini_streaming() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "gemini",
            r#"printf '{"candidates":[{"content":{"parts":[{"text":"Stub response."}],"role":"model"}}]}\n'
printf '{"candidates":[{"content":{"parts":[{"text":"Done."}],"role":"model"}}]}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("gemini", "test prompt", RunOptions::default(), |ev| {
                events.push(ev)
            })
        });

        assert!(result.is_ok());
        assert_eq!(events.len(), 2);
        assert!(events
            .iter()
            .all(|ev| matches!(ev.payload, AgentEventPayload::JsonLine(_))));
    }

    #[test]
    fn test_stub_opencode_streaming() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "opencode",
            r#"printf '{"type":"start","agent":"opencode"}\n'
printf '{"type":"message","role":"assistant","content":"Stub response."}\n'
printf '{"type":"end","exit_code":0}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("opencode", "test prompt", RunOptions::default(), |ev| {
                events.push(ev)
            })
        });

        assert!(result.is_ok());
        assert_eq!(events.len(), 3);
        assert!(events
            .iter()
            .all(|ev| matches!(ev.payload, AgentEventPayload::JsonLine(_))));
    }

    #[test]
    fn test_stub_agent_streaming() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "agent",
            r#"printf '{"event":"start","agent":"agent"}\n'
printf '{"event":"message","role":"assistant","text":"Stub response."}\n'
printf '{"event":"end","status":"success"}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("agent", "test prompt", RunOptions::default(), |ev| {
                events.push(ev)
            })
        });

        assert!(result.is_ok());
        assert_eq!(events.len(), 3);
        assert!(events
            .iter()
            .all(|ev| matches!(ev.payload, AgentEventPayload::JsonLine(_))));
    }

    #[test]
    fn test_stub_claude_quota_exceeded_stderr() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "claude",
            r#"printf '{"type":"system","subtype":"init"}\n'
printf 'Claude usage limit reached. Your limit will reset at 5 PM hour.\n' >&2
printf '{"type":"result","subtype":"success","result":"OK"}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("claude", "test", RunOptions::default(), |ev| {
                events.push(ev)
            })
        });

        assert!(result.is_ok());
        let quota_events: Vec<_> = events
            .iter()
            .filter(|ev| matches!(ev.payload, AgentEventPayload::QuotaExceeded { .. }))
            .collect();
        assert!(!quota_events.is_empty(), "Should detect quota exceeded");
        if let AgentEventPayload::QuotaExceeded { info, .. } = &quota_events[0].payload {
            assert_eq!(info.agent_key, "claude");
            assert_eq!(info.category, aikit_sdk::QuotaCategory::Hourly);
        } else {
            panic!("Expected QuotaExceeded payload");
        }
        let rr = result.unwrap();
        assert!(rr.quota_exceeded.is_some());
    }

    #[test]
    fn test_stub_claude_quota_failed_to_load_usage() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "claude",
            r#"printf 'Error: Failed to load usage data: {"error":{"type":"rate_limit_error","message":"Rate limited. Please try again later."}}\n' >&2
printf '{"type":"result","subtype":"success","result":"OK"}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("claude", "test", RunOptions::default(), |ev| {
                events.push(ev)
            })
        });

        assert!(result.is_ok());
        let quota_events: Vec<_> = events
            .iter()
            .filter(|ev| matches!(ev.payload, AgentEventPayload::QuotaExceeded { .. }))
            .collect();
        assert!(!quota_events.is_empty());
        if let AgentEventPayload::QuotaExceeded { info, .. } = &quota_events[0].payload {
            assert_eq!(info.agent_key, "claude");
        }
    }

    #[test]
    fn test_stub_claude_quota_api_error_plain() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "claude",
            r#"printf 'API Error: Rate limit reached\n' >&2
printf '{"type":"result","subtype":"success","result":"OK"}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("claude", "test", RunOptions::default(), |ev| {
                events.push(ev)
            })
        });

        assert!(result.is_ok());
        let quota_events: Vec<_> = events
            .iter()
            .filter(|ev| matches!(ev.payload, AgentEventPayload::QuotaExceeded { .. }))
            .collect();
        assert!(!quota_events.is_empty());
    }

    #[test]
    fn test_stub_claude_quota_http_429_line() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "claude",
            r#"printf 'HTTP 429: rate_limit_error: This request would exceed your rate limit.\n' >&2
printf '{"type":"result","subtype":"success","result":"OK"}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("claude", "test", RunOptions::default(), |ev| {
                events.push(ev)
            })
        });

        assert!(result.is_ok());
        let quota_events: Vec<_> = events
            .iter()
            .filter(|ev| matches!(ev.payload, AgentEventPayload::QuotaExceeded { .. }))
            .collect();
        assert!(!quota_events.is_empty());
    }

    #[test]
    fn test_stub_codex_quota_exceeded_json() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "codex",
            r#"printf '{"type":"error","code":"rate_limit_exceeded","message":"You have exceeded your request rate limit"}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("codex", "test", RunOptions::default(), |ev| events.push(ev))
        });

        assert!(result.is_ok());
        let quota_events: Vec<_> = events
            .iter()
            .filter(|ev| matches!(ev.payload, AgentEventPayload::QuotaExceeded { .. }))
            .collect();
        assert!(!quota_events.is_empty());
        if let AgentEventPayload::QuotaExceeded { info, .. } = &quota_events[0].payload {
            assert_eq!(info.agent_key, "codex");
        }
    }

    #[test]
    fn test_stub_codex_quota_exceeded_rawline() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "codex",
            r#"printf 'stream disconnected before completion: Rate limit reached for organization org-abc on tokens per min (TPM): Limit 250000, Used 250000\n' >&2"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("codex", "test", RunOptions::default(), |ev| events.push(ev))
        });

        assert!(result.is_ok());
        let quota_events: Vec<_> = events
            .iter()
            .filter(|ev| matches!(ev.payload, AgentEventPayload::QuotaExceeded { .. }))
            .collect();
        assert!(!quota_events.is_empty());
    }

    #[test]
    fn test_stub_gemini_quota_exceeded_json() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "gemini",
            r#"printf '{"error":{"code":429,"status":"RESOURCE_EXHAUSTED","message":"Quota exceeded"}}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("gemini", "test", RunOptions::default(), |ev| {
                events.push(ev)
            })
        });

        assert!(result.is_ok());
        let quota_events: Vec<_> = events
            .iter()
            .filter(|ev| matches!(ev.payload, AgentEventPayload::QuotaExceeded { .. }))
            .collect();
        assert!(!quota_events.is_empty());
        if let AgentEventPayload::QuotaExceeded { info, .. } = &quota_events[0].payload {
            assert_eq!(info.agent_key, "gemini");
            assert_eq!(info.category, aikit_sdk::QuotaCategory::Unknown);
        }
    }

    #[test]
    fn test_stub_gemini_quota_exceeded_rawline() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "gemini",
            r#"printf "prompt 1: ERROR {'code': 429, 'message': 'Rate limit exceeded. Try again later.'}\n""#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("gemini", "test", RunOptions::default(), |ev| {
                events.push(ev)
            })
        });

        assert!(result.is_ok());
        let quota_events: Vec<_> = events
            .iter()
            .filter(|ev| matches!(ev.payload, AgentEventPayload::QuotaExceeded { .. }))
            .collect();
        assert!(!quota_events.is_empty());
    }

    #[test]
    fn test_stub_opencode_quota_exceeded_json() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "opencode",
            r#"printf '{"type":"error","message":"weekly quota exceeded"}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("opencode", "test", RunOptions::default(), |ev| {
                events.push(ev)
            })
        });

        assert!(result.is_ok());
        let quota_events: Vec<_> = events
            .iter()
            .filter(|ev| matches!(ev.payload, AgentEventPayload::QuotaExceeded { .. }))
            .collect();
        assert!(!quota_events.is_empty());
        if let AgentEventPayload::QuotaExceeded { info, .. } = &quota_events[0].payload {
            assert_eq!(info.agent_key, "opencode");
            assert_eq!(info.category, aikit_sdk::QuotaCategory::Weekly);
        }
    }

    #[test]
    fn test_stub_opencode_insufficient_quota_json() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "opencode",
            r#"printf '{"type":"error","error":{"type":"insufficient_quota","code":"insufficient_quota","message":"You exceeded your current quota.","param":null}}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("opencode", "test", RunOptions::default(), |ev| {
                events.push(ev)
            })
        });

        assert!(result.is_ok());
        let quota_events: Vec<_> = events
            .iter()
            .filter(|ev| matches!(ev.payload, AgentEventPayload::QuotaExceeded { .. }))
            .collect();
        assert!(!quota_events.is_empty());
    }

    #[test]
    fn test_stub_agent_quota_exceeded_json() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "agent",
            r#"printf '{"type":"error","message":"Rate limit exceeded for hourly requests"}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("agent", "test", RunOptions::default(), |ev| events.push(ev))
        });

        assert!(result.is_ok());
        let quota_events: Vec<_> = events
            .iter()
            .filter(|ev| matches!(ev.payload, AgentEventPayload::QuotaExceeded { .. }))
            .collect();
        assert!(!quota_events.is_empty());
        if let AgentEventPayload::QuotaExceeded { info, .. } = &quota_events[0].payload {
            assert_eq!(info.agent_key, "agent");
            assert_eq!(info.category, aikit_sdk::QuotaCategory::Hourly);
        }
    }

    #[test]
    fn test_stub_agent_structured_log_quota() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "agent",
            r#"printf 'structured-log.info {"message":"agent_cli.turn.outcome","metadata":{"outcome":"error","grpc_code":"resource_exhausted","error_text":"Usage limit for slow pool"}}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("agent", "test", RunOptions::default(), |ev| events.push(ev))
        });

        assert!(result.is_ok());
        let quota_events: Vec<_> = events
            .iter()
            .filter(|ev| matches!(ev.payload, AgentEventPayload::QuotaExceeded { .. }))
            .collect();
        assert!(!quota_events.is_empty());
    }

    #[test]
    fn test_stub_no_quota_events_on_success() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "claude",
            r#"printf '{"type":"system","subtype":"init","session_id":"stub001"}\n'
printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Stub response."}]}}\n'
printf '{"type":"result","subtype":"success","result":"OK"}\n'"#,
        );

        let mut events = Vec::new();
        let result = with_stub_path(dir.path(), || {
            run_agent_events("claude", "test prompt", RunOptions::default(), |ev| {
                events.push(ev)
            })
        });

        assert!(result.is_ok());
        let quota_events: Vec<_> = events
            .iter()
            .filter(|ev| matches!(ev.payload, AgentEventPayload::QuotaExceeded { .. }))
            .collect();
        assert!(quota_events.is_empty(), "No quota events on success");
        assert!(result.unwrap().quota_exceeded.is_none());
    }

    #[test]
    fn test_run_agent_returns_quota_exceeded_error() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "claude",
            r#"printf 'Claude usage limit reached. Your limit will reset at 5 PM hour.\n' >&2
printf '{"type":"result","subtype":"success","result":"OK"}\n'"#,
        );

        let result = with_stub_path(dir.path(), || {
            run_agent("claude", "test", RunOptions::default())
        });

        match result {
            Err(RunError::QuotaExceeded(info)) => {
                assert_eq!(info.agent_key, "claude");
                assert_eq!(info.category, aikit_sdk::QuotaCategory::Hourly);
            }
            other => panic!("expected quota-exceeded error, got {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Fixture-based parser tests
// ---------------------------------------------------------------------------

#[test]
fn test_fixture_opencode_all_lines_valid_json() {
    let fixture = include_str!("fixtures/streaming/opencode.jsonl");
    for line in fixture.lines().filter(|l| !l.is_empty()) {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(parsed.is_ok(), "Line is not valid JSON: {}", line);
    }
}

#[test]
fn test_fixture_claude_all_lines_valid_json() {
    let fixture = include_str!("fixtures/streaming/claude.jsonl");
    for line in fixture.lines().filter(|l| !l.is_empty()) {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(parsed.is_ok(), "Line is not valid JSON: {}", line);
    }
}

#[test]
fn test_fixture_codex_all_lines_valid_json() {
    let fixture = include_str!("fixtures/streaming/codex.jsonl");
    for line in fixture.lines().filter(|l| !l.is_empty()) {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(parsed.is_ok(), "Line is not valid JSON: {}", line);
    }
}

#[test]
fn test_fixture_gemini_all_lines_valid_json() {
    let fixture = include_str!("fixtures/streaming/gemini.jsonl");
    for line in fixture.lines().filter(|l| !l.is_empty()) {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(parsed.is_ok(), "Line is not valid JSON: {}", line);
    }
}

#[test]
fn test_fixture_agent_all_lines_valid_json() {
    let fixture = include_str!("fixtures/streaming/agent.jsonl");
    for line in fixture.lines().filter(|l| !l.is_empty()) {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(parsed.is_ok(), "Line is not valid JSON: {}", line);
    }
}
