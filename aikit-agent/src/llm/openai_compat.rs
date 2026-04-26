use std::time::Duration;

use serde::Serialize;

use crate::llm::gateway::LlmGateway;
use crate::llm::stream::parse_sse_body;
use crate::llm::types::{
    LlmError, LlmMessage, LlmRequest, LlmResponse, LlmStreamEvent, LlmStreamHandle, LlmUsage,
    OpenAiResponse, OpenAiToolCall, ToolCall, ToolCallFunction, ToolDefinition,
};

/// Resolves API key from environment variables.
///
/// Resolution order: custom env var → OPENAI_API_KEY → AIKIT_API_KEY.
pub fn resolve_api_key(custom_env: Option<&str>) -> Result<String, LlmError> {
    let mut checked = Vec::new();

    if let Some(env_name) = custom_env {
        checked.push(env_name.to_string());
        if let Ok(val) = std::env::var(env_name) {
            if !val.is_empty() {
                return Ok(val);
            }
        }
    }

    checked.push("OPENAI_API_KEY".to_string());
    if let Ok(val) = std::env::var("OPENAI_API_KEY") {
        if !val.is_empty() {
            return Ok(val);
        }
    }

    checked.push("AIKIT_API_KEY".to_string());
    if let Ok(val) = std::env::var("AIKIT_API_KEY") {
        if !val.is_empty() {
            return Ok(val);
        }
    }

    Err(LlmError::NoApiKey {
        checked: checked.join(", "),
    })
}

fn build_client(timeout_secs: u64, connect_timeout_secs: u64) -> Result<reqwest::Client, LlmError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .connect_timeout(Duration::from_secs(connect_timeout_secs))
        .build()
        .map_err(|e| LlmError::RequestFailed {
            message: format!("failed to build HTTP client: {}", e),
        })
}

fn block_on_async<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");
    rt.block_on(future)
}

#[derive(Serialize)]
struct OpenAiChatRequest<'a> {
    model: &'a str,
    messages: &'a [LlmMessage],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<&'a ToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

fn convert_tool_calls(calls: &[OpenAiToolCall]) -> Vec<ToolCall> {
    calls
        .iter()
        .map(|tc| ToolCall {
            id: tc.id.clone(),
            call_type: tc.call_type.clone(),
            function: ToolCallFunction {
                name: tc.function.name.clone(),
                arguments: tc.function.arguments.clone(),
            },
        })
        .collect()
}

async fn send_complete(
    client: &reqwest::Client,
    base_url: &str,
    req: &LlmRequest,
) -> Result<LlmResponse, LlmError> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = OpenAiChatRequest {
        model: &req.model,
        messages: &req.messages,
        tools: req.tools.iter().collect(),
        temperature: req.temperature,
        top_p: req.top_p,
        max_tokens: req.max_tokens,
        stream: false,
        stream_options: None,
    };

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", req.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| LlmError::RequestFailed {
            message: e.to_string(),
        })?;

    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        return Err(LlmError::ErrorResponse {
            status: status.as_u16(),
            url,
            body: body_text,
        });
    }

    let resp: OpenAiResponse = response.json().await.map_err(|e| LlmError::RequestFailed {
        message: format!("failed to parse response: {}", e),
    })?;

    let first = resp.choices.into_iter().next();
    let content = first
        .as_ref()
        .and_then(|c| c.message.as_ref())
        .and_then(|m| m.content.clone());
    let tool_calls = first
        .as_ref()
        .and_then(|c| c.message.as_ref())
        .and_then(|m| m.tool_calls.as_ref())
        .map(|tc| convert_tool_calls(tc))
        .unwrap_or_default();
    let finish_reason = first.and_then(|c| c.finish_reason);
    let usage = resp.usage.map(|u| LlmUsage {
        input_tokens: u.prompt_tokens.unwrap_or(0),
        output_tokens: u.completion_tokens.unwrap_or(0),
        total_tokens: u.total_tokens,
    });

    Ok(LlmResponse {
        content,
        tool_calls,
        finish_reason,
        usage,
    })
}

