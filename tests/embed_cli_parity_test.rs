//! Parity tests: verifies that `run_builtin_agent` embed API produces identical
//! output to `aikit run -a agent` CLI for the same prompt and mock LLM.
//!
//! All tests use a local mockito HTTP server as the LLM backend so no network
//! access is required.

use std::sync::{Mutex, OnceLock};

use aikit_sdk::{run_builtin_agent, OutputMode, ProgressSink, RunError, RunOptions, RunProgress};
use serde_json::Value;

/// Serialises env-var access across tests in this binary to avoid races.
fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

/// Minimal non-streaming OpenAI chat-completion response that makes the agent
/// loop exit after a single iteration (`finish_reason: "stop"`, no tool calls).
fn chat_completion_json(content: &str) -> String {
    serde_json::json!({
        "id": "chatcmpl-parity",
        "object": "chat.completion",
        "created": 1_700_000_000u64,
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": content },
            "finish_reason": "stop"
        }]
    })
    .to_string()
}

/// Parse NDJSON output and strip the `seq` field from every object so that
/// event sequences from two independent runs can be compared for equality.
fn parse_ndjson_no_seq(output: &str) -> Vec<Value> {
    output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let mut v: Value = serde_json::from_str(line).unwrap_or(Value::Null);
            if let Value::Object(ref mut map) = v {
                map.remove("seq");
            }
            v
        })
        .collect()
}

// ── error-path tests (no network, no env-var manipulation) ───────────────────

/// `run_builtin_agent` with any key other than "aikit" returns `WrongAgentKey`.
#[test]
fn embed_wrong_key_error() {
    let result = run_builtin_agent(
        "claude",
        "test",
        RunOptions::new(),
        OutputMode::Plain,
        &mut Vec::new(),
        &mut Vec::new(),
        None,
    );
    assert!(
        matches!(result.unwrap_err(), RunError::WrongAgentKey(_)),
        "expected WrongAgentKey"
    );
}

/// `OutputMode::Progress` with `sink = None` returns `MissingProgressSink`.
#[test]
fn embed_missing_sink_error() {
    let result = run_builtin_agent(
        "aikit",
        "test",
        RunOptions::new(),
        OutputMode::Progress,
        &mut Vec::new(),
        &mut Vec::new(),
        None,
    );
    assert!(
        matches!(result.unwrap_err(), RunError::MissingProgressSink),
        "expected MissingProgressSink"
    );
}

// ── parity tests (mock HTTP LLM server) ──────────────────────────────────────

/// Embed `OutputMode::Events` NDJSON must match `aikit run -a agent --events`
/// NDJSON line-for-line (excluding the `seq` field).
#[test]
fn embed_events_matches_cli_events() {
    let _guard = env_lock();

    let mut server = mockito::Server::new();
    let response_body = chat_completion_json("Hello from events parity test");

    // Single mock endpoint responds to all POST requests (no .expect() limit).
    let _mock = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(&response_body)
        .create();

    let llm_url = format!("{}/v1", server.url());

    // Set env vars so the in-process embed call hits the mock server.
    std::env::set_var("AIKIT_LLM_URL", &llm_url);
    std::env::set_var("OPENAI_API_KEY", "fake-key-for-parity");

    // ── embed API call ────────────────────────────────────────────────────────
    let mut embed_writer: Vec<u8> = Vec::new();
    let mut embed_err: Vec<u8> = Vec::new();
    run_builtin_agent(
        "aikit",
        "parity test prompt",
        RunOptions::new().with_model("gpt-4o".to_string()),
        OutputMode::Events,
        &mut embed_writer,
        &mut embed_err,
        None,
    )
    .expect("embed events call failed");

    let embed_events = parse_ndjson_no_seq(&String::from_utf8_lossy(&embed_writer));
    assert!(
        !embed_events.is_empty(),
        "embed API produced no NDJSON events"
    );

    // ── CLI subprocess call ───────────────────────────────────────────────────
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aikit"))
        .env("AIKIT_LLM_URL", &llm_url)
        .env("OPENAI_API_KEY", "fake-key-for-parity")
        .arg("run")
        .arg("-a")
        .arg("agent")
        .arg("-p")
        .arg("parity test prompt")
        .arg("--events")
        .output()
        .expect("failed to spawn aikit CLI");

    let cli_events = parse_ndjson_no_seq(&String::from_utf8_lossy(&output.stdout));
    assert!(!cli_events.is_empty(), "CLI produced no NDJSON events");

    assert_eq!(
        embed_events, cli_events,
        "embed API and CLI event sequences differ (seq field excluded)"
    );
}

