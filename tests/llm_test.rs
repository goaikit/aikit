//! Integration tests for the `aikit llm` command
//!
//! Uses mockito to mock the OpenAI-compatible API endpoint.

#![allow(deprecated)]

use assert_cmd::Command;
use predicates::prelude::*;

fn aikit() -> Command {
    Command::cargo_bin("aikit").unwrap()
}

fn mock_openai_response() -> serde_json::Value {
    serde_json::json!({
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello! How can I help you?"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 8,
            "total_tokens": 18
        }
    })
}

fn mock_stream_chunks() -> String {
    let chunks = [
        r#"data: {"id":"chatcmpl-1","model":"gpt-4o","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":null}]}"#,
        r#"data: {"id":"chatcmpl-1","model":"gpt-4o","choices":[{"index":0,"delta":{"content":"!"},"finish_reason":null}]}"#,
        r#"data: {"id":"chatcmpl-1","model":"gpt-4o","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        r#"data: {"id":"chatcmpl-1","model":"gpt-4o","choices":[{"index":0,"delta":{}}],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}}"#,
        "data: [DONE]",
    ];
    chunks.iter().map(|c| format!("{}\n\n", c)).collect()
}

fn mock_malformed_stream_chunks() -> String {
    let chunks = [
        r#"data: {"id":"chatcmpl-1","model":"gpt-4o","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#,
        r#"data: {this is not valid json}"#,
        "data: [DONE]",
    ];
    chunks.iter().map(|c| format!("{}\n\n", c)).collect()
}

#[test]
fn test_llm_non_streaming_text_mode() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&mock_openai_response()).unwrap())
        .create();

    let url = server.url();

    aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args(["llm", "-m", "gpt-4o", "-p", "hello", "-u", &url])
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello! How can I help you?"));

    mock.assert();
}

#[test]
fn test_llm_non_streaming_json_mode() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&mock_openai_response()).unwrap())
        .create();

    let url = server.url();

    let result = aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args([
            "llm", "-m", "gpt-4o", "-p", "hello", "-u", &url, "--format", "json",
        ])
        .assert()
        .success();

    let output = String::from_utf8(result.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(parsed["schema_version"], "1.0");
    assert_eq!(parsed["model"], "gpt-4o");
    assert_eq!(parsed["content"], "Hello! How can I help you?");
    assert_eq!(parsed["finish_reason"], "stop");
    assert!(parsed["latency_ms"].as_u64().is_some());
    assert!(parsed["usage"].is_object());
    assert_eq!(parsed["usage"]["input_tokens"], 10);
    assert_eq!(parsed["usage"]["output_tokens"], 8);
    assert_eq!(parsed["usage"]["total_tokens"], 18);

    mock.assert();
}

#[test]
fn test_llm_streaming_text_mode() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(mock_stream_chunks())
        .create();

    let url = server.url();

    aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args(["llm", "-m", "gpt-4o", "-p", "hello", "-u", &url, "--stream"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello!"));

    mock.assert();
}

#[test]
fn test_llm_streaming_json_mode() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(mock_stream_chunks())
        .create();

    let url = server.url();

    let result = aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args([
            "llm", "-m", "gpt-4o", "-p", "hello", "-u", &url, "--stream", "--format", "json",
        ])
        .assert()
        .success();

    let output = String::from_utf8(result.get_output().stdout.clone()).unwrap();
    let lines: Vec<&str> = output.trim().lines().filter(|l| !l.is_empty()).collect();

    assert!(
        lines.len() >= 2,
        "Expected at least 2 NDJSON lines, got: {:?}",
        lines
    );

    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["schema_version"], "1.0");
    assert_eq!(first["type"], "delta");
    assert_eq!(first["seq"], 1);

    let last: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert_eq!(last["type"], "done");

    let seqs: Vec<u64> = lines
        .iter()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["seq"]
                .as_u64()
                .unwrap()
        })
        .collect();
    let expected_seq: Vec<u64> = (1..=seqs.len() as u64).collect();
    assert_eq!(seqs, expected_seq, "seq should be monotonic and gap-free");

    mock.assert();
}

