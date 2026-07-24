//! CLI integration tests for spec 013 invocation-envelope flags. These exercise
//! the agent-free paths (`--capabilities` short-circuits before any run) and
//! flag registration, so no agent binary is required.

use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn spec013_capabilities_codex_matrix() {
    let mut cmd = Command::cargo_bin("aikit").unwrap();
    cmd.args(["agent", "run", "--agent", "codex", "--capabilities"]);
    cmd.assert()
        .success()
        .stdout(contains("backend: codex"))
        .stdout(contains("sandbox:"))
        .stdout(contains("supported (os-enforced)"))
        .stdout(contains("auto-approve"))
        .stdout(contains("working-dir"));
}

#[test]
fn spec013_capabilities_cursor_marks_unsupported() {
    let mut cmd = Command::cargo_bin("aikit").unwrap();
    cmd.args(["agent", "run", "--agent", "cursor", "--capabilities"]);
    cmd.assert()
        .success()
        .stdout(contains("backend: cursor"))
        .stdout(contains("unsupported"));
}

#[test]
fn spec013_capabilities_unknown_agent_fails() {
    let mut cmd = Command::cargo_bin("aikit").unwrap();
    cmd.args(["agent", "run", "--agent", "bogus", "--capabilities"]);
    cmd.assert().failure().stderr(contains("unknown agent"));
}

#[test]
fn spec013_flags_appear_in_help() {
    let mut cmd = Command::cargo_bin("aikit").unwrap();
    cmd.args(["agent", "run", "--help"]);
    cmd.assert()
        .success()
        .stdout(contains("--sandbox"))
        .stdout(contains("--auto-approve"))
        .stdout(contains("--cd"))
        .stdout(contains("--add-dir"))
        .stdout(contains("--output-result"))
        .stdout(contains("--output-schema"))
        .stdout(contains("--bare"))
        .stdout(contains("--ephemeral"))
        .stdout(contains("--skip-git-repo-check"))
        .stdout(contains("--capabilities"));
}
