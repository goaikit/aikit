//! BUG-3: a signal-killed agent process must never be reported as a clean
//! exit. `ExitStatus::code()` is `None` on signal death (OOM-kill, `kill
//! -9`, segfault) — that must map to a distinct sentinel and an error
//! frame, not `exit_code: 0`.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use aikit::cli::serve::{execute_with_run_fn, make_production_run_fn, ServeArgs};

/// A fake "agent" binary (spawn name for the `cursor` backend, ADR 0006)
/// that answers `--version` normally but SIGKILLs itself on the real run —
/// simulating an OOM-kill / `kill -9` / segfault.
fn write_self_killing_agent(dir: &std::path::Path) {
    let stub_path = dir.join("agent");
    let mut f = std::fs::File::create(&stub_path).unwrap();
    writeln!(
        f,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo '1.0.0'\n  exit 0\nfi\nkill -9 $$"
    )
    .unwrap();
    let mut perms = f.metadata().unwrap().permissions();
    perms.set_mode(0o755);
    f.set_permissions(perms).unwrap();
}

#[tokio::test]
async fn test_signal_killed_run_is_not_reported_as_success() {
    let dir = tempfile::tempdir().unwrap();
    write_self_killing_agent(dir.path());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let orig_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{}", dir.path().display(), orig_path));

    let args = ServeArgs {
        host: "127.0.0.1".to_string(),
        port,
        run_timeout_secs: 30,
        max_sessions: 10,
        api_key: None,
        insecure: false,
    };
    tokio::spawn(async move {
        execute_with_run_fn(args, make_production_run_fn())
            .await
            .ok();
    });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .json(&serde_json::json!({"agent": "cursor", "content": "die"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();

    assert!(
        text.contains("event: error") && text.contains("abnormal_termination"),
        "a signal-killed run must surface an abnormal_termination error event; got:\n{}",
        text
    );
    assert!(
        !text.contains("\"exit_code\":0"),
        "a signal-killed run must never report exit_code: 0 (success); got:\n{}",
        text
    );
    assert!(
        text.contains("\"exit_code\":137"),
        "a signal-killed run should map to the 137 sentinel; got:\n{}",
        text
    );
}
