//! Integration tests for run_agent_events using in-process agent stubs.
//!
//! Each test writes a small shell script to a temp directory, prepends it to
//! PATH, and calls run_agent_events exactly as production code would. No
//! Docker or external infrastructure is required.

use aikit_sdk::{run_agent_events, AgentEventPayload, AgentEventStream, RunOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;

/// Serialize PATH-mutation tests to avoid races between parallel test threads.
static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Write an executable shell script to `dir/<name>` and return its path.
fn write_stub(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "#!/bin/sh\n{}", body).unwrap();
    let mut perms = f.metadata().unwrap().permissions();
    perms.set_mode(0o755);
    f.set_permissions(perms).unwrap();
    path
}

/// Run `run_agent_events` with `dir` prepended to PATH, restore PATH after.
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

// ---------------------------------------------------------------------------
// Per-agent streaming tests
// ---------------------------------------------------------------------------

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
