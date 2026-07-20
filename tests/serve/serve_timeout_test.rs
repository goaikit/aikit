//! Tests timeout handling: run_timeout SSE error followed by done.

use std::time::Duration;

use aikit::cli::serve::{execute_with_run_fn, make_timeout_stub_run_fn, ServeArgs};

async fn start_timeout_server(timeout_secs: u64) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let args = ServeArgs {
        host: "127.0.0.1".to_string(),
        port,
        run_timeout_secs: timeout_secs,
        max_sessions: 10,
        api_key: None,
        insecure: false,
    };
    let stub = make_timeout_stub_run_fn();

    tokio::spawn(async move {
        execute_with_run_fn(args, stub).await.ok();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    port
}

#[tokio::test]
async fn test_timeout_emits_sse_error_and_done() {
    let port = start_timeout_server(1).await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    let resp = client
        .post(format!("{}/api/v1/messages", base))
        .json(&serde_json::json!({"agent": "aikit", "content": "trigger timeout"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();

    assert!(
        text.contains("event: error"),
        "SSE stream must contain error event; got:\n{}",
        text
    );
    assert!(
        text.contains("run_timeout"),
        "error event must carry run_timeout code; got:\n{}",
        text
    );
    assert!(
        text.contains("event: done"),
        "SSE stream must contain done event after error; got:\n{}",
        text
    );
    assert!(
        text.contains("exit_code"),
        "done event must contain exit_code; got:\n{}",
        text
    );
}
