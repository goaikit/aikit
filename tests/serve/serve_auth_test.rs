//! Tests for bearer-token authentication middleware.

use std::time::Duration;

use aikit::cli::serve::{execute_with_run_fn, make_stub_run_fn_with_session, ServeArgs};

async fn start_auth_server(api_key: &str) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let args = ServeArgs {
        host: "127.0.0.1".to_string(),
        port,
        run_timeout_secs: 30,
        max_sessions: 10,
        api_key: Some(api_key.to_string()),
    };
    let stub = make_stub_run_fn_with_session(vec![], None);

    tokio::spawn(async move {
        execute_with_run_fn(args, stub).await.ok();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    port
}

#[tokio::test]
async fn test_missing_auth_returns_401() {
    let port = start_auth_server("mysecret").await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://127.0.0.1:{}/health", port))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 401);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "unauthorized");
}

#[tokio::test]
async fn test_wrong_key_returns_401() {
    let port = start_auth_server("mysecret").await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://127.0.0.1:{}/health", port))
        .header("Authorization", "Bearer wrongkey")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 401);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "unauthorized");
}

#[tokio::test]
async fn test_correct_key_succeeds() {
    let port = start_auth_server("mysecret").await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://127.0.0.1:{}/health", port))
        .header("Authorization", "Bearer mysecret")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
}
