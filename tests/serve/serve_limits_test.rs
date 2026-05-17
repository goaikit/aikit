//! Tests for capacity and concurrency limits: 429, 409, 422.

use std::time::Duration;

use aikit::cli::serve::{
    execute_with_run_fn, make_blocking_stub_run_fn, make_stub_run_fn, ServeArgs,
};

async fn start_limited_server(max_sessions: usize) -> u16 {
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
    let stub = make_stub_run_fn(vec![]);

    tokio::spawn(async move {
        execute_with_run_fn(args, stub).await.ok();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    port
}

async fn start_blocking_server() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let args = ServeArgs {
        host: "127.0.0.1".to_string(),
        port,
        run_timeout_secs: 30,
        max_sessions: 10,
        api_key: None,
    };
    // Blocking stub runs longer than the test to simulate a long-running agent
    let stub = make_blocking_stub_run_fn(Duration::from_secs(5));

    tokio::spawn(async move {
        execute_with_run_fn(args, stub).await.ok();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    port
}

#[tokio::test]
async fn test_max_sessions_returns_429() {
    let port = start_limited_server(1).await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    // Create the one allowed session
    let resp = client
        .post(format!("{}/v1/sessions", base))
        .json(&serde_json::json!({"agent": "codex"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Attempt to create another — should get 429
    let resp = client
        .post(format!("{}/v1/sessions", base))
        .json(&serde_json::json!({"agent": "codex"}))
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
        "error message must contain max_sessions count"
    );
}

#[tokio::test]
async fn test_empty_content_returns_422() {
    let port = start_limited_server(10).await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    let resp = client
        .post(format!("{}/v1/sessions", base))
        .json(&serde_json::json!({"agent": "codex"}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let session_id = body["session_id"].as_str().unwrap().to_string();

    let resp = client
        .post(format!("{}/v1/sessions/{}/messages", base, session_id))
        .json(&serde_json::json!({"content": ""}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn test_concurrent_message_returns_409() {
    let port = start_blocking_server().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    let resp = client
        .post(format!("{}/v1/sessions", base))
        .json(&serde_json::json!({"agent": "codex"}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let session_id = body["session_id"].as_str().unwrap().to_string();

    let base2 = base.clone();
    let sid2 = session_id.clone();
    let client2 = client.clone();

    // Start a long-running message request
    let first_req = tokio::spawn(async move {
        client
            .post(format!("{}/v1/sessions/{}/messages", base, session_id))
            .json(&serde_json::json!({"content": "hello"}))
            .send()
            .await
            .unwrap()
    });

    // Give it time to transition to Running
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Second message while first is running — expect 409
    let resp = client2
        .post(format!("{}/v1/sessions/{}/messages", base2, sid2))
        .json(&serde_json::json!({"content": "concurrent"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "session_busy");

    // Abort the first request to clean up
    first_req.abort();
}

#[tokio::test]
async fn test_missing_agent_returns_422() {
    let port = start_limited_server(10).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/v1/sessions", port))
        .json(&serde_json::json!({"agent": ""}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "invalid_request");
}
