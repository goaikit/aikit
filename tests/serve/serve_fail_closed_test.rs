//! SEC-2 / ADR 0012: `aikit serve` must refuse to start when bound to a
//! non-loopback address with no `--api-key`, unless `--insecure` is passed
//! explicitly. Loopback stays open with no key required.

use std::time::Duration;

use aikit::cli::serve::{execute_with_run_fn, make_stub_run_fn_with_session, ServeArgs};

#[tokio::test]
async fn test_non_loopback_without_api_key_fails_closed() {
    // The fail-closed check runs before any socket is bound, so an
    // arbitrary (even a priori "unavailable") port is fine here.
    let args = ServeArgs {
        host: "0.0.0.0".to_string(),
        port: 0,
        run_timeout_secs: 30,
        max_sessions: 10,
        api_key: None,
        insecure: false,
    };
    let stub = make_stub_run_fn_with_session(vec![], None);

    let result = execute_with_run_fn(args, stub).await;
    let err = result
        .expect_err("a non-loopback bind with no --api-key and no --insecure must refuse to start");
    let msg = err.to_string();
    assert!(
        msg.contains("--api-key") && msg.contains("--insecure"),
        "error message should explain both ways to fix this: {msg}"
    );
}

#[tokio::test]
async fn test_non_loopback_with_insecure_flag_starts_anyway() {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let args = ServeArgs {
        host: "0.0.0.0".to_string(),
        port,
        run_timeout_secs: 30,
        max_sessions: 10,
        api_key: None,
        insecure: true,
    };
    let stub = make_stub_run_fn_with_session(vec![], None);

    tokio::spawn(async move {
        execute_with_run_fn(args, stub).await.ok();
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/api/v1/agents", port))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "--insecure must actually override the fail-closed check and let the server start"
    );
}

#[tokio::test]
async fn test_non_loopback_with_api_key_starts_fine() {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let args = ServeArgs {
        host: "0.0.0.0".to_string(),
        port,
        run_timeout_secs: 30,
        max_sessions: 10,
        api_key: Some("mysecret".to_string()),
        insecure: false,
    };
    let stub = make_stub_run_fn_with_session(vec![], None);

    tokio::spawn(async move {
        execute_with_run_fn(args, stub).await.ok();
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/api/v1/agents", port))
        .header("Authorization", "Bearer mysecret")
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "a non-loopback bind with a real --api-key must start normally"
    );
}

#[tokio::test]
async fn test_loopback_without_api_key_starts_fine() {
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
    let stub = make_stub_run_fn_with_session(vec![], None);

    tokio::spawn(async move {
        execute_with_run_fn(args, stub).await.ok();
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/api/v1/agents", port))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "loopback bind with no --api-key must stay open per ADR 0012 (existing local \
         consumers — agentrt, the optimization loop, chat BFFs — are unaffected)"
    );
}
