//! Integration tests for the `aikit serve` history routes (spec 008 Task 5).
//!
//! Contract (see `specs/008-history-backend/contracts/history-api-contract.md`,
//! gitignored): unknown `{backend}` key → 404 `unknown_backend`; a known
//! Backend with no history store (e.g. `codex`) → 409 `history_unsupported`;
//! `claude` → 200 against a fixture; `404` vs `200 []` for messages; the
//! three-state PATCH semantics for `tag`.
//!
//! `CLAUDE_CONFIG_DIR`-touching assertions all live in ONE test function
//! (`test_claude_history_full_flow`) run sequentially within itself, so no
//! other test in this binary races the process-global env var. The other
//! tests here (`codex`/unknown-key) never read `CLAUDE_CONFIG_DIR` and are
//! safe to run in parallel with it.

use std::path::Path;
use std::time::Duration;

use aikit::cli::serve::{execute_with_run_fn, make_stub_run_fn_with_session, RunFn, ServeArgs};

fn make_args(port: u16) -> ServeArgs {
    ServeArgs {
        host: "127.0.0.1".to_string(),
        port,
        run_timeout_secs: 30,
        max_sessions: 10,
        api_key: None,
        insecure: false,
    }
}

async fn start_server() -> u16 {
    start_server_with(make_stub_run_fn_with_session(vec![], None)).await
}

async fn start_server_with(run_fn: RunFn) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let args = make_args(port);
    tokio::spawn(async move {
        execute_with_run_fn(args, run_fn).await.ok();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    port
}

/// Write a two-message (user + assistant) fixture session under
/// `<config_dir>/projects/-tmp-fixture-proj/<session_id>.jsonl` and return
/// the session id. Mirrors the fixture shape used by
/// `aikit-sdk/src/history/claude.rs`'s `#[ignore]`d live fixture test.
fn write_fixture(config_dir: &Path, session_id: &str) {
    let project_dir = config_dir.join("projects").join("-tmp-fixture-proj");
    std::fs::create_dir_all(&project_dir).unwrap();

    let jsonl_path = project_dir.join(format!("{session_id}.jsonl"));
    let lines = [
        serde_json::json!({
            "type": "user", "uuid": "u1", "sessionId": session_id,
            "message": {"content": "hello from the fixture"},
            "timestamp": "2026-01-15T10:00:00Z",
            "cwd": "/tmp/fixture-proj", "gitBranch": "main"
        }),
        serde_json::json!({
            "type": "assistant", "uuid": "a1", "parentUuid": "u1", "sessionId": session_id,
            "message": {"model": "claude-x", "content": [{"type": "text", "text": "hi there"}]},
            "timestamp": "2026-01-15T10:00:05Z"
        }),
    ];
    let body = lines
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&jsonl_path, body).unwrap();
}

// ── unknown backend key → 404 ────────────────────────────────────────────────

