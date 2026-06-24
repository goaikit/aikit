//! Integration smoke test. Skips automatically when `codex` is not on PATH,
//! so this is safe to run in CI environments that don't have Codex installed.

use aikit_agent_codex::{CodexClient, SpawnOptions};

#[tokio::test]
async fn initialize_handshake_round_trip() {
    if which::which("codex").is_err() {
        eprintln!("skipping: 'codex' not on PATH");
        return;
    }
    let (client, _events) = CodexClient::spawn().await.expect("spawn");
    let result = client
        .initialize("aikit_agent_codex_test", "aikit-agent-codex test", "0.0.0")
        .await
        .expect("initialize");
    assert!(
        result.get("codexHome").is_some() || result.get("userAgent").is_some(),
        "expected codexHome or userAgent in initialize response, got: {result}"
    );
    client.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn spawn_with_missing_binary_returns_spawn_error() {
    let opts = SpawnOptions {
        codex_bin: "definitely-not-a-real-codex-binary-xyz".into(),
        ..Default::default()
    };
    let result = CodexClient::spawn_with(opts).await;
    assert!(result.is_err(), "expected spawn to fail for missing binary");
}

#[tokio::test]
async fn double_initialize_is_rejected() {
    if which::which("codex").is_err() {
        eprintln!("skipping: 'codex' not on PATH");
        return;
    }
    let (client, _events) = CodexClient::spawn().await.expect("spawn");
    client
        .initialize("aikit_agent_codex_test", "aikit-agent-codex test", "0.0.0")
        .await
        .expect("first initialize");
    let second = client
        .initialize("aikit_agent_codex_test", "aikit-agent-codex test", "0.0.0")
        .await;
    assert!(second.is_err(), "second initialize should fail");
    client.shutdown().await.expect("shutdown");
}
