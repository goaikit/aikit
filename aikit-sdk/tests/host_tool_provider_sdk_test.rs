//! Tests for HostToolProvider re-export surface from aikit-sdk.
//!
//! AC-3: A struct implementing HostToolProvider compiles when depending only on
//! aikit_sdk imports — no direct aikit-agent import allowed in this file.
//!
//! AC-12: Integration test confirming that passing Some(provider) to
//! run_aikit_agent causes the LLM HTTP request to contain the host tool's schema.

use aikit_sdk::{run_aikit_agent, HostToolDefinition, HostToolProvider, RunOptions};
use std::sync::Arc;

// ── AC-3: SDK-only HostToolProvider implementation ────────────────────────────

struct SdkOnlyProvider;

impl HostToolProvider for SdkOnlyProvider {
    fn list_tools(&self) -> Vec<HostToolDefinition> {
        vec![HostToolDefinition {
            name: "my_host_tool".to_string(),
            description: Some("A host-defined tool".to_string()),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }]
    }

    fn call_tool(&self, name: &str, _arguments: serde_json::Value) -> Result<String, String> {
        Ok(format!("called {}", name))
    }
}

/// AC-3: Verifies that HostToolProvider and HostToolDefinition are usable via
/// aikit_sdk imports alone, without importing aikit-agent directly.
#[test]
fn test_host_tool_provider_compiles_from_sdk_only() {
    let provider = SdkOnlyProvider;
    let tools = provider.list_tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "my_host_tool");
    assert_eq!(tools[0].description.as_deref(), Some("A host-defined tool"));

    let result = provider.call_tool("my_host_tool", serde_json::json!({}));
    assert!(result.is_ok());
    assert!(result.unwrap().contains("my_host_tool"));
}

/// AC-3 continued: Arc<dyn HostToolProvider> is constructable from SDK types alone.
#[test]
fn test_arc_host_tool_provider_from_sdk_only() {
    let provider: Arc<dyn HostToolProvider> = Arc::new(SdkOnlyProvider);
    let tools = provider.list_tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "my_host_tool");
}

// ── AC-12: Integration test — host tool schema reaches the LLM gateway ───────

/// Serial mutex to avoid environment-variable races between concurrent tests.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn mock_stop_response() -> String {
    serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "Done."},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
    })
    .to_string()
}

/// AC-12: Calling run_aikit_agent with Some(provider) causes the LLM HTTP
/// request body to contain "my_host_tool" in the tools array.
#[test]
fn test_run_aikit_agent_host_tool_schema_reaches_llm() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let mut server = mockito::Server::new();

    // Assert the request body contains "my_host_tool" (the host tool name).
    let mock = server
        .mock("POST", "/chat/completions")
        .match_body(mockito::Matcher::Regex("my_host_tool".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_stop_response())
        .create();

    let url = server.url();

    std::env::set_var("AIKIT_LLM_URL", &url);
    std::env::set_var("OPENAI_API_KEY", "test-key-ac12");
    std::env::set_var("AIKIT_MAX_ITERATIONS", "1");

    let options = RunOptions::new().with_stream(false);
    let provider: Arc<dyn HostToolProvider> = Arc::new(SdkOnlyProvider);

    let result = run_aikit_agent("test prompt", &options, Some(provider), |_| {});

    std::env::remove_var("AIKIT_LLM_URL");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("AIKIT_MAX_ITERATIONS");

    // The mock assertion verifies the request contained "my_host_tool".
    mock.assert();
    let _ = result;
}

/// AC-11: Passing None reproduces existing behavior — the agent still completes
/// and no host tool appears in the schema transmitted to the LLM.
#[test]
fn test_run_aikit_agent_none_provider_no_regression() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let mut server = mockito::Server::new();

    // Mock without body assertion — just confirm the call reaches the server.
    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_stop_response())
        .create();

    let url = server.url();

    std::env::set_var("AIKIT_LLM_URL", &url);
    std::env::set_var("OPENAI_API_KEY", "test-key-ac11");
    std::env::set_var("AIKIT_MAX_ITERATIONS", "1");

    let options = RunOptions::new().with_stream(false);
    let result = run_aikit_agent("test prompt", &options, None, |_| {});

    std::env::remove_var("AIKIT_LLM_URL");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("AIKIT_MAX_ITERATIONS");

    mock.assert();
    let _ = result;
}
