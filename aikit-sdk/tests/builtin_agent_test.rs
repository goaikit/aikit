//! Unit and integration tests for `run_builtin_agent`.

use aikit_sdk::{run_builtin_agent, OutputMode, RunError, RunOptions};

#[test]
fn test_wrong_agent_key_returns_error() {
    let options = RunOptions::new();
    let mut writer = Vec::new();
    let mut err_writer = Vec::new();

    let result = run_builtin_agent(
        "claude",
        "hello",
        options,
        OutputMode::Plain,
        &mut writer,
        &mut err_writer,
        None,
    );

    assert!(result.is_err());
    match result.unwrap_err() {
        RunError::WrongAgentKey(key) => assert_eq!(key, "claude"),
        e => panic!("expected WrongAgentKey, got {:?}", e),
    }
}

#[test]
fn test_wrong_agent_key_codex() {
    let options = RunOptions::new();
    let mut writer = Vec::new();
    let mut err_writer = Vec::new();

    let result = run_builtin_agent(
        "codex",
        "hello",
        options,
        OutputMode::Events,
        &mut writer,
        &mut err_writer,
        None,
    );

    assert!(matches!(result.unwrap_err(), RunError::WrongAgentKey(_)));
}

#[test]
fn test_missing_progress_sink_returns_error() {
    let options = RunOptions::new();
    let mut writer = Vec::new();
    let mut err_writer = Vec::new();

    let result = run_builtin_agent(
        "aikit",
        "hello",
        options,
        OutputMode::Progress,
        &mut writer,
        &mut err_writer,
        None,
    );

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), RunError::MissingProgressSink));
}

#[test]
fn test_output_mode_variants_are_distinct() {
    assert_ne!(OutputMode::Plain, OutputMode::Events);
    assert_ne!(OutputMode::Events, OutputMode::Progress);
    assert_ne!(OutputMode::Plain, OutputMode::Progress);
    assert_eq!(OutputMode::Events, OutputMode::Events);
}

#[test]
fn test_run_error_display_missing_sink() {
    let err = RunError::MissingProgressSink;
    let msg = err.to_string();
    assert!(msg.contains("ProgressSink"), "msg: {}", msg);
}

#[test]
fn test_run_error_display_wrong_key() {
    let err = RunError::WrongAgentKey("gemini".to_string());
    let msg = err.to_string();
    assert!(msg.contains("gemini"), "msg: {}", msg);
    assert!(msg.contains("aikit"), "msg: {}", msg);
}

/// Verifies that Progress mode with a sink returns MissingProgressSink when sink is None.
#[test]
fn test_embed_missing_sink_error() {
    let options = RunOptions::new();
    let mut writer = Vec::new();
    let mut err_writer = Vec::new();
    let result = run_builtin_agent(
        "aikit",
        "test prompt",
        options,
        OutputMode::Progress,
        &mut writer,
        &mut err_writer,
        None,
    );
    assert!(matches!(result.unwrap_err(), RunError::MissingProgressSink));
}

/// Verifies that calling with wrong key returns WrongAgentKey.
#[test]
fn test_embed_wrong_key_error() {
    let options = RunOptions::new();
    let mut writer = Vec::new();
    let mut err_writer = Vec::new();
    let result = run_builtin_agent(
        "claude",
        "test prompt",
        options,
        OutputMode::Plain,
        &mut writer,
        &mut err_writer,
        None,
    );
    match result.unwrap_err() {
        RunError::WrongAgentKey(k) => assert_eq!(k, "claude"),
        e => panic!("unexpected error: {:?}", e),
    }
}
