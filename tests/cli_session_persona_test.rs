//! D2: `aikit agent run --session-persona` on a backend that can't enforce it must fail
//! loud (matching serve's F2 422 `unsupported_tool_policy`), not silently drop the tool
//! policy. See `BackendCapabilities::supports_tool_policy` (aikit-sdk) and the check in
//! `src/cli/run.rs`. No real agent CLI is required: the rejection fires before any agent
//! is dispatched, and the one accepting case uses `--dry-run` to stay hermetic.

use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

const PERSONA_JSON: &str = r#"{"reviewer":{"name":"reviewer","description":"d","prompt":"p"}}"#;

fn aikit_cmd(args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_aikit"));
    cmd.args(args);
    cmd
}

#[test]
fn session_persona_on_external_agent_is_rejected() {
    aikit_cmd(&[
        "agent",
        "run",
        "-a",
        "claude",
        "-p",
        "hi",
        "--session-agents",
        PERSONA_JSON,
        "--session-persona",
        "reviewer",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("--session-persona"))
    .stderr(predicate::str::contains(
        "only enforced by the in-process 'aikit' backend",
    ));
}

#[test]
fn session_persona_on_external_agent_is_rejected_under_dry_run_too() {
    // Dry-run validates config; a silently-dropped tool policy is exactly the kind of
    // config error dry-run exists to catch, so it must reject just like a real run.
    aikit_cmd(&[
        "agent",
        "run",
        "-a",
        "claude",
        "-p",
        "hi",
        "--session-agents",
        PERSONA_JSON,
        "--session-persona",
        "reviewer",
        "--dry-run",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("--session-persona"));
}

#[test]
fn session_persona_on_aikit_backend_passes_the_check() {
    // `--dry-run` keeps this hermetic (no real in-process agent turn is executed) while
    // still exercising persona resolution and the D2 capability check's accept branch.
    aikit_cmd(&[
        "agent",
        "run",
        "-a",
        "aikit",
        "-p",
        "hi",
        "--session-agents",
        PERSONA_JSON,
        "--session-persona",
        "reviewer",
        "--dry-run",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("Session persona: reviewer"));
}
