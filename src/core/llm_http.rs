//! LLM HTTP client for OpenAI-compatible chat completions API
//!
//! Provides types, error handling, and HTTP functions for invoking
//! `/v1/chat/completions` endpoints via direct `reqwest` calls.

use std::io::Read;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LlmError {
    #[error("E_LLM_PROMPT_MISSING: neither --prompt nor --prompt-file provided")]
    PromptMissing,

    #[error("E_LLM_PROMPT_CONFLICT: both --prompt and --prompt-file provided")]
    PromptConflict,

    #[error("E_LLM_PROMPT_FILE_READ: failed to read prompt file '{path}': {reason}")]
    PromptFileRead { path: String, reason: String },

    #[error("E_LLM_AUTH_MISSING: API key not found (checked: {checked})")]
    AuthMissing { checked: String },

    #[error("E_LLM_REQUEST_FAILED: {message}")]
    RequestFailed { message: String },

    #[error("E_LLM_RESPONSE_ERROR: HTTP {status} from {url}: {body}")]
    ResponseError {
        status: u16,
        url: String,
        body: String,
    },

    #[error("E_LLM_OUTPUT_WRITE: failed to write output to '{path}': {reason}")]
    OutputWrite { path: String, reason: String },

    #[error("E_LLM_STREAM_PROTOCOL: invalid stream event at line {line}: {detail}")]
    #[allow(dead_code)]
    StreamProtocol { line: u64, detail: String },

    #[error("E_LLM_TIMEOUT: request timed out after {seconds}s")]
    Timeout { seconds: u64 },
}

#[derive(Serialize, Debug)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ChatResponse {
    pub id: Option<String>,
    pub model: Option<String>,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Choice {
    #[allow(dead_code)]
    pub index: Option<u64>,
    pub message: Option<ChoiceMessage>,
    pub delta: Option<ChoiceMessage>,
    pub finish_reason: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ChoiceMessage {
    pub content: Option<String>,
    #[allow(dead_code)]
    pub role: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Usage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Serialize, Debug)]
pub struct JsonEnvelope {
    pub schema_version: &'static str,
    pub request_id: Option<String>,
    pub base_url: String,
    pub latency_ms: u64,
    pub model: String,
    pub content: Option<String>,
    pub finish_reason: Option<String>,
    pub usage: Option<JsonUsage>,
}

#[derive(Serialize, Debug, Clone)]
pub struct JsonUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Serialize, Debug)]
pub struct StreamEvent {
    pub schema_version: &'static str,
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<JsonUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl StreamEvent {
    pub fn delta(seq: u64, content: Option<String>, finish_reason: Option<String>) -> Self {
        Self {
            schema_version: "1.0",
            event_type: "delta",
            seq,
            content,
            finish_reason,
            usage: None,
            code: None,
            message: None,
        }
    }

    pub fn usage_event(seq: u64, usage: JsonUsage) -> Self {
        Self {
            schema_version: "1.0",
            event_type: "usage",
            seq,
            content: None,
            finish_reason: None,
            usage: Some(usage),
            code: None,
            message: None,
        }
    }

    pub fn done(seq: u64) -> Self {
        Self {
            schema_version: "1.0",
            event_type: "done",
            seq,
            content: None,
            finish_reason: None,
            usage: None,
            code: None,
            message: None,
        }
    }

    pub fn error(seq: u64, code: String, message: String) -> Self {
        Self {
            schema_version: "1.0",
            event_type: "error",
            seq,
            content: None,
            finish_reason: None,
            usage: None,
            code: Some(code),
            message: Some(message),
        }
    }
}

pub fn resolve_api_key(custom_env: Option<&str>) -> Result<String, LlmError> {
    let mut checked_vars = Vec::new();

    if let Some(env_name) = custom_env {
        checked_vars.push(env_name.to_string());
        if let Ok(val) = std::env::var(env_name) {
            if !val.is_empty() {
                return Ok(val);
            }
        }
    }

    checked_vars.push("OPENAI_API_KEY".to_string());
    if let Ok(val) = std::env::var("OPENAI_API_KEY") {
        if !val.is_empty() {
            return Ok(val);
        }
    }

    checked_vars.push("AIKIT_API_KEY".to_string());
    if let Ok(val) = std::env::var("AIKIT_API_KEY") {
        if !val.is_empty() {
            return Ok(val);
        }
    }

    Err(LlmError::AuthMissing {
        checked: checked_vars.join(", "),
    })
}

pub fn resolve_prompt(prompt: Option<&str>, prompt_file: Option<&str>) -> Result<String, LlmError> {
    match (prompt, prompt_file) {
        (Some(_), Some(_)) => Err(LlmError::PromptConflict),
        (Some(p), None) => Ok(p.to_string()),
        (None, Some(path)) => {
            if path == "-" {
                let mut buffer = String::new();
                std::io::stdin().read_to_string(&mut buffer).map_err(|e| {
                    LlmError::PromptFileRead {
                        path: "-".to_string(),
                        reason: e.to_string(),
                    }
                })?;
                Ok(buffer)
            } else {
                std::fs::read_to_string(path).map_err(|e| LlmError::PromptFileRead {
                    path: path.to_string(),
                    reason: e.to_string(),
                })
            }
        }
        (None, None) => Err(LlmError::PromptMissing),
    }
}

pub fn build_chat_request(
    model: &str,
    user_prompt: &str,
    system_prompt: Option<&str>,
    stream: bool,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
    top_p: Option<f64>,
) -> ChatRequest {
    let mut messages = Vec::new();
    if let Some(sys) = system_prompt {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: sys.to_string(),
        });
    }
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: user_prompt.to_string(),
    });
    ChatRequest {
        model: model.to_string(),
        messages,
        stream: if stream { Some(true) } else { None },
        max_tokens,
        temperature,
        top_p,
    }
}

