//! CLI-level tests for `aikit session sync` (`execute_sync`).
//!
//! Covers the command glue that the in-crate `aikit-session-sync` tests can't
//! reach: env-var resolution, the fail-closed owner rule, `--format`
//! validation, and exit-code mapping. These call `execute_sync` in-process
//! (not the spawned binary) so we can assert exit codes precisely.
//!
//! `execute_sync` reads process-global env vars, so every test serializes on a
//! shared lock and starts from a cleaned env, restoring on drop. The exit-2
//! config/auth paths all return *before* any S3 client is built or any file is
//! written, so they need neither network nor a real home.
#![cfg(feature = "agent-adapters")]

use std::sync::OnceLock;

use aikit::cli::session::{execute_sync, SyncSessionsArgs};
use tokio::sync::{Mutex, MutexGuard};

const VARS: &[&str] = &[
    "AIKIT_SYNC_BUCKET",
    "AIKIT_SYNC_ENDPOINT",
    "AIKIT_SYNC_REGION",
    "AIKIT_SYNC_OWNER",
    "AIKIT_SYNC_PREFIX",
    "AIKIT_SYNC_ALLOW_HTTP",
    "AIKIT_SYNC_ENDPOINT_CA_BUNDLE",
    "AIKIT_SYNC_CREDENTIAL_OWNER",
    "RUST_LOG",
    // Cleared so the S3 credential preflight sees a known-empty environment.
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_PROFILE",
    "AWS_DEFAULT_PROFILE",
    "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
    "AWS_CONTAINER_CREDENTIALS_FULL_URI",
    "AWS_WEB_IDENTITY_TOKEN_FILE",
    "AIKIT_SYNC_ALLOW_INSTANCE_CREDENTIALS",
];

/// Serialize env-mutating tests on an async-aware lock (held across `.await`)
/// and start each from a cleaned env.
async fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let guard = LOCK.get_or_init(|| Mutex::new(())).lock().await;
    for v in VARS {
        std::env::remove_var(v);
    }
    guard
}

/// Defaults: no flags set. Tests override the fields they exercise.
fn args() -> SyncSessionsArgs {
    SyncSessionsArgs {
        bucket: None,
        endpoint: None,
        region: None,
        owner: None,
        key_prefix: None,
        tools: vec![],
        watch: false,
        dry_run: false,
        allow_http: false,
        format: "default".to_string(),
        log_level: None,
        log_format: "text".to_string(),
    }
}

#[tokio::test]
async fn invalid_format_returns_2() {
    let _g = env_lock().await;
    let mut a = args();
    a.format = "yaml".to_string();
    a.owner = Some("alice".into());
    assert_eq!(execute_sync(a).await.unwrap(), 2);
}

#[tokio::test]
async fn missing_bucket_and_endpoint_returns_2() {
    let _g = env_lock().await;
    let mut a = args();
    a.owner = Some("alice".into()); // owner ok, but no bucket/endpoint
    assert_eq!(execute_sync(a).await.unwrap(), 2);
}

#[tokio::test]
async fn missing_owner_returns_2() {
    let _g = env_lock().await;
    let mut a = args();
    a.bucket = Some("b".into());
    a.endpoint = Some("http://127.0.0.1:9000".into());
    a.allow_http = true;
    // No owner anywhere → resolve_owner fails closed.
    assert_eq!(execute_sync(a).await.unwrap(), 2);
}

#[tokio::test]
async fn owner_mismatch_fails_closed_returns_2() {
    let _g = env_lock().await;
    std::env::set_var("AIKIT_SYNC_CREDENTIAL_OWNER", "bob");
    let mut a = args();
    a.bucket = Some("b".into());
    a.endpoint = Some("http://127.0.0.1:9000".into());
    a.allow_http = true;
    a.owner = Some("alice".into()); // disagrees with credential owner "bob"
    assert_eq!(execute_sync(a).await.unwrap(), 2);
}

/// Owner resolves and config is complete, but no AWS credential source is in the
/// environment (env_lock cleaned them). A real (non-dry-run) sync must fail fast
/// with an auth error → exit 2, rather than falling into the IMDS retry loop.
/// No network: the preflight returns before any S3 client is built.
#[tokio::test]
async fn missing_aws_credentials_returns_2() {
    let _g = env_lock().await;
    let mut a = args();
    a.bucket = Some("b".into());
    a.endpoint = Some("http://127.0.0.1:9000".into());
    a.allow_http = true;
    a.owner = Some("alice".into()); // owner ok → construction is reached
    assert_eq!(execute_sync(a).await.unwrap(), 2);
}

/// Config supplied entirely through env (no flags) resolves and a dry run
/// completes with exit 0 — proving the env fallbacks are wired. Unix-only and
/// hermetic: `$HOME` is redirected to a temp dir so the state store and the
/// adapters' `~/.claude`/`~/.codex` scans stay inside the sandbox and never
/// touch the real home (dirs::home_dir honors $HOME on unix; on Windows it
/// would not, which is why this is gated).
#[cfg(unix)]
#[tokio::test]
async fn env_fallbacks_resolve_and_dry_run_exits_0() {
    let _g = env_lock().await;
    let home = tempfile::tempdir().unwrap();
    let prev_home = std::env::var_os("HOME");
    std::env::set_var("HOME", home.path());
    std::env::set_var("AIKIT_SYNC_BUCKET", "env-bucket");
    std::env::set_var("AIKIT_SYNC_ENDPOINT", "http://127.0.0.1:9000");
    std::env::set_var("AIKIT_SYNC_OWNER", "env-alice");

    let mut a = args();
    a.dry_run = true; // no network; InMemorySink

    let code = execute_sync(a).await.unwrap();

    match prev_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
    assert_eq!(
        code, 0,
        "env-resolved dry run over an empty home should succeed"
    );
}