#[tokio::test]
async fn test_unknown_backend_returns_404() {
    let port = start_server().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    let resp = client
        .get(format!("{base}/api/v1/history/not-a-real-backend"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "unknown_backend");
    assert_eq!(body["backend"], "not-a-real-backend");
}

// ── codex (known Backend, no history store) → 409 ────────────────────────────

#[tokio::test]
async fn test_codex_backend_history_routes_return_409() {
    let port = start_server().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");
    let some_uuid = "550e8400-e29b-41d4-a716-446655440000";

    let resp = client
        .get(format!("{base}/api/v1/history/codex"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409, "list must be 409, not 404 or empty 200");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "history_unsupported");
    assert_eq!(body["backend"], "codex");

    let resp = client
        .get(format!("{base}/api/v1/history/codex/{some_uuid}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);

    let resp = client
        .get(format!("{base}/api/v1/history/codex/{some_uuid}/messages"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);

    let resp = client
        .patch(format!("{base}/api/v1/history/codex/{some_uuid}"))
        .json(&serde_json::json!({"rename": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "history_unsupported");
}

// ── claude: full flow against a fixture (single CLAUDE_CONFIG_DIR test) ──────

#[tokio::test]
async fn test_claude_history_full_flow() {
    let port = start_server().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    let fixture = tempfile::tempdir().unwrap();
    let session_id = "650e8400-e29b-41d4-a716-446655440010";
    write_fixture(fixture.path(), session_id);

    // SAFETY-equivalent to the aikit-sdk live fixture test: this is the only
    // test in this binary that touches `CLAUDE_CONFIG_DIR`, so no other test
    // running concurrently can race this mutation.
    let prev = std::env::var("CLAUDE_CONFIG_DIR").ok();
    std::env::set_var("CLAUDE_CONFIG_DIR", fixture.path());

    // ── 200 list for claude against the fixture ──
    let resp = client
        .get(format!("{base}/api/v1/history/claude"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let sessions: serde_json::Value = resp.json().await.unwrap();
    let sessions = sessions.as_array().expect("list must be a JSON array");
    assert!(
        sessions.iter().any(|s| s["session_id"] == session_id),
        "fixture session must appear in the list; got: {sessions:?}"
    );
    let found = sessions
        .iter()
        .find(|s| s["session_id"] == session_id)
        .unwrap();
    assert_eq!(found["backend"], "claude");
    // 2026-range regression guard: already-ms passthrough, not seconds.
    let lm = found["last_modified_ms"].as_i64().unwrap();
    assert!(lm > 1_700_000_000_000 && lm < 2_000_000_000_000);

    // ── 200 info for the fixture session ──
    let resp = client
        .get(format!("{base}/api/v1/history/claude/{session_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let info: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(info["session_id"], session_id);

    // ── 404 not_found for a well-formed but absent id ──
    let missing = "00000000-0000-4000-8000-000000000000";
    let resp = client
        .get(format!("{base}/api/v1/history/claude/{missing}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "not_found");
    assert_eq!(body["session_id"], missing);

    // ── 400 invalid_id for a malformed id ──
    let resp = client
        .get(format!("{base}/api/v1/history/claude/not-a-uuid"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "invalid_id");

    // ── 200 with 2 messages ──
    let resp = client
        .get(format!(
            "{base}/api/v1/history/claude/{session_id}/messages"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let messages: serde_json::Value = resp.json().await.unwrap();
    let messages = messages.as_array().expect("messages must be a JSON array");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["role"], "assistant");

    // ── 200 [] : the session exists but the page is beyond its messages ──
    // (distinguishes "empty page" from "session not found" — spec 008 §7).
    let resp = client
        .get(format!(
            "{base}/api/v1/history/claude/{session_id}/messages?offset=100"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let messages: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        messages.as_array().unwrap().len(),
        0,
        "an out-of-range page must be 200 [], not 404"
    );

    // ── 404 not_found : messages for an absent (but well-formed) session ──
    let resp = client
        .get(format!("{base}/api/v1/history/claude/{missing}/messages"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "not_found");

    // ── PATCH: empty body → 400 empty_patch ──
    let resp = client
        .patch(format!("{base}/api/v1/history/claude/{session_id}"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "empty_patch");

    // ── PATCH: rename → 204, then GET reflects custom_title ──
    let resp = client
        .patch(format!("{base}/api/v1/history/claude/{session_id}"))
        .json(&serde_json::json!({"rename": "My Renamed Session"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);
    let resp = client
        .get(format!("{base}/api/v1/history/claude/{session_id}"))
        .send()
        .await
        .unwrap();
    let info: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(info["custom_title"], "My Renamed Session");

    // ── PATCH: tag set (three-state: value present) → 204, tag reflected ──
    let resp = client
        .patch(format!("{base}/api/v1/history/claude/{session_id}"))
        .json(&serde_json::json!({"tag": "urgent"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);
    let resp = client
        .get(format!("{base}/api/v1/history/claude/{session_id}"))
        .send()
        .await
        .unwrap();
    let info: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(info["tag"], "urgent");

    // ── PATCH: rename-only body leaves the tag untouched (three-state: absent) ──
    let resp = client
        .patch(format!("{base}/api/v1/history/claude/{session_id}"))
        .json(&serde_json::json!({"rename": "Still Renamed"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);
    let resp = client
        .get(format!("{base}/api/v1/history/claude/{session_id}"))
        .send()
        .await
        .unwrap();
    let info: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        info["tag"], "urgent",
        "tag must be untouched when absent from the PATCH body"
    );
    assert_eq!(info["custom_title"], "Still Renamed");

    // ── PATCH: tag null (three-state: explicit clear) → 204, tag cleared ──
    let resp = client
        .patch(format!("{base}/api/v1/history/claude/{session_id}"))
        .json(&serde_json::json!({"tag": null}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);
    let resp = client
        .get(format!("{base}/api/v1/history/claude/{session_id}"))
        .send()
        .await
        .unwrap();
    let info: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(info["tag"], serde_json::Value::Null);

    // ── PATCH: well-formed but absent session id → 404 not_found ──
    let resp = client
        .patch(format!("{base}/api/v1/history/claude/{missing}"))
        .json(&serde_json::json!({"rename": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    match prev {
        Some(v) => std::env::set_var("CLAUDE_CONFIG_DIR", v),
        None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
    }
}
