//! CLI integration tests for `aikit run --progress` flag behaviour.

use assert_cmd::Command;
use predicates::str::contains;

/// `--progress` and `--events` are mutually exclusive.
#[test]
fn test_progress_conflicts_with_events() {
    let mut cmd = Command::cargo_bin("aikit").unwrap();
    cmd.args(["run", "--agent", "opencode", "--progress", "--events", "-p", "test"]);
    cmd.assert()
        .failure()
        .stderr(contains("cannot be used with"));
}

/// Dry-run with `--progress` reports progress mode in output.
#[test]
fn test_progress_dry_run_output() {
    let mut cmd = Command::cargo_bin("aikit").unwrap();
    cmd.args(["run", "--agent", "opencode", "--progress", "--dry-run", "-p", "hello"]);
    cmd.assert()
        .success()
        .stdout(contains("Progress mode: true"))
        .stdout(contains("Configuration validated successfully (dry-run)"));
}

/// Dry-run without `--progress` shows `false` for progress mode.
#[test]
fn test_no_progress_dry_run_output() {
    let mut cmd = Command::cargo_bin("aikit").unwrap();
    cmd.args(["run", "--agent", "opencode", "--dry-run", "-p", "hello"]);
    cmd.assert()
        .success()
        .stdout(contains("Progress mode: false"));
}

/// `--progress` flag appears in help output.
#[test]
fn test_progress_flag_in_help() {
    let mut cmd = Command::cargo_bin("aikit").unwrap();
    cmd.args(["run", "--help"]);
    cmd.assert()
        .success()
        .stdout(contains("--progress"));
}
