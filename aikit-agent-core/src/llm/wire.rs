//! The OpenAI chat-completions wire shapes.
//!
//! Every host sends the same request body ([`ChatCompletionRequest`]) and reads
//! the same response shapes, so a browser host and the native driver put the
//! same bytes on the wire.

use serde::{Deserialize, Serialize};

use crate::llm::types::{
    LlmError, LlmMessage, LlmRequest, LlmResponse, LlmUsage, ToolCall, ToolCallFunction,
    ToolDefinition,
};

/// The body of a `POST {base_url}/chat/completions` request.
#[derive(Serialize, Debug)]
pub struct ChatCompletionRequest<'a> {
    pub model: &'a str,
    pub messages: &'a [LlmMessage],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<&'a ToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    // Always on the wire, `false` included: a judge (aikit-evals) asserts the
    // body it sent, and an absent field is not the same statement as `false`.
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
}

#[derive(Serialize, Debug)]
pub struct StreamOptions {
    pub include_usage: bool,
}

impl<'a> ChatCompletionRequest<'a> {
    /// The body for `req`. `stream` decides between a streamed answer (which
    /// also asks for usage in the last chunk) and a whole one; it is a
    /// parameter, not `req.stream`, because `LlmGateway::complete` and
    /// `LlmGateway::stream` pick the mode themselves.
    pub fn new(req: &'a LlmRequest, stream: bool) -> Self {
        Self::from_parts(
            &req.model,
            &req.messages,
            &req.tools,
            req.temperature,
            req.top_p,
            req.max_tokens,
            stream,
        )
    }

    pub(crate) fn from_parts(
        model: &'a str,
        messages: &'a [LlmMessage],
        tools: &'a [ToolDefinition],
        temperature: Option<f64>,
        top_p: Option<f64>,
        max_tokens: Option<u32>,
        stream: bool,
    ) -> Self {
        Self {
            model,
            messages,
            tools: tools.iter().collect(),
            temperature,
            top_p,
            max_tokens,
            stream,
            stream_options: stream.then_some(StreamOptions {
                include_usage: true,
            }),
        }
    }
}

/// A whole (non-streamed) chat-completions response.
#[derive(Deserialize, Debug)]
pub struct OpenAiResponse {
    pub id: Option<String>,
    pub model: Option<String>,
    pub choices: Vec<OpenAiChoice>,
    pub usage: Option<OpenAiUsage>,
}

#[derive(Deserialize, Debug)]
pub struct OpenAiChoice {
    pub index: Option<u64>,
    pub message: Option<OpenAiMessage>,
    pub delta: Option<OpenAiDelta>,
    pub finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct OpenAiMessage {
    pub content: Option<String>,
    pub role: Option<String>,
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Deserialize, Debug)]
pub struct OpenAiDelta {
    pub content: Option<String>,
    pub role: Option<String>,
    pub tool_calls: Option<Vec<OpenAiToolCallDelta>>,
}

#[derive(Deserialize, Debug)]
pub struct OpenAiToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: Option<String>,
    pub function: OpenAiFunctionCall,
}

#[derive(Deserialize, Debug)]
pub struct OpenAiFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Deserialize, Debug)]
pub struct OpenAiToolCallDelta {
    pub id: Option<String>,
    pub function: Option<OpenAiFunctionCallDelta>,
}

#[derive(Deserialize, Debug)]
pub struct OpenAiFunctionCallDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct OpenAiUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

impl From<OpenAiUsage> for LlmUsage {
    fn from(u: OpenAiUsage) -> Self {
        LlmUsage {
            input_tokens: u.prompt_tokens.unwrap_or(0),
            output_tokens: u.completion_tokens.unwrap_or(0),
            total_tokens: u.total_tokens,
        }
    }
}

/// One `data:` chunk of a streamed chat-completions response.
#[derive(Deserialize, Debug)]
pub struct OpenAiSseChunk {
    pub choices: Option<Vec<OpenAiChoice>>,
    pub usage: Option<OpenAiUsage>,
    pub id: Option<String>,
    pub model: Option<String>,
}

/// The tool calls of a whole response, in the loop's shape.
pub fn convert_tool_calls(calls: &[OpenAiToolCall]) -> Vec<ToolCall> {
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

impl From<OpenAiResponse> for LlmResponse {
    /// The first choice of a whole response; the others are ignored.
    fn from(resp: OpenAiResponse) -> Self {
        let first = resp.choices.into_iter().next();
        let message = first.as_ref().and_then(|c| c.message.as_ref());
        let content = message.and_then(|m| m.content.clone());
        let tool_calls = message
            .and_then(|m| m.tool_calls.as_ref())
            .map(|tc| convert_tool_calls(tc))
            .unwrap_or_default();
        let finish_reason = first.and_then(|c| c.finish_reason);
        LlmResponse {
            content,
            tool_calls,
            finish_reason,
            usage: resp.usage.map(LlmUsage::from),
            model: resp.model,
        }
    }
}

/// Parse the body of a whole (non-streamed) chat-completions response.
pub fn parse_completion_body(body: &str) -> Result<LlmResponse, LlmError> {
    let resp: OpenAiResponse = serde_json::from_str(body).map_err(|e| LlmError::RequestFailed {
        message: format!("failed to parse response: {e}"),
    })?;
    Ok(resp.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(stream: bool) -> LlmRequest {
        LlmRequest {
            model: "m".into(),
            base_url: "http://x".into(),
            api_key: "secret".into(),
            messages: vec![],
            tools: vec![],
            tool_choice: None,
            temperature: None,
            top_p: None,
            max_tokens: Some(5),
            stream,
        }
    }

    #[test]
    fn a_whole_body_says_stream_false_and_asks_no_usage() {
        let req = request(false);
        let json = serde_json::to_value(ChatCompletionRequest::new(&req, false)).unwrap();
        assert_eq!(json["stream"], false);
        assert!(json.get("stream_options").is_none());
        assert!(
            json.get("tools").is_none(),
            "an empty tool list is left out"
        );
        assert!(
            json.get("api_key").is_none(),
            "the key never goes in the body"
        );
        assert_eq!(json["max_tokens"], 5);
    }

    #[test]
    fn a_streamed_body_asks_for_usage() {
        let req = request(true);
        let json = serde_json::to_value(ChatCompletionRequest::new(&req, true)).unwrap();
        assert_eq!(json["stream"], true);
        assert_eq!(json["stream_options"]["include_usage"], true);
    }

    #[test]
    fn a_whole_response_becomes_an_llm_response() {
        let body = r#"{"model":"m-1","choices":[{"message":{"content":null,"tool_calls":[
            {"id":"c1","type":"function","function":{"name":"f","arguments":"{}"}}]},
            "finish_reason":"tool_calls"}],"usage":{"prompt_tokens":3,"completion_tokens":2}}"#;
        let resp = parse_completion_body(body).unwrap();
        assert_eq!(resp.model.as_deref(), Some("m-1"));
        assert_eq!(resp.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].function.name, "f");
        let usage = resp.usage.unwrap();
        assert_eq!((usage.input_tokens, usage.output_tokens), (3, 2));
        assert_eq!(usage.total_tokens, None);
    }

    #[test]
    fn a_body_that_is_not_json_is_a_request_failure() {
        let err = parse_completion_body("<html>").unwrap_err();
        assert!(matches!(err, LlmError::RequestFailed { .. }), "{err}");
    }
}
