//! Tests for capacity and concurrency limits on the new POST /api/v1/messages flow.

use std::time::Duration;

use aikit::cli::serve::{
    execute_with_run_fn, make_blocking_stub_run_fn, make_stub_run_fn_with_session, RunFn, ServeArgs,
};

async fn start_server(run_fn: RunFn, max_sessions: usize) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let args = ServeArgs {
        host: "127.0.0.1".to_string(),
        port,
        run_timeout_secs: 30,
        max_sessions,
        api_key: None,
    };

    tokio::spawn(async move {
        execute_with_run_fn(args, run_fn).await.ok();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    port
}

#[tokio::test]
async fn test_max_sessions_returns_429() {
    let port = start_server(make_blocking_stub_run_fn(Duration::from_secs(5)), 1).await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    // Kick off a long-running request that occupies the one allowed slot.
    let base_a = base.clone();
    let client_a = client.clone();
    let first = tokio::spawn(async move {
        client_a
            .post(format!("{}/api/v1/messages", base_a))
            .json(&serde_json::json!({"agent": "aikit", "content": "blocker"}))
            .send()
            .await
            .unwrap()
    });

    // Give the server a moment to register the run.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client
        .post(format!("{}/api/v1/messages", base))
        .json(&serde_json::json!({"agent": "aikit", "content": "second"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 429);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "session_limit_reached");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains('1'),
        "error message must contain the max count"
    );

    first.abort();
}

#[tokio::test]
async fn test_concurrent_resume_returns_409() {
    let port = start_server(make_blocking_stub_run_fn(Duration::from_secs(5)), 10).await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);
    let session_id = "shared-session";

    // For aikit resume, we need the session to exist on disk. Point the
    // SessionStore at a temp dir and seed one entry.
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("AIKIT_SESSIONS_DIR", tmp.path());
    let path = tmp.path().join(format!("{}.json", session_id));
    std::fs::write(
        &path,
        serde_json::json!({
            "session_id": session_id,
            "agent": "aikit",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "cwd": "/tmp",
            "turns": [],
        })
        .to_string(),
    )
    .unwrap();

    // Start a long-running resume.
    let base_a = base.clone();
    let client_a = client.clone();
    let first = tokio::spawn(async move {
        client_a
            .post(format!("{}/api/v1/messages", base_a))
            .json(&serde_json::json!({
                "agent": "aikit",
                "session_id": session_id,
                "content": "first",
            }))
            .send()
            .await
            .unwrap()
    });

    tokio::time::sleep(Duration::from_millis(300)).await;

    // Second resume for the same session_id → 409.
    let resp = client
        .post(format!("{}/api/v1/messages", base))
        .json(&serde_json::json!({
            "agent": "aikit",
            "session_id": session_id,
            "content": "second",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "session_busy");

    first.abort();
    std::env::remove_var("AIKIT_SESSIONS_DIR");
}

#[tokio::test]
async fn test_invalid_request_returns_422() {
    let port = start_server(make_stub_run_fn_with_session(vec![], None), 10).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .json(&serde_json::json!({"agent": "", "content": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "invalid_request");
}
