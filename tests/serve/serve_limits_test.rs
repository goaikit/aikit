//! Tests for capacity and concurrency limits on the new POST /api/v1/messages flow.

use std::time::{Duration, Instant};

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
        insecure: false,
    };

    tokio::spawn(async move {
        execute_with_run_fn(args, run_fn).await.ok();
    });

    // Wait until the server actually answers rather than assuming a fixed
    // sleep is long enough — on a loaded runner it is not.
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if client
            .get(format!("{}/api/v1/sessions", base))
            .send()
            .await
            .is_ok()
        {
            return port;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("server on port {port} never became ready");
}

/// Waits until the server's run registry satisfies `pred`.
///
/// These tests depend on a previously-issued request having been *registered*
/// before the next one is sent. Sleeping a fixed amount and hoping is a race:
/// under load the registration can land after the sleep expires, and the test
/// then sees the wrong status code. `GET /api/v1/sessions` reports the
/// registry directly, so wait on that instead. The deadline is a safety net —
/// a healthy run satisfies `pred` almost immediately.
async fn await_runs(
    client: &reqwest::Client,
    base: &str,
    what: &str,
    pred: impl Fn(&[serde_json::Value]) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last = "<no successful response>".to_string();
    while Instant::now() < deadline {
        if let Ok(resp) = client.get(format!("{}/api/v1/sessions", base)).send().await {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                let empty = Vec::new();
                let sessions = body["sessions"].as_array().unwrap_or(&empty);
                if pred(sessions) {
                    return;
                }
                last = body.to_string();
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}; last /sessions body: {last}");
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

    // The blocker must hold the one allowed slot before the next request is
    // sent, or the limit under test simply is not in force yet.
    await_runs(&client, &base, "the blocker to occupy the only slot", |s| {
        !s.is_empty()
    })
    .await;

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

    // The first resume must be registered against `session_id` before the
    // second is sent, otherwise there is nothing for it to collide with.
    await_runs(&client, &base, "the first resume to register", |runs| {
        runs.iter().any(|r| r["session_id"] == session_id)
    })
    .await;

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
