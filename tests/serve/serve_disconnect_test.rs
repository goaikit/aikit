//! Tests client disconnect recovery: server cleans up when client drops mid-stream.

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
        insecure: false,
    };
    let stub = make_blocking_stub_run_fn(Duration::from_secs(3));

    tokio::spawn(async move {
        execute_with_run_fn(args, stub).await.ok();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    port
}

#[tokio::test]
async fn test_disconnect_frees_session_slot() {
    let port = start_server().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    // Fire a request with a tight client-side timeout so it drops mid-stream.
    let client2 = reqwest::Client::builder()
        .timeout(Duration::from_millis(200))
        .build()
        .unwrap();

    let _ = client2
        .post(format!("{}/api/v1/messages", base))
        .json(&serde_json::json!({"agent": "aikit", "content": "drop me"}))
        .send()
        .await;

    // Give the server time to notice the disconnect and clean up.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // The session listing should report no active runs.
    let resp = client
        .get(format!("{}/api/v1/sessions", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let active_running = body["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["status"] == "running")
        .count();
    assert_eq!(
        active_running, 0,
        "all runs should be idle/closed after disconnect; got: {:?}",
        body
    );
}
