//! The SSE parser. It lives in [`aikit_agent_core::llm::stream`], shared
//! with the on-device hosts; this module keeps its `aikit_agent` path.

pub use aikit_agent_core::llm::stream::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::{LlmError, LlmStreamEvent};

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
    fn test_parse_sse_chunk_text_delta() {
        let data = r#"{"choices":[{"delta":{"content":"hello"},"finish_reason":null}]}"#;
        let events = parse_sse_chunk(data, 1).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            LlmStreamEvent::TextDelta { content } => assert_eq!(content, "hello"),
            _ => panic!("expected TextDelta"),
        }
    }

    #[test]
    fn test_parse_sse_chunk_finish() {
        let data = r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#;
        let events = parse_sse_chunk(data, 1).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            LlmStreamEvent::Completed { finish_reason, .. } => {
                assert_eq!(finish_reason, "stop")
            }
            _ => panic!("expected Completed"),
        }
    }

    #[test]
    fn test_parse_sse_body_done_terminates() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\ndata: [DONE]\ndata: {\"choices\":[]}";
        let events = parse_sse_body(body).unwrap();
        // Only events before [DONE] should be included
        assert!(events
            .iter()
            .any(|e| matches!(e, LlmStreamEvent::TextDelta { .. })));
        // No events after [DONE]
        let delta_count = events
            .iter()
            .filter(|e| matches!(e, LlmStreamEvent::TextDelta { .. }))
            .count();
        assert_eq!(delta_count, 1);
    }

    #[test]
    fn test_parse_sse_chunk_invalid_json() {
        let result = parse_sse_chunk("not-json", 5);
        assert!(result.is_err());
        match result.unwrap_err() {
            LlmError::StreamProtocol { line, .. } => assert_eq!(line, 5),
            _ => panic!("expected StreamProtocol error"),
        }
    }
}
