use crate::llm::gateway::LlmGateway;
use crate::llm::types::{
    LlmError, LlmRequest, LlmResponse, LlmStreamEvent, LlmStreamHandle, ToolCall,
};

/// A mock LLM gateway for testing.
///
/// Responses are pre-configured at construction time. Each call to `complete()`
/// or `stream()` consumes the next response in the queue. When the queue is
/// exhausted, subsequent calls return a default empty response.
pub struct MockGateway {
    responses: std::sync::Mutex<std::collections::VecDeque<MockResponse>>,
}

/// A pre-configured response for the mock gateway.
#[derive(Clone, Debug)]
pub struct MockResponse {
    pub content: Option<String>,
    pub finish_reason: String,
    pub stream_events: Vec<LlmStreamEvent>,
    pub error: Option<LlmError>,
    pub tool_calls: Vec<ToolCall>,
}

impl MockResponse {
    /// Create a simple text completion response.
    pub fn text(content: impl Into<String>) -> Self {
        let content = content.into();
        Self {
            stream_events: vec![
                LlmStreamEvent::TextDelta {
                    content: content.clone(),
                },
                LlmStreamEvent::Completed {
                    finish_reason: "stop".to_string(),
                    usage: None,
                },
            ],
            content: Some(content),
            finish_reason: "stop".to_string(),
            error: None,
            tool_calls: vec![],
        }
    }

    /// Create a response that simulates an LLM error.
    pub fn error(err: LlmError) -> Self {
        Self {
            content: None,
            finish_reason: "error".to_string(),
            stream_events: vec![],
            error: Some(err),
            tool_calls: vec![],
        }
    }

    pub fn tool_call(
        call_id: impl Into<String>,
        name: impl Into<String>,
        args: impl Into<String>,
    ) -> Self {
        let id = call_id.into();
        let fn_name = name.into();
        let arguments = args.into();
        Self {
            content: None,
            finish_reason: "tool_calls".to_string(),
            stream_events: vec![
                LlmStreamEvent::ToolCallDelta {
                    id: id.clone(),
                    function_name: fn_name.clone(),
                    arguments_delta: arguments.clone(),
                },
                LlmStreamEvent::Completed {
                    finish_reason: "tool_calls".to_string(),
                    usage: None,
                },
            ],
            error: None,
            tool_calls: vec![ToolCall {
                id,
                call_type: Some("function".to_string()),
                function: crate::llm::types::ToolCallFunction {
                    name: fn_name,
                    arguments,
                },
            }],
        }
    }
}

impl Clone for LlmError {
    fn clone(&self) -> Self {
        match self {
            Self::NoApiKey { checked } => Self::NoApiKey {
                checked: checked.clone(),
            },
            Self::RequestFailed { message } => Self::RequestFailed {
                message: message.clone(),
            },
            Self::ErrorResponse { status, url, body } => Self::ErrorResponse {
                status: *status,
                url: url.clone(),
                body: body.clone(),
            },
            Self::StreamProtocol { line, detail } => Self::StreamProtocol {
                line: *line,
                detail: detail.clone(),
            },
        }
    }
}

impl MockGateway {
    pub fn new(responses: Vec<MockResponse>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses.into()),
        }
    }

    fn next_response(&self) -> MockResponse {
        let mut queue = self.responses.lock().unwrap();
        queue.pop_front().unwrap_or_else(|| MockResponse::text(""))
    }
}

impl LlmGateway for MockGateway {
    fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let resp = self.next_response();
        if let Some(err) = resp.error {
            return Err(err);
        }
        Ok(crate::llm::types::LlmResponse {
            content: resp.content,
            tool_calls: resp.tool_calls,
            finish_reason: Some(resp.finish_reason),
            usage: None,
        })
    }

    fn stream(&self, _req: LlmRequest) -> Result<LlmStreamHandle, LlmError> {
        let resp = self.next_response();
        if let Some(err) = resp.error {
            return Err(err);
        }
        let events: Vec<Result<LlmStreamEvent, LlmError>> =
            resp.stream_events.into_iter().map(Ok).collect();
        Ok(LlmStreamHandle::new(events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_gateway_complete() {
        let gw = MockGateway::new(vec![MockResponse::text("hello from mock")]);
        let req = LlmRequest {
            model: "test".to_string(),
            base_url: "http://localhost".to_string(),
            api_key: "fake".to_string(),
            messages: vec![],
            tools: vec![],
            tool_choice: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: false,
        };
        let resp = gw.complete(req).unwrap();
        assert_eq!(resp.content, Some("hello from mock".to_string()));
        assert_eq!(resp.finish_reason, Some("stop".to_string()));
    }

    #[test]
    fn test_mock_gateway_stream() {
        let gw = MockGateway::new(vec![MockResponse::text("streaming text")]);
        let req = LlmRequest {
            model: "test".to_string(),
            base_url: "http://localhost".to_string(),
            api_key: "fake".to_string(),
            messages: vec![],
            tools: vec![],
            tool_choice: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: true,
        };
        let handle = gw.stream(req).unwrap();
        let events: Vec<_> = handle.collect();
        assert!(events
            .iter()
            .any(|e| matches!(e, Ok(LlmStreamEvent::TextDelta { .. }))));
    }

    #[test]
    fn test_mock_gateway_error() {
        let gw = MockGateway::new(vec![MockResponse::error(LlmError::ErrorResponse {
            status: 401,
            url: "http://localhost".to_string(),
            body: "unauthorized".to_string(),
        })]);
        let req = LlmRequest {
            model: "test".to_string(),
            base_url: "http://localhost".to_string(),
            api_key: "bad-key".to_string(),
            messages: vec![],
            tools: vec![],
            tool_choice: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: false,
        };
        let err = gw.complete(req).unwrap_err();
        match err {
            LlmError::ErrorResponse { status, .. } => assert_eq!(status, 401),
            _ => panic!("expected ErrorResponse"),
        }
    }

    #[test]
    fn test_mock_gateway_exhausted_returns_empty() {
        let gw = MockGateway::new(vec![]);
        let req = LlmRequest {
            model: "test".to_string(),
            base_url: "http://localhost".to_string(),
            api_key: "fake".to_string(),
            messages: vec![],
            tools: vec![],
            tool_choice: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: false,
        };
        let resp = gw.complete(req).unwrap();
        assert_eq!(resp.content, Some("".to_string()));
    }
}
