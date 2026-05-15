//! Unit and integration tests for `run_builtin_agent`.

use aikit_sdk::session_store::SessionStore;
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

/// Resume with a session ID that does not exist returns SessionNotFound.
#[test]
fn test_resume_missing_session_returns_not_found() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::env::set_var("AIKIT_SESSIONS_DIR", tmp.path().to_str().unwrap());
    std::env::set_var("AIKIT_API_KEY", "test-key-unused");

    let options = RunOptions::new().with_session_id("nonexistent-session-id-xyz");
    let mut writer = Vec::new();
    let mut err_writer = Vec::new();

    let result = run_builtin_agent(
        "aikit",
        "test prompt",
        options,
        OutputMode::Plain,
        &mut writer,
        &mut err_writer,
        None,
    );

    std::env::remove_var("AIKIT_SESSIONS_DIR");
    std::env::remove_var("AIKIT_API_KEY");

    match result.unwrap_err() {
        RunError::SessionNotFound(id) => {
            assert_eq!(id, "nonexistent-session-id-xyz");
        }
        e => panic!("expected SessionNotFound, got {:?}", e),
    }
}

/// Resume with a session file containing invalid JSON returns SessionLoadFailed.
#[test]
fn test_resume_corrupt_session_returns_load_failed() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::env::set_var("AIKIT_SESSIONS_DIR", tmp.path().to_str().unwrap());
    std::env::set_var("AIKIT_API_KEY", "test-key-unused");

    let corrupt_id = "corrupt-session-id-abc";
    let session_path = tmp.path().join(format!("{}.json", corrupt_id));
    std::fs::write(&session_path, "not valid json at all {{{{").unwrap();

    let options = RunOptions::new().with_session_id(corrupt_id);
    let mut writer = Vec::new();
    let mut err_writer = Vec::new();

    let result = run_builtin_agent(
        "aikit",
        "test prompt",
        options,
        OutputMode::Plain,
        &mut writer,
        &mut err_writer,
        None,
    );

    std::env::remove_var("AIKIT_SESSIONS_DIR");
    std::env::remove_var("AIKIT_API_KEY");

    match result.unwrap_err() {
        RunError::SessionLoadFailed { id, reason: _ } => {
            assert_eq!(id, corrupt_id);
        }
        e => panic!("expected SessionLoadFailed, got {:?}", e),
    }
}

/// RunError::SessionNotFound formats to the expected error string.
#[test]
fn test_run_error_session_not_found_display() {
    let err = RunError::SessionNotFound("my-session".to_string());
    assert_eq!(err.to_string(), "error: session 'my-session' not found");
}

/// RunError::SessionLoadFailed formats to the expected error string.
#[test]
fn test_run_error_session_load_failed_display() {
    let err = RunError::SessionLoadFailed {
        id: "my-session".to_string(),
        reason: "invalid JSON".to_string(),
    };
    assert_eq!(
        err.to_string(),
        "error: session 'my-session' could not be loaded: invalid JSON"
    );
}

/// RunOptions::default().session_id is None; with_session_id sets it to Some.
#[test]
fn test_run_options_session_id_builder() {
    let opts = RunOptions::new();
    assert_eq!(opts.session_id, None);
    let opts = opts.with_session_id("test-abc");
    assert_eq!(opts.session_id, Some("test-abc".to_string()));
}

/// SessionStore::default() is accessible (implements Default trait).
#[test]
fn test_session_store_default_is_accessible() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::env::set_var("AIKIT_SESSIONS_DIR", tmp.path().to_str().unwrap());
    let store = SessionStore::default();
    assert!(store.sessions_dir.exists());
    std::env::remove_var("AIKIT_SESSIONS_DIR");
}
