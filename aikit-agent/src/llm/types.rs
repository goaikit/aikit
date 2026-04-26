use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("E_AIKIT_NO_API_KEY: API key not found (checked: {checked})")]
    NoApiKey { checked: String },

    #[error("E_AIKIT_LLM_REQUEST_FAILED: {message}")]
    RequestFailed { message: String },

    #[error("E_AIKIT_LLM_ERROR_RESPONSE: HTTP {status} from {url}: {body}")]
    ErrorResponse {
        status: u16,
        url: String,
        body: String,
    },

    #[error("E_AIKIT_STREAM_PROTOCOL: invalid stream event at line {line}: {detail}")]
    StreamProtocol { line: u64, detail: String },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LlmMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<MessageToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MessageToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: MessageToolCallFunction,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MessageToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FunctionDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum ToolChoice {
    Mode(String),
}

impl ToolChoice {
    pub fn auto() -> Self {
        ToolChoice::Mode("auto".to_string())
    }

    pub fn none() -> Self {
        ToolChoice::Mode("none".to_string())
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub call_type: Option<String>,
    pub function: ToolCallFunction,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct LlmRequest {
    pub model: String,
    pub base_url: String,
    pub api_key: String,
    pub messages: Vec<LlmMessage>,
    pub tools: Vec<ToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    pub stream: bool,
}

#[derive(Clone, Debug)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: Option<String>,
    pub usage: Option<LlmUsage>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LlmUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum LlmStreamEvent {
    TextDelta {
        content: String,
    },
    ToolCallDelta {
        id: String,
        function_name: String,
        arguments_delta: String,
    },
    UsageUpdate {
        usage: LlmUsage,
    },
    Completed {
        finish_reason: String,
        usage: Option<LlmUsage>,
    },
    ProviderError {
        code: String,
        message: String,
    },
}

pub struct LlmStreamHandle {
    events: std::vec::IntoIter<Result<LlmStreamEvent, LlmError>>,
}

impl LlmStreamHandle {
    pub fn new(events: Vec<Result<LlmStreamEvent, LlmError>>) -> Self {
        Self {
            events: events.into_iter(),
        }
    }
}

impl Iterator for LlmStreamHandle {
    type Item = Result<LlmStreamEvent, LlmError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.events.next()
    }
}

// Internal OpenAI API response types for deserialization
#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiResponse {
    #[allow(dead_code)]
    pub id: Option<String>,
    #[allow(dead_code)]
    pub model: Option<String>,
    pub choices: Vec<OpenAiChoice>,
    pub usage: Option<OpenAiUsage>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiChoice {
    #[allow(dead_code)]
    pub index: Option<u64>,
    pub message: Option<OpenAiMessage>,
    pub delta: Option<OpenAiDelta>,
    pub finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiMessage {
    pub content: Option<String>,
    #[allow(dead_code)]
    pub role: Option<String>,
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiDelta {
    pub content: Option<String>,
    #[allow(dead_code)]
    pub role: Option<String>,
    pub tool_calls: Option<Vec<OpenAiToolCallDelta>>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: Option<String>,
    pub function: OpenAiFunctionCall,
}

#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiToolCallDelta {
    pub id: Option<String>,
    pub function: Option<OpenAiFunctionCallDelta>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiFunctionCallDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiSseChunk {
    pub choices: Option<Vec<OpenAiChoice>>,
    pub usage: Option<OpenAiUsage>,
    #[allow(dead_code)]
    pub id: Option<String>,
    #[allow(dead_code)]
    pub model: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_message_assistant_tool_call_serializes_tool_calls() {
        let msg = LlmMessage {
            role: "assistant".to_string(),
            content: None,
            tool_calls: Some(vec![MessageToolCall {
                id: "call_abc".to_string(),
                call_type: "function".to_string(),
                function: MessageToolCallFunction {
                    name: "read_file".to_string(),
                    arguments: r#"{"path": "AGENTS.md"}"#.to_string(),
                },
            }]),
            tool_call_id: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains("\"tool_calls\""),
            "should contain tool_calls key"
        );
        assert!(
            !json.contains("\"tool_call_id\""),
            "should not contain tool_call_id key"
        );
        assert!(
            json.contains("\"type\":\"function\""),
            "should contain type field"
        );
    }

    #[test]
    fn test_llm_message_tool_result_serializes_tool_call_id() {
        let msg = LlmMessage {
            role: "tool".to_string(),
            content: Some("file contents".to_string()),
            tool_calls: None,
            tool_call_id: Some("call_abc".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains("\"tool_call_id\":\"call_abc\""),
            "should contain tool_call_id"
        );
        assert!(
            !json.contains("\"tool_calls\""),
            "should not contain tool_calls key"
        );
    }

    #[test]
    fn test_llm_message_user_serializes_content_only() {
        let msg = LlmMessage {
            role: "user".to_string(),
            content: Some("hello".to_string()),
            tool_calls: None,
            tool_call_id: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains("\"content\":\"hello\""),
            "should contain content"
        );
        assert!(
            !json.contains("\"tool_calls\""),
            "should not contain tool_calls"
        );
        assert!(
            !json.contains("\"tool_call_id\""),
            "should not contain tool_call_id"
        );
    }
}
