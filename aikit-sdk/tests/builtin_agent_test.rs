//! Unit and integration tests for `run_builtin_agent`.

use aikit_agent::llm::mock::{MockGateway, MockResponse};
use aikit_agent::llm::types::{LlmError, LlmRequest, LlmResponse, LlmStreamHandle};
use aikit_agent::LlmGateway;
use aikit_sdk::session_store::SessionStore;
use aikit_sdk::{
    run_aikit_agent_with_gateway, run_builtin_agent, OutputMode, RunError, RunOptions,
};

/// A gateway that records every outbound LlmRequest and delegates responses to a MockGateway.
struct CapturingGateway {
    captured: std::sync::Arc<std::sync::Mutex<Vec<LlmRequest>>>,
    inner: MockGateway,
}

impl CapturingGateway {
    fn new(responses: Vec<MockResponse>) -> Self {
        Self {
            captured: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            inner: MockGateway::new(responses),
        }
    }
}

impl LlmGateway for CapturingGateway {
    fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        self.captured.lock().unwrap().push(req.clone());
        self.inner.complete(req)
    }
    fn stream(&self, req: LlmRequest) -> Result<LlmStreamHandle, LlmError> {
        self.captured.lock().unwrap().push(req.clone());
        self.inner.stream(req)
    }
}

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
    std::env::set_var("AIKIT_API_KEY", "test-key-unused");
    let store = SessionStore {
        sessions_dir: tmp.path().to_path_buf(),
    };

    let options = RunOptions::new().with_session_id("nonexistent-session-id-xyz");
    let gw = MockGateway::new(vec![]);

    let result =
        run_aikit_agent_with_gateway("test prompt", &options, Box::new(gw), Some(store), |_| {});

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
    std::env::set_var("AIKIT_API_KEY", "test-key-unused");
    let store = SessionStore {
        sessions_dir: tmp.path().to_path_buf(),
    };

    let corrupt_id = "corrupt-session-id-abc";
    let session_path = tmp.path().join(format!("{}.json", corrupt_id));
    std::fs::write(&session_path, "not valid json at all {{{{").unwrap();

    let options = RunOptions::new().with_session_id(corrupt_id);
    let gw = MockGateway::new(vec![]);

    let result =
        run_aikit_agent_with_gateway("test prompt", &options, Box::new(gw), Some(store), |_| {});

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

/// New session (no session_id) writes a session file and includes "Session: <id>" in stderr.
/// Criterion 10 and 11.
#[test]
fn test_new_session_creates_file_and_prints_to_stderr() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::env::set_var("AIKIT_API_KEY", "test-key-unused");
    let store = SessionStore {
        sessions_dir: tmp.path().to_path_buf(),
    };

    let options = RunOptions::new();
    let gw = MockGateway::new(vec![MockResponse::text("Hello from mock agent!")]);
    let mut on_event_called = false;
    let result =
        run_aikit_agent_with_gateway("Say hello", &options, Box::new(gw), Some(store), |_event| {
            on_event_called = true;
        });

    std::env::remove_var("AIKIT_API_KEY");

    let result = result.expect("run should succeed");

    // Criterion 11: stderr contains "Session: <id>\n"
    let stderr_str = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr_str.contains("Session: "),
        "stderr should contain 'Session: ', got: {:?}",
        stderr_str
    );

    // Extract the session ID from stderr
    let session_id = stderr_str
        .lines()
        .find(|l| l.starts_with("Session: "))
        .and_then(|l| l.strip_prefix("Session: "))
        .expect("session line should be present")
        .to_string();

    // Criterion 10: session file was written
    let session_path = tmp.path().join(format!("{}.json", session_id));
    assert!(
        session_path.exists(),
        "session file should exist at {:?}",
        session_path
    );

    // Criterion 10: index.json was updated
    let index_path = tmp.path().join("index.json");
    assert!(index_path.exists(), "index.json should exist");
    let index_content = std::fs::read_to_string(&index_path).unwrap();
    assert!(
        index_content.contains(&session_id),
        "index.json should contain the session ID"
    );

    // Criterion 10: session file contains the turns from the run
    let session_content = std::fs::read_to_string(&session_path).unwrap();
    assert!(
        session_content.contains("Say hello"),
        "session file should contain the user prompt"
    );
    assert!(
        session_content.contains("Hello from mock agent!"),
        "session file should contain the agent response"
    );
}

