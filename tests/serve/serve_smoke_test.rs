//! Integration test: start the server on 127.0.0.1:0, exercise all CRUD endpoints.
//! Agent execution is stubbed — no LLM credentials required.

use std::time::Duration;

use aikit::cli::serve::{execute_with_run_fn, make_stub_run_fn, ServeArgs};

fn make_args(port: u16) -> ServeArgs {
    ServeArgs {
        host: "127.0.0.1".to_string(),
        port,
        run_timeout_secs: 30,
        max_sessions: 10,
        api_key: None,
    }
}

async fn start_server() -> u16 {
    // Bind port 0 to get an OS-assigned port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let args = make_args(port);
    let stub = make_stub_run_fn(vec![("text", r#"{"content":"Hello from stub!"}"#)]);

    tokio::spawn(async move {
        execute_with_run_fn(args, stub).await.ok();
    });

    // Give the server a moment to bind
    tokio::time::sleep(Duration::from_millis(100)).await;
    port
}

#[tokio::test]
async fn test_health_endpoint() {
    let port = start_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://127.0.0.1:{}/health", port))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert!(
        body["version"].as_str().is_some(),
        "version field must be present"
    );
}

#[tokio::test]
async fn test_agents_endpoint() {
    let port = start_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://127.0.0.1:{}/v1/agents", port))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["agents"].is_array(), "agents must be an array");
}

#[tokio::test]
async fn test_session_lifecycle() {
    let port = start_server().await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    // Create session
    let resp = client
        .post(format!("{}/v1/sessions", base))
        .json(&serde_json::json!({"agent": "codex"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    let session_id = body["session_id"].as_str().unwrap().to_string();

    // Validate UUID v4 format
    assert_eq!(session_id.len(), 36, "session_id must be a UUID");

    // Get session - should be idle
    let resp = client
        .get(format!("{}/v1/sessions/{}", base, session_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "idle");

    // List sessions - should include our session
    let resp = client
        .get(format!("{}/v1/sessions", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let sessions = body["sessions"].as_array().unwrap();
    assert!(
        sessions.iter().any(|s| s["session_id"] == session_id),
        "session must appear in list"
    );

    // Send a message and consume SSE stream
    let resp = client
        .post(format!("{}/v1/sessions/{}/messages", base, session_id))
        .json(&serde_json::json!({"content": "hello"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(
        resp.headers()
            .get("content-type")
            .map(|v| v.to_str().unwrap_or("").contains("text/event-stream"))
            .unwrap_or(false),
        "response must be SSE"
    );

    let text = resp.text().await.unwrap();
    assert!(
        text.contains("event: done"),
        "SSE stream must contain done event; got:\n{}",
        text
    );
    assert!(
        text.contains("exit_code"),
        "done event must contain exit_code; got:\n{}",
        text
    );

    // After done, session should be idle
    tokio::time::sleep(Duration::from_millis(50)).await;
    let resp = client
        .get(format!("{}/v1/sessions/{}", base, session_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "idle");

    // Delete session
    let resp = client
        .delete(format!("{}/v1/sessions/{}", base, session_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "closed");

    // Get after delete should be 404
    let resp = client
        .get(format!("{}/v1/sessions/{}", base, session_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_not_found_route() {
    let port = start_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://127.0.0.1:{}/v1/nonexistent", port))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "not_found");
}
