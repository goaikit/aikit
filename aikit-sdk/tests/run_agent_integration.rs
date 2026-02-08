//! Integration tests for aikit-sdk runner module
//!
//! These tests verify that the runner correctly spawns and interacts with
//! agent CLIs. Tests are skipped if the required CLI is not on PATH.

use aikit_sdk::{run_agent, RunOptions};

#[test]
#[ignore = "Requires opencode to be installed on PATH"]
fn test_run_agent_opencode_basic() {
    let result = run_agent("opencode", "echo hello", RunOptions::default()).unwrap();

    assert!(result.success());
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(stdout.contains("hello") || stdout.to_lowercase().contains("hello"));
}

#[test]
#[ignore = "Requires codex to be installed on PATH"]
fn test_run_agent_codex_basic() {
    let result = run_agent("codex", "say hi", RunOptions::default()).unwrap();

    assert!(result.success());
}

#[test]
#[ignore = "Requires claude to be installed on PATH"]
fn test_run_agent_claude_basic() {
    let result = run_agent("claude", "say hi", RunOptions::default()).unwrap();

    assert!(result.success());
}

#[test]
fn test_run_agent_not_runnable() {
    let result = run_agent("copilot", "test", RunOptions::default());

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("not runnable"));
}

#[test]
fn test_run_agent_with_options() {
    let options = RunOptions {
        model: Some("test-model".to_string()),
        yolo: true,
        stream: false,
    };

    let result = run_agent("unknown", "test", options);

    assert!(result.is_err());
}