#[test]
fn test_llm_auth_missing() {
    aikit()
        .env("OPENAI_API_KEY", "")
        .env("AIKIT_API_KEY", "")
        .args([
            "llm",
            "-m",
            "gpt-4o",
            "-p",
            "hello",
            "-u",
            "http://127.0.0.1:1/v1",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("E_LLM_AUTH_MISSING"));
}

#[test]
fn test_llm_auth_missing_json_mode() {
    let result = aikit()
        .env("OPENAI_API_KEY", "")
        .env("AIKIT_API_KEY", "")
        .args([
            "llm",
            "-m",
            "gpt-4o",
            "-p",
            "hello",
            "-u",
            "http://127.0.0.1:1/v1",
            "--format",
            "json",
        ])
        .assert()
        .failure();

    let output = String::from_utf8(result.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(parsed["type"], "error");
    assert_eq!(parsed["code"], "E_LLM_AUTH_MISSING");
}

#[test]
fn test_llm_prompt_file() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&mock_openai_response()).unwrap())
        .create();

    let url = server.url();
    let dir = tempfile::tempdir().unwrap();
    let prompt_file = dir.path().join("prompt.txt");
    std::fs::write(&prompt_file, "hello from file").unwrap();

    aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args([
            "llm",
            "-m",
            "gpt-4o",
            "--prompt-file",
            prompt_file.to_str().unwrap(),
            "-u",
            &url,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello! How can I help you?"));

    mock.assert();
}

#[test]
fn test_llm_prompt_file_not_found() {
    let server = mockito::Server::new();
    let url = server.url();

    aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args([
            "llm",
            "-m",
            "gpt-4o",
            "--prompt-file",
            "/nonexistent/path/prompt.txt",
            "-u",
            &url,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("E_LLM_PROMPT_FILE_READ"));
}

#[test]
fn test_llm_output_file() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&mock_openai_response()).unwrap())
        .create();

    let url = server.url();
    let dir = tempfile::tempdir().unwrap();
    let out_file = dir.path().join("output").join("response.txt");

    aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args([
            "llm",
            "-m",
            "gpt-4o",
            "-p",
            "hello",
            "-u",
            &url,
            "--out",
            out_file.to_str().unwrap(),
        ])
        .assert()
        .success();

    let content = std::fs::read_to_string(&out_file).unwrap();
    assert!(content.contains("Hello! How can I help you?"));

    mock.assert();
}

#[test]
fn test_llm_response_error() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"message":"Invalid API key"}}"#)
        .create();

    let url = server.url();

    aikit()
        .env("OPENAI_API_KEY", "bad-key")
        .args(["llm", "-m", "gpt-4o", "-p", "hello", "-u", &url])
        .assert()
        .failure()
        .stderr(predicate::str::contains("E_LLM_RESPONSE_ERROR"))
        .stderr(predicate::str::contains("401"));

    mock.assert();
}

#[test]
fn test_llm_request_failed() {
    aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args([
            "llm",
            "-m",
            "gpt-4o",
            "-p",
            "hello",
            "-u",
            "http://127.0.0.1:1/v1",
            "--connect-timeout",
            "1",
            "--timeout",
            "1",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("E_LLM"));
}

#[test]
fn test_llm_system_prompt() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .match_body(mockito::Matcher::PartialJsonString(
            r#"{"messages":[{"role":"system","content":"You are helpful"}]}"#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&mock_openai_response()).unwrap())
        .create();

    let url = server.url();

    aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args([
            "llm",
            "-m",
            "gpt-4o",
            "--system",
            "You are helpful",
            "-p",
            "hello",
            "-u",
            &url,
        ])
        .assert()
        .success();

    mock.assert();
}

#[test]
fn test_llm_url_override() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&mock_openai_response()).unwrap())
        .create();

    let url = server.url();

    aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args(["llm", "-m", "my-model", "-p", "hello", "-u", &url])
        .assert()
        .success();

    mock.assert();
}

#[test]
fn test_llm_usage_stderr() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&mock_openai_response()).unwrap())
        .create();

    let url = server.url();

    let result = aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args([
            "llm", "-m", "gpt-4o", "-p", "hello", "-u", &url, "--usage", "stderr",
        ])
        .assert()
        .success();

    let stderr = String::from_utf8(result.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("prompt_tokens=10"));
    assert!(stderr.contains("completion_tokens=8"));

    mock.assert();
}

#[test]
fn test_llm_usage_none() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&mock_openai_response()).unwrap())
        .create();

    let url = server.url();

    let result = aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args([
            "llm", "-m", "gpt-4o", "-p", "hello", "-u", &url, "--usage", "none",
        ])
        .assert()
        .success();

    let stderr = String::from_utf8(result.get_output().stderr.clone()).unwrap();
    assert!(!stderr.contains("prompt_tokens"));

    mock.assert();
}

#[test]
fn test_llm_quiet_flag() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&mock_openai_response()).unwrap())
        .create();

    let url = server.url();

    let result = aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args([
            "llm", "-m", "gpt-4o", "-p", "hello", "-u", &url, "--quiet", "--usage", "stderr",
        ])
        .assert()
        .success();

    let stderr = String::from_utf8(result.get_output().stderr.clone()).unwrap();
    assert!(
        !stderr.contains("prompt_tokens"),
        "quiet should suppress usage on stderr"
    );

    mock.assert();
}