/// Resume with a valid session passes prior turns as the first N messages to the LLM.
/// Criterion 12.
#[test]
fn test_resume_passes_prior_turns_to_llm() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::env::set_var("AIKIT_API_KEY", "test-key-unused");
    let store = SessionStore {
        sessions_dir: tmp.path().to_path_buf(),
    };

    // First, create a session file with prior turns.
    let session_id = "prior-turns-test-session-id-1111";
    let session_json = serde_json::json!({
        "session_id": session_id,
        "agent": "aikit",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:01:00Z",
        "cwd": "/test/dir",
        "turns": [
            { "role": "user", "content": "What is 2+2?" },
            { "role": "assistant", "content": "4" }
        ]
    });
    let session_path = tmp.path().join(format!("{}.json", session_id));
    std::fs::write(&session_path, serde_json::to_string(&session_json).unwrap()).unwrap();

    let options = RunOptions::new().with_session_id(session_id);
    let gw = CapturingGateway::new(vec![MockResponse::text("Based on prior context...")]);
    let captured = std::sync::Arc::clone(&gw.captured);

    let result =
        run_aikit_agent_with_gateway("What is 3+3?", &options, Box::new(gw), Some(store), |_| {});

    std::env::remove_var("AIKIT_API_KEY");

    result.expect("resume should succeed");

    let requests = captured.lock().unwrap();
    assert!(
        !requests.is_empty(),
        "at least one LLM request should be made"
    );
    let first_req = &requests[0];

    let non_system: Vec<_> = first_req
        .messages
        .iter()
        .filter(|m| m.role != "system")
        .collect();

    // Should have: prior user, prior assistant, new user = at least 3 messages
    assert!(
        non_system.len() >= 3,
        "expected at least 3 non-system messages (2 prior + 1 new), got {}",
        non_system.len()
    );
    assert_eq!(
        non_system[0].content.as_deref(),
        Some("What is 2+2?"),
        "first prior turn"
    );
    assert_eq!(non_system[0].role, "user");
    assert_eq!(
        non_system[1].content.as_deref(),
        Some("4"),
        "second prior turn"
    );
    assert_eq!(non_system[1].role, "assistant");
    assert_eq!(
        non_system[2].content.as_deref(),
        Some("What is 3+3?"),
        "new user message"
    );
    assert_eq!(non_system[2].role, "user");
}

/// Two consecutive runs linked by --resume: second run includes all turns from first run.
/// Criterion 17.
#[test]
fn test_two_run_integration_with_resume() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::env::set_var("AIKIT_API_KEY", "test-key-unused");

    // --- First run: no session_id, creates a new session ---
    let options1 = RunOptions::new();
    let gw1 = MockGateway::new(vec![MockResponse::text("First run response")]);
    let store1 = SessionStore {
        sessions_dir: tmp.path().to_path_buf(),
    };
    let result1 = run_aikit_agent_with_gateway(
        "First prompt",
        &options1,
        Box::new(gw1),
        Some(store1),
        |_| {},
    );
    let result1 = result1.expect("first run should succeed");

    // Extract session ID from stderr
    let stderr1 = String::from_utf8_lossy(&result1.stderr);
    let session_id = stderr1
        .lines()
        .find(|l| l.starts_with("Session: "))
        .and_then(|l| l.strip_prefix("Session: "))
        .expect("first run should print session ID")
        .to_string();

    // Verify the session file was written
    let session_path = tmp.path().join(format!("{}.json", session_id));
    assert!(
        session_path.exists(),
        "session file from first run should exist"
    );

    // --- Second run: with session_id, resumes the first session ---
    let options2 = RunOptions::new().with_session_id(&session_id);
    let gw2 = CapturingGateway::new(vec![MockResponse::text("Second run response")]);
    let captured = std::sync::Arc::clone(&gw2.captured);
    let store2 = SessionStore {
        sessions_dir: tmp.path().to_path_buf(),
    };

    let result2 = run_aikit_agent_with_gateway(
        "Second prompt",
        &options2,
        Box::new(gw2),
        Some(store2),
        |_| {},
    );

    std::env::remove_var("AIKIT_API_KEY");

    result2.expect("second run should succeed");

    // Verify second run's LLM request includes all turns from first run + new prompt
    let requests = captured.lock().unwrap();
    assert!(
        !requests.is_empty(),
        "at least one LLM request in second run"
    );
    let first_req = &requests[0];

    let non_system: Vec<_> = first_req
        .messages
        .iter()
        .filter(|m| m.role != "system")
        .collect();

    // Should have: first-run user, first-run assistant, second-run user = at least 3
    assert!(
        non_system.len() >= 3,
        "second run should include prior turns + new prompt, got {} non-system messages",
        non_system.len()
    );

    // First turn from run 1: the user prompt
    assert_eq!(
        non_system[0].content.as_deref(),
        Some("First prompt"),
        "first non-system message should be the first run's user prompt"
    );
    assert_eq!(non_system[0].role, "user");

    // Second turn from run 1: the assistant response
    assert_eq!(
        non_system[1].content.as_deref(),
        Some("First run response"),
        "second non-system message should be the first run's assistant response"
    );
    assert_eq!(non_system[1].role, "assistant");

    // Last turn: the new user prompt for run 2
    let last = non_system.last().unwrap();
    assert_eq!(
        last.content.as_deref(),
        Some("Second prompt"),
        "last non-system message should be the second run's user prompt"
    );
    assert_eq!(last.role, "user");
}
