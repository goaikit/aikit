//! OpenCode adapter integration tests. Spec 010 Phase 3 task 17.
//!
//! Uses the `opencode_fixture` helper to seed synthetic `opencode.db` files
//! with known rows. The fixture helper is a `tests/`-level module so it can
//! be reused across the integration test files.

#![cfg(feature = "opencode")]

mod opencode_fixture;

use aikit_session_capture::opencode::OpenCodeAdapter;
use aikit_session_capture::Adapter;

#[tokio::test]
async fn parses_fixture_into_events() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = opencode_fixture::open_fixture_file(tmp.path()).unwrap();
    let adapter = OpenCodeAdapter::default().with_override_roots(vec![tmp.path().to_path_buf()]);
    let res = adapter
        .parse_session_file(&db_path, 0)
        .await
        .expect("parse");

    // The fixture has: user message + assistant message + bash tool part.
    // Expect ≥1 ToolEvent with kind Bash.
    let tool_kinds: Vec<_> = res.tool_events.iter().map(|e| e.kind).collect();
    assert!(
        tool_kinds.contains(&aikit_session_capture::ActionKind::Bash),
        "expected a Bash ToolEvent, got: {tool_kinds:?}"
    );
    // Bash result back-filled.
    let bash_ev = res
        .tool_events
        .iter()
        .find(|e| e.kind == aikit_session_capture::ActionKind::Bash)
        .unwrap();
    assert_eq!(bash_ev.status, aikit_session_capture::ActionStatus::Success);
    assert_eq!(bash_ev.output.as_deref(), Some("ok all tests pass"));
    assert_eq!(bash_ev.duration_ms, Some(50)); // 1300 - 1250 = 50
    assert_eq!(bash_ev.session_id, "sess-1");

    // One token event from the assistant message.
    assert_eq!(res.token_events.len(), 1, "expected exactly 1 TokenEvent");
    let tok = &res.token_events[0];
    assert_eq!(tok.input_tokens, Some(100));
    assert_eq!(tok.output_tokens, Some(50));
    assert_eq!(tok.reasoning_tokens, Some(10));
    assert_eq!(tok.cache_read_tokens, Some(80));
    assert_eq!(tok.cache_creation_tokens, Some(20));
    assert_eq!(tok.model.as_deref(), Some("claude-sonnet-4-20250514"));

    // new_offset is the high-water time_updated (1300 from the tool part).
    assert_eq!(res.new_offset, 1300);
}

#[tokio::test]
async fn watermark_offset_skips_consumed_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = opencode_fixture::open_fixture_file(tmp.path()).unwrap();
    let adapter = OpenCodeAdapter::default().with_override_roots(vec![tmp.path().to_path_buf()]);
    // First parse from offset 0.
    let r1 = adapter.parse_session_file(&db_path, 0).await.unwrap();
    assert!(!r1.tool_events.is_empty());
    // Second parse from the advanced watermark → no new rows.
    let r2 = adapter
        .parse_session_file(&db_path, r1.new_offset)
        .await
        .unwrap();
    assert!(
        r2.tool_events.is_empty(),
        "no new events expected at watermark"
    );
    assert!(r2.token_events.is_empty());
}

#[tokio::test]
async fn parse_twice_produces_identical_source_event_ids() {
    // Idempotency invariant: a full re-walk produces identical IDs.
    let tmp = tempfile::tempdir().unwrap();
    let db_path = opencode_fixture::open_fixture_file(tmp.path()).unwrap();
    let adapter = OpenCodeAdapter::default().with_override_roots(vec![tmp.path().to_path_buf()]);
    let r1 = adapter.parse_session_file(&db_path, 0).await.unwrap();
    let r2 = adapter.parse_session_file(&db_path, 0).await.unwrap();
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
    assert_eq!(token_ids1, token_ids2);
}

#[tokio::test]
async fn secrets_are_scrubbed() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("opencode.db");
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        opencode_fixture::seed_secrets_fixture(&conn).unwrap();
    }
    let adapter = OpenCodeAdapter::default().with_override_roots(vec![tmp.path().to_path_buf()]);
    let res = adapter.parse_session_file(&db_path, 0).await.unwrap();
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
                assert!(!s.contains(f), "SCRUB FAILURE: '{f}' in {s:?}");
            }
        }
    }
}

#[tokio::test]
async fn is_session_file_matches_opencode_db() {
    let adapter =
        OpenCodeAdapter::default().with_override_roots(vec![std::path::PathBuf::from("/tmp/oc")]);
    assert!(adapter.is_session_file(std::path::Path::new("/tmp/oc/opencode.db")));
    assert!(adapter.is_session_file(std::path::Path::new("/tmp/oc/opencode.db-wal")));
    assert!(!adapter.is_session_file(std::path::Path::new("/tmp/oc/other.db")));
    assert!(!adapter.is_session_file(std::path::Path::new("/tmp/elsewhere/opencode.db")));
}

#[tokio::test]
async fn no_new_rows_returns_empty_result_with_advanced_watermark() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = opencode_fixture::open_fixture_file(tmp.path()).unwrap();
    let adapter = OpenCodeAdapter::default().with_override_roots(vec![tmp.path().to_path_buf()]);
    // Parse from an offset far ahead of any row in the fixture.
    let res = adapter.parse_session_file(&db_path, 10_000).await.unwrap();
    assert!(res.tool_events.is_empty());
    assert!(res.token_events.is_empty());
    // Watermark is the actual latest in the DB (1300), not the requested offset.
    assert_eq!(res.new_offset, 1300);
}