async fn send_stream(
    client: &reqwest::Client,
    base_url: &str,
    req: &LlmRequest,
) -> Result<Vec<LlmStreamEvent>, LlmError> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = OpenAiChatRequest {
        model: &req.model,
        messages: &req.messages,
        tools: req.tools.iter().collect(),
        temperature: req.temperature,
        top_p: req.top_p,
        max_tokens: req.max_tokens,
        stream: true,
        stream_options: Some(StreamOptions {
            include_usage: true,
        }),
    };

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", req.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| LlmError::RequestFailed {
            message: e.to_string(),
        })?;

    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        return Err(LlmError::ErrorResponse {
            status: status.as_u16(),
            url,
            body: body_text,
        });
    }

    let text = response.text().await.map_err(|e| LlmError::RequestFailed {
        message: format!("failed to read stream body: {}", e),
    })?;

    parse_sse_body(&text)
}

pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    timeout_secs: u64,
}

impl OpenAiCompatProvider {
    pub fn new(timeout_secs: u64, connect_timeout_secs: u64) -> Result<Self, LlmError> {
        let client = build_client(timeout_secs, connect_timeout_secs)?;
        Ok(Self {
            client,
            timeout_secs,
        })
    }
}

impl LlmGateway for OpenAiCompatProvider {
    fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let client = &self.client;
        let base_url = req.base_url.clone();
        block_on_async(send_complete(client, &base_url, &req))
    }

    fn stream(&self, req: LlmRequest) -> Result<LlmStreamHandle, LlmError> {
        let client = &self.client;
        let base_url = req.base_url.clone();
        let events = block_on_async(send_stream(client, &base_url, &req))?;
        Ok(LlmStreamHandle::new(events.into_iter().map(Ok).collect()))
    }
}

impl std::fmt::Debug for OpenAiCompatProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OpenAiCompatProvider {{ timeout_secs: {} }}",
            self.timeout_secs
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_api_key_missing() {
        let _guard = crate::test_support::env_lock();
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("AIKIT_API_KEY");
        let err = resolve_api_key(None).unwrap_err();
        match err {
            LlmError::NoApiKey { checked } => {
                assert!(checked.contains("OPENAI_API_KEY"));
                assert!(checked.contains("AIKIT_API_KEY"));
            }
            _ => panic!("expected NoApiKey, got {:?}", err),
        }
    }

    #[test]
    fn test_resolve_api_key_from_openai_env() {
        let _guard = crate::test_support::env_lock();
        std::env::remove_var("AIKIT_API_KEY");
        std::env::set_var("OPENAI_API_KEY", "test-key-openai");
        let key = resolve_api_key(None).unwrap();
        assert_eq!(key, "test-key-openai");
        std::env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn test_resolve_api_key_custom_env_takes_precedence() {
        let _guard = crate::test_support::env_lock();
        std::env::set_var("MY_CUSTOM_KEY", "custom-value");
        std::env::set_var("OPENAI_API_KEY", "openai-value");
        let key = resolve_api_key(Some("MY_CUSTOM_KEY")).unwrap();
        assert_eq!(key, "custom-value");
        std::env::remove_var("MY_CUSTOM_KEY");
        std::env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn test_resolve_api_key_custom_env_missing() {
        let _guard = crate::test_support::env_lock();
        std::env::remove_var("MY_MISSING_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("AIKIT_API_KEY");
        let err = resolve_api_key(Some("MY_MISSING_KEY")).unwrap_err();
        match err {
            LlmError::NoApiKey { checked } => {
                assert!(checked.contains("MY_MISSING_KEY"));
                assert!(checked.contains("OPENAI_API_KEY"));
                assert!(checked.contains("AIKIT_API_KEY"));
            }
            _ => panic!("expected NoApiKey"),
        }
    }
}