pub fn build_client(timeout: u64, connect_timeout: u64) -> Result<reqwest::Client, LlmError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .connect_timeout(Duration::from_secs(connect_timeout))
        .build()
        .map_err(|e| LlmError::RequestFailed {
            message: format!("failed to build HTTP client: {}", e),
        })
}

pub async fn send_chat(
    client: &reqwest::Client,
    base_url: &str,
    req: &ChatRequest,
    key: &str,
    timeout_secs: u64,
) -> Result<ChatResponse, LlmError> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", key))
        .header("Content-Type", "application/json")
        .json(req)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                LlmError::Timeout {
                    seconds: timeout_secs,
                }
            } else if e.is_connect() {
                LlmError::RequestFailed {
                    message: format!("connection error: {}", e),
                }
            } else {
                LlmError::RequestFailed {
                    message: e.to_string(),
                }
            }
        })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(LlmError::ResponseError {
            status: status.as_u16(),
            url,
            body,
        });
    }

    response
        .json::<ChatResponse>()
        .await
        .map_err(|e| LlmError::RequestFailed {
            message: format!("failed to parse response: {}", e),
        })
}

pub async fn send_chat_stream(
    client: &reqwest::Client,
    base_url: &str,
    req: &ChatRequest,
    key: &str,
    timeout_secs: u64,
) -> Result<reqwest::Response, LlmError> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", key))
        .header("Content-Type", "application/json")
        .json(req)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                LlmError::Timeout {
                    seconds: timeout_secs,
                }
            } else {
                LlmError::RequestFailed {
                    message: e.to_string(),
                }
            }
        })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(LlmError::ResponseError {
            status: status.as_u16(),
            url,
            body,
        });
    }

    Ok(response)
}

#[derive(Deserialize, Debug)]
pub struct SseChunk {
    pub choices: Option<Vec<Choice>>,
    pub usage: Option<Usage>,
    #[allow(dead_code)]
    pub id: Option<String>,
    #[allow(dead_code)]
    pub model: Option<String>,
}

pub fn parse_sse_line(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "data: [DONE]" {
        return Some("[DONE]");
    }
    if let Some(data) = trimmed.strip_prefix("data: ") {
        Some(data)
    } else if let Some(data) = trimmed.strip_prefix("data:") {
        Some(data.trim())
    } else {
        None
    }
}

pub fn extract_content_from_chunk(chunk: &SseChunk) -> (Option<String>, Option<String>) {
    if let Some(choices) = &chunk.choices {
        if let Some(first) = choices.first() {
            let content = first.delta.as_ref().and_then(|d| d.content.clone());
            let finish = first.finish_reason.clone();
            return (content, finish);
        }
    }
    (None, None)
}

pub fn extract_usage_from_chunk(chunk: &SseChunk) -> Option<Usage> {
    chunk.usage.clone()
}

pub fn render_text_response(response: &ChatResponse) -> String {
    response
        .choices
        .first()
        .and_then(|c| c.message.as_ref())
        .and_then(|m| m.content.clone())
        .unwrap_or_default()
}

