//! SEC-3: live sessions must enforce `max_sessions` the same way one-shot
//! runs already do (429 at capacity), since a live session opens a real,
//! unbounded-duration bidirectional subprocess.
//!
//! `max_sessions: 0` makes the very first `POST /live-sessions` hit
//! capacity before any real claude/codex subprocess would ever be spawned,
//! so this test needs no real agent binary installed.

use std::time::Duration;

use aikit::cli::serve::{execute_with_run_fn, make_stub_run_fn, ServeArgs};

async fn start_server(max_sessions: usize) -> u16 {
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
    let stub = make_stub_run_fn();

    tokio::spawn(async move {
        execute_with_run_fn(args, stub).await.ok();
    });

    tokio::time::sleep(Duration::from_millis(150)).await;
    port
}

#[tokio::test]
async fn test_live_session_max_sessions_returns_429_at_capacity() {
    let port = start_server(0).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/live-sessions", port))
        .json(&serde_json::json!({"agent": "claude", "prompt": "hi"}))
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
            .contains('0'),
        "error message must contain the max count: {body:?}"
    );
}

#[tokio::test]
async fn test_live_session_whitespace_only_prompt_rejected() {
    let port = start_server(10).await;
    let client = reqwest::Client::new();

    for prompt in ["   ", "\t", "\n"] {
        let resp = client
            .post(format!("http://127.0.0.1:{}/api/v1/live-sessions", port))
            .json(&serde_json::json!({"agent": "claude", "prompt": prompt}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            422,
            "whitespace-only prompt {prompt:?} must be rejected"
        );
    }
}