/// Embed `OutputMode::Plain` writer bytes must match `aikit run -a agent`
/// stdout byte-for-byte.
#[test]
fn embed_plain_matches_cli_default() {
    let _guard = env_lock();

    let mut server = mockito::Server::new();
    let response_body = chat_completion_json("Plain text response for parity");

    let _mock = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(&response_body)
        .create();

    let llm_url = format!("{}/v1", server.url());

    std::env::set_var("AIKIT_LLM_URL", &llm_url);
    std::env::set_var("OPENAI_API_KEY", "fake-key-for-parity");

    // ── embed API call ────────────────────────────────────────────────────────
    let mut embed_writer: Vec<u8> = Vec::new();
    let mut embed_err: Vec<u8> = Vec::new();
    run_builtin_agent(
        "aikit",
        "plain parity prompt",
        RunOptions::new().with_model("gpt-4o".to_string()),
        OutputMode::Plain,
        &mut embed_writer,
        &mut embed_err,
        None,
    )
    .expect("embed plain call failed");

    assert!(!embed_writer.is_empty(), "embed plain produced no output");

    // ── CLI subprocess call ───────────────────────────────────────────────────
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aikit"))
        .env("AIKIT_LLM_URL", &llm_url)
        .env("OPENAI_API_KEY", "fake-key-for-parity")
        .arg("run")
        .arg("-a")
        .arg("agent")
        .arg("-p")
        .arg("plain parity prompt")
        .output()
        .expect("failed to spawn aikit CLI");

    let cli_stdout = output.stdout;
    assert!(!cli_stdout.is_empty(), "CLI plain produced no output");

    assert_eq!(
        embed_writer, cli_stdout,
        "embed API and CLI plain output differ byte-for-byte"
    );
}

/// In `OutputMode::Progress` mode, `on_progress` must be called at least once
/// and `on_finalize` exactly once with exit code 0.
#[test]
fn embed_progress_sink_called() {
    let _guard = env_lock();

    let mut server = mockito::Server::new();
    let response_body = chat_completion_json("Progress sink test response");

    let _mock = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(&response_body)
        .create();

    let llm_url = format!("{}/v1", server.url());

    std::env::set_var("AIKIT_LLM_URL", &llm_url);
    std::env::set_var("OPENAI_API_KEY", "fake-key-for-parity");

    use std::sync::{Arc, Mutex};

    struct TestSink {
        progress_calls: Arc<Mutex<usize>>,
        finalize_calls: Arc<Mutex<usize>>,
        exit_codes: Arc<Mutex<Vec<i32>>>,
    }

    impl ProgressSink for TestSink {
        fn on_progress(&mut self, _progress: &RunProgress) {
            *self.progress_calls.lock().unwrap() += 1;
        }
        fn on_finalize(&mut self, exit_code: i32, _token_footer: Option<String>) {
            *self.finalize_calls.lock().unwrap() += 1;
            self.exit_codes.lock().unwrap().push(exit_code);
        }
    }

    let progress_calls = Arc::new(Mutex::new(0usize));
    let finalize_calls = Arc::new(Mutex::new(0usize));
    let exit_codes = Arc::new(Mutex::new(Vec::<i32>::new()));

    let sink = Box::new(TestSink {
        progress_calls: Arc::clone(&progress_calls),
        finalize_calls: Arc::clone(&finalize_calls),
        exit_codes: Arc::clone(&exit_codes),
    });

    let mut writer: Vec<u8> = Vec::new();
    let mut err_writer: Vec<u8> = Vec::new();

    run_builtin_agent(
        "aikit",
        "progress sink test prompt",
        RunOptions::new().with_model("gpt-4o".to_string()),
        OutputMode::Progress,
        &mut writer,
        &mut err_writer,
        Some(sink),
    )
    .expect("embed progress call failed");

    assert!(
        *progress_calls.lock().unwrap() >= 1,
        "on_progress should have been called at least once"
    );
    assert_eq!(
        *finalize_calls.lock().unwrap(),
        1,
        "on_finalize should have been called exactly once"
    );
    assert_eq!(
        exit_codes.lock().unwrap().first().copied(),
        Some(0),
        "exit code for a successful run should be 0"
    );
}
