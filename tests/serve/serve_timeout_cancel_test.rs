//! BUG-2 / ADR 0014: a run that exceeds its timeout must be *actually*
//! cancelled — the subprocess killed via the shared `RunCancelHandle`, not
//! merely abandoned — and the session record must land in a terminal state
//! (`Closed`), never left `Idle` (which would let a second POST for the
//! same session_id start a concurrent run on top of a still-dying first
//! one, corrupting the session file per BUG-2/BUG-8).
//!
//! Uses the *real* production run path (`make_production_run_fn`, which
//! drives `run_agent_events_cancellable`) against a genuinely slow fake
//! agent binary on PATH — not an instant stub — so the actual cancel /
//! process-group-kill machinery from ADR 0014 is exercised end to end.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use aikit::cli::serve::{execute_with_run_fn, make_production_run_fn, ServeArgs};

/// Writes a fake "agent" binary (the spawn name `build_argv` uses for the
/// `cursor` backend — see ADR 0006) that answers `--version` instantly (so
/// the availability probe reports it runnable) but otherwise sleeps far
/// longer than any timeout under test, simulating a real hung/slow agent
/// process.
fn write_fake_slow_agent(dir: &std::path::Path) {
    let stub_path = dir.join("agent");
    let mut f = std::fs::File::create(&stub_path).unwrap();
    writeln!(
        f,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo '1.0.0'\n  exit 0\nfi\necho 'before sleep'\nsleep 60"
    )
    .unwrap();
    let mut perms = f.metadata().unwrap().permissions();
    perms.set_mode(0o755);
    f.set_permissions(perms).unwrap();
}

async fn start_server(timeout_secs: u64, path_dir: &std::path::Path) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let orig_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{}", path_dir.display(), orig_path));

    let args = ServeArgs {
        host: "127.0.0.1".to_string(),
        port,
        run_timeout_secs: timeout_secs,
        max_sessions: 10,
        api_key: None,
        insecure: false,
    };
    let run_fn = make_production_run_fn();

    tokio::spawn(async move {
        execute_with_run_fn(args, run_fn).await.ok();
    });

    tokio::time::sleep(Duration::from_millis(150)).await;
    port
}

#[tokio::test]
async fn test_timeout_actually_cancels_real_run_and_reaches_terminal_state() {
    let dir = tempfile::tempdir().unwrap();
    write_fake_slow_agent(dir.path());

    let port = start_server(1, dir.path()).await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    let start = std::time::Instant::now();
    let resp = client
        .post(format!("{}/api/v1/messages", base))
        .json(&serde_json::json!({"agent": "cursor", "content": "trigger a real slow run"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    let elapsed = start.elapsed();

    // The 1s configured timeout plus the SIGTERM->(~3s grace)->SIGKILL
    // escalation should finish well within this generous bound. If the
    // cancel handle were not actually wired through, the fake agent would
    // sleep for 60s and this would time out the whole test instead.
    assert!(
        elapsed < Duration::from_secs(20),
        "SSE response should complete promptly once the run is actually \
         cancelled (not merely abandoned), took {:?}",
        elapsed
    );
    assert!(
        text.contains("event: error") && text.contains("run_timeout"),
        "expected a run_timeout error event; got:\n{}",
        text
    );
    assert!(text.contains("event: done"));

    // Pull the server-minted session_id out of the very first SSE frame.
    let session_line = text
        .lines()
        .find(|l| l.starts_with("data:") && l.contains("session_id"))
        .expect("expected a session frame");
    let session_json: serde_json::Value =
        serde_json::from_str(session_line.trim_start_matches("data: ")).unwrap();
    let session_id = session_json["session_id"].as_str().unwrap();

    // BUG-2: the record must be terminal (Closed), not Idle. GET filters
    // Closed records out entirely, so a 404 here proves the run did not
    // land back in a resumable Idle state — an Idle record would still
    // 200 here and would also pass spawn_run's busy check, allowing a
    // second concurrent run on the same session.
    let get_resp = client
        .get(format!("{}/api/v1/sessions/{}", base, session_id))
        .send()
        .await
        .unwrap();
    assert_eq!(
        get_resp.status(),
        404,
        "a timed-out session must reach a terminal state, not remain Idle/resumable"
    );

    // And a resume POST for that session_id must also be rejected (BUG-10
    // territory too: a terminal session must not be resurrectable).
    let resume_resp = client
        .post(format!("{}/api/v1/messages", base))
        .json(&serde_json::json!({
            "agent": "cursor",
            "session_id": session_id,
            "content": "try to resume the dead session",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resume_resp.status(),
        404,
        "resuming a terminated session must not be possible"
    );
}