pub fn build_json_envelope(
    response: &ChatResponse,
    base_url: &str,
    latency_ms: u64,
) -> JsonEnvelope {
    let content = response
        .choices
        .first()
        .and_then(|c| c.message.as_ref())
        .and_then(|m| m.content.clone());

    let finish_reason = response
        .choices
        .first()
        .and_then(|c| c.finish_reason.clone());

    let usage = response.usage.as_ref().map(|u| JsonUsage {
        input_tokens: u.prompt_tokens.unwrap_or(0),
        output_tokens: u.completion_tokens.unwrap_or(0),
        total_tokens: u.total_tokens.unwrap_or(0),
    });

    JsonEnvelope {
        schema_version: "1.0",
        request_id: response.id.clone(),
        base_url: base_url.to_string(),
        latency_ms,
        model: response.model.clone().unwrap_or_default(),
        content,
        finish_reason,
        usage,
    }
}

pub fn format_usage_stderr(usage: &Usage) -> String {
    format!(
        "Usage: prompt_tokens={}, completion_tokens={}, total_tokens={}",
        usage.prompt_tokens.unwrap_or(0),
        usage.completion_tokens.unwrap_or(0),
        usage.total_tokens.unwrap_or(0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_prompt_with_inline() {
        let result = resolve_prompt(Some("hello"), None).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_resolve_prompt_missing() {
        let err = resolve_prompt(None, None).unwrap_err();
        match err {
            LlmError::PromptMissing => {}
            _ => panic!("Expected PromptMissing, got {:?}", err),
        }
    }

    #[test]
    fn test_resolve_prompt_conflict() {
        let err = resolve_prompt(Some("a"), Some("b")).unwrap_err();
        match err {
            LlmError::PromptConflict => {}
            _ => panic!("Expected PromptConflict, got {:?}", err),
        }
    }

    #[test]
    fn test_resolve_prompt_file_not_found() {
        let err = resolve_prompt(None, Some("/nonexistent/path/file.txt")).unwrap_err();
        match err {
            LlmError::PromptFileRead { path, .. } => {
                assert_eq!(path, "/nonexistent/path/file.txt");
            }
            _ => panic!("Expected PromptFileRead, got {:?}", err),
        }
    }

    #[test]
    fn test_build_chat_request_no_system() {
        let req = build_chat_request("gpt-4o", "hello", None, false, None, None, None);
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert!(req.stream.is_none());
        assert!(req.max_tokens.is_none());
    }

    #[test]
    fn test_build_chat_request_with_system() {
        let req = build_chat_request(
            "gpt-4o",
            "hello",
            Some("You are helpful"),
            true,
            Some(100),
            Some(0.7),
            Some(0.9),
        );
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, "system");
        assert_eq!(req.messages[1].role, "user");
        assert_eq!(req.stream, Some(true));
        assert_eq!(req.max_tokens, Some(100));
        assert_eq!(req.temperature, Some(0.7));
        assert_eq!(req.top_p, Some(0.9));
    }

    #[test]
    fn test_resolve_api_key_missing() {
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("AIKIT_API_KEY");
        std::env::remove_var("MY_CUSTOM_KEY");
        let err = resolve_api_key(None).unwrap_err();
        match err {
            LlmError::AuthMissing { checked } => {
                assert!(checked.contains("OPENAI_API_KEY"));
                assert!(checked.contains("AIKIT_API_KEY"));
            }
            _ => panic!("Expected AuthMissing, got {:?}", err),
        }
    }

    #[test]
    fn test_resolve_api_key_custom_env() {
        std::env::remove_var("MY_CUSTOM_LLM_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("AIKIT_API_KEY");
        let err = resolve_api_key(Some("MY_CUSTOM_LLM_KEY")).unwrap_err();
        match err {
            LlmError::AuthMissing { checked } => {
                assert!(checked.contains("MY_CUSTOM_LLM_KEY"));
                assert!(checked.contains("OPENAI_API_KEY"));
                assert!(checked.contains("AIKIT_API_KEY"));
            }
            _ => panic!("Expected AuthMissing, got {:?}", err),
        }
    }

    #[test]
    fn test_parse_sse_line_data() {
        assert_eq!(
            parse_sse_line("data: {\"hello\":true}"),
            Some("{\"hello\":true}")
        );
    }

    #[test]
    fn test_parse_sse_line_done() {
        assert_eq!(parse_sse_line("data: [DONE]"), Some("[DONE]"));
    }

    #[test]
    fn test_parse_sse_line_empty() {
        assert_eq!(parse_sse_line(""), None);
    }

    #[test]
    fn test_parse_sse_line_whitespace() {
        assert_eq!(parse_sse_line("   "), None);
    }

    #[test]
    fn test_parse_sse_line_no_prefix() {
        assert_eq!(parse_sse_line("just text"), None);
    }

    #[test]
    fn test_render_text_response() {
        let response = ChatResponse {
            id: Some("chatcmpl-123".to_string()),
            model: Some("gpt-4o".to_string()),
            choices: vec![Choice {
                index: Some(0),
                message: Some(ChoiceMessage {
                    content: Some("Hello world".to_string()),
                    role: Some("assistant".to_string()),
                }),
                delta: None,
                finish_reason: Some("stop".to_string()),
            }],
            usage: None,
        };
        assert_eq!(render_text_response(&response), "Hello world");
    }

    #[test]
    fn test_build_json_envelope() {
        let response = ChatResponse {
            id: Some("chatcmpl-123".to_string()),
            model: Some("gpt-4o".to_string()),
            choices: vec![Choice {
                index: Some(0),
                message: Some(ChoiceMessage {
                    content: Some("Hello".to_string()),
                    role: Some("assistant".to_string()),
                }),
                delta: None,
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(Usage {
                prompt_tokens: Some(10),
                completion_tokens: Some(5),
                total_tokens: Some(15),
            }),
        };
        let envelope = build_json_envelope(&response, "https://api.openai.com/v1", 150);
        assert_eq!(envelope.schema_version, "1.0");
        assert_eq!(envelope.request_id, Some("chatcmpl-123".to_string()));
        assert_eq!(envelope.latency_ms, 150);
        assert_eq!(envelope.content, Some("Hello".to_string()));
        assert_eq!(envelope.finish_reason, Some("stop".to_string()));
        assert!(envelope.usage.is_some());
        let u = envelope.usage.unwrap();
        assert_eq!(u.input_tokens, 10);
        assert_eq!(u.output_tokens, 5);
        assert_eq!(u.total_tokens, 15);
    }

    #[test]
    fn test_stream_event_delta() {
        let event = StreamEvent::delta(1, Some("hello".to_string()), None);
        assert_eq!(event.schema_version, "1.0");
        assert_eq!(event.event_type, "delta");
        assert_eq!(event.seq, 1);
        assert_eq!(event.content, Some("hello".to_string()));
    }

    #[test]
    fn test_stream_event_done() {
        let event = StreamEvent::done(5);
        assert_eq!(event.event_type, "done");
        assert_eq!(event.seq, 5);
    }

    #[test]
    fn test_stream_event_error() {
        let event = StreamEvent::error(
            3,
            "E_LLM_STREAM_PROTOCOL".to_string(),
            "bad data".to_string(),
        );
        assert_eq!(event.event_type, "error");
        assert_eq!(event.code, Some("E_LLM_STREAM_PROTOCOL".to_string()));
    }

    #[test]
    fn test_stream_event_serialization() {
        let event = StreamEvent::delta(1, Some("hi".to_string()), None);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"delta\""));
        assert!(json.contains("\"schema_version\":\"1.0\""));
        assert!(json.contains("\"seq\":1"));
    }

    #[test]
    fn test_format_usage_stderr() {
        let usage = Usage {
            prompt_tokens: Some(10),
            completion_tokens: Some(20),
            total_tokens: Some(30),
        };
        let s = format_usage_stderr(&usage);
        assert_eq!(
            s,
            "Usage: prompt_tokens=10, completion_tokens=20, total_tokens=30"
        );
    }

    #[test]
    fn test_extract_content_from_chunk() {
        let chunk = SseChunk {
            choices: Some(vec![Choice {
                index: Some(0),
                message: None,
                delta: Some(ChoiceMessage {
                    content: Some("hello".to_string()),
                    role: Some("assistant".to_string()),
                }),
                finish_reason: None,
            }]),
            usage: None,
            id: None,
            model: None,
        };
        let (content, finish) = extract_content_from_chunk(&chunk);
        assert_eq!(content, Some("hello".to_string()));
        assert!(finish.is_none());
    }

    #[test]
    fn test_extract_usage_from_chunk() {
        let chunk = SseChunk {
            choices: None,
            usage: Some(Usage {
                prompt_tokens: Some(5),
                completion_tokens: Some(3),
                total_tokens: Some(8),
            }),
            id: None,
            model: None,
        };
        let usage = extract_usage_from_chunk(&chunk);
        assert!(usage.is_some());
        let u = usage.unwrap();
        assert_eq!(u.prompt_tokens, Some(5));
    }

    #[test]
    fn test_build_client() {
        let client = build_client(30, 5);
        assert!(client.is_ok());
    }
}
