//! Tests client disconnect recovery: session returns to idle when client drops mid-stream.

use std::time::Duration;

use aikit::cli::serve::{execute_with_run_fn, make_blocking_stub_run_fn, ServeArgs};

async fn start_server() -> u16 {
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
    // Blocking stub runs longer than the client timeout (200ms) to simulate a long-running agent
    let stub = make_blocking_stub_run_fn(Duration::from_secs(3));

    tokio::spawn(async move {
        execute_with_run_fn(args, stub).await.ok();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    port
}

#[tokio::test]
async fn test_disconnect_session_returns_idle() {
    let port = start_server().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    // Create a session
    let resp = client
        .post(format!("{}/v1/sessions", base))
        .json(&serde_json::json!({"agent": "codex"}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let session_id = body["session_id"].as_str().unwrap().to_string();

    // Start message request in background and immediately drop it (simulate disconnect)
    let client2 = reqwest::Client::builder()
        .timeout(Duration::from_millis(200))
        .build()
        .unwrap();

    let _ = client2
        .post(format!("{}/v1/sessions/{}/messages", base, session_id))
        .json(&serde_json::json!({"content": "disconnect me"}))
        .send()
        .await;
    // The request either times out or connects; either way the connection drops quickly

    // Give the server time to notice the disconnect and clean up
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Session should be idle (not running)
    let resp = client
        .get(format!("{}/v1/sessions/{}", base, session_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["status"], "idle",
        "session must return to idle after client disconnect; status: {}",
        body["status"]
    );
}
