//! The model types. They live in [`aikit_agent_core::llm::types`], shared
//! with the on-device hosts; this module keeps their `aikit_agent` path.

pub use aikit_agent_core::llm::types::*;

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