#[test]
fn test_llm_prompt_group_conflict() {
    aikit()
        .args(["llm", "-m", "gpt-4o", "-p", "a", "--prompt-file", "b.txt"])
        .assert()
        .failure();
}

#[test]
fn test_llm_prompt_missing() {
    aikit().args(["llm", "-m", "gpt-4o"]).assert().failure();
}

#[test]
fn test_llm_custom_api_key_env() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&mock_openai_response()).unwrap())
        .create();

    let url = server.url();

    aikit()
        .env("MY_CUSTOM_KEY", "custom-key-123")
        .env("OPENAI_API_KEY", "")
        .env("AIKIT_API_KEY", "")
        .args([
            "llm",
            "-m",
            "gpt-4o",
            "-p",
            "hello",
            "-u",
            &url,
            "--api-key-env",
            "MY_CUSTOM_KEY",
        ])
        .assert()
        .success();

    mock.assert();
}

#[test]
fn test_llm_streaming_json_schema_version() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(mock_stream_chunks())
        .create();

    let url = server.url();

    let result = aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args([
            "llm", "-m", "gpt-4o", "-p", "hello", "-u", &url, "--stream", "--format", "json",
        ])
        .assert()
        .success();

    let output = String::from_utf8(result.get_output().stdout.clone()).unwrap();
    for line in output.trim().lines().filter(|l| !l.is_empty()) {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(
            parsed["schema_version"], "1.0",
            "Every NDJSON line must have schema_version 1.0"
        );
    }

    mock.assert();
}

#[test]
fn test_llm_backward_compat_existing_commands() {
    aikit().args(["check"]).assert().success();
    aikit().args(["--version"]).assert().success();
}

#[test]
fn test_llm_stdin_prompt() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&mock_openai_response()).unwrap())
        .create();

    let url = server.url();

    aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args(["llm", "-m", "gpt-4o", "--prompt-file", "-", "-u", &url])
        .write_stdin("hello from stdin")
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello! How can I help you?"));

    mock.assert();
}

#[test]
fn test_llm_stream_protocol_error() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(mock_malformed_stream_chunks())
        .create();

    let url = server.url();

    aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args(["llm", "-m", "gpt-4o", "-p", "hello", "-u", &url, "--stream"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("E_LLM_STREAM_PROTOCOL"));

    mock.assert();
}

#[test]
fn test_llm_stream_json_quiet_error_on_stdout() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(mock_malformed_stream_chunks())
        .create();

    let url = server.url();

    let result = aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args([
            "llm", "-m", "gpt-4o", "-p", "hello", "-u", &url, "--stream", "--format", "json",
            "--quiet",
        ])
        .assert()
        .failure();

    let output = String::from_utf8(result.get_output().stdout.clone()).unwrap();
    let lines: Vec<&str> = output.trim().lines().filter(|l| !l.is_empty()).collect();

    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["type"], "delta");

    let has_error = lines.iter().any(|l| {
        serde_json::from_str::<serde_json::Value>(l)
            .map(|v| v["type"] == "error" && v["code"] == "E_LLM_STREAM_PROTOCOL")
            .unwrap_or(false)
    });
    assert!(
        has_error,
        "JSON error event must appear on stdout even with --quiet"
    );

    mock.assert();
}

#[test]
fn test_llm_timeout() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    std::thread::spawn(move || {
        if let Ok((_stream, _)) = listener.accept() {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    });

    aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args([
            "llm",
            "-m",
            "gpt-4o",
            "-p",
            "hello",
            "-u",
            &format!("http://127.0.0.1:{}/v1", port),
            "--timeout",
            "1",
            "--connect-timeout",
            "5",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("E_LLM_TIMEOUT"));
}

#[test]
fn test_llm_output_write_error() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&mock_openai_response()).unwrap())
        .create();

    let url = server.url();

    let dir = tempfile::tempdir().unwrap();
    let blocking_file = dir.path().join("blocking");
    std::fs::write(&blocking_file, "content").unwrap();
    let impossible_path = blocking_file.join("sub").join("output.txt");

    aikit()
        .env("OPENAI_API_KEY", "test-key-123")
        .args([
            "llm",
            "-m",
            "gpt-4o",
            "-p",
            "hello",
            "-u",
            &url,
            "--out",
            impossible_path.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("E_LLM_OUTPUT_WRITE"));

    mock.assert();
}
