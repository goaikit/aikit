use crate::llm::types::{LlmError, LlmStreamEvent, LlmUsage, OpenAiSseChunk};

/// Parse an SSE data line and return the JSON data portion.
/// Returns `None` if the line is not a data line or is empty.
/// Returns `Some("[DONE]")` for the terminal signal.
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

/// Parse a single SSE chunk JSON string into stream events.
///
/// Returns a vector of events produced by this chunk.
pub fn parse_sse_chunk(data: &str, line_num: u64) -> Result<Vec<LlmStreamEvent>, LlmError> {
    let chunk: OpenAiSseChunk =
        serde_json::from_str(data).map_err(|e| LlmError::StreamProtocol {
            line: line_num,
            detail: format!("JSON parse error: {}", e),
        })?;

    let mut events = Vec::new();

    // Extract usage if present
    if let Some(usage) = chunk.usage {
        events.push(LlmStreamEvent::UsageUpdate {
            usage: LlmUsage {
                input_tokens: usage.prompt_tokens.unwrap_or(0),
                output_tokens: usage.completion_tokens.unwrap_or(0),
                total_tokens: usage.total_tokens,
            },
        });
    }

    if let Some(choices) = chunk.choices {
        for choice in choices {
            if let Some(finish_reason) = &choice.finish_reason {
                if !finish_reason.is_empty() {
                    events.push(LlmStreamEvent::Completed {
                        finish_reason: finish_reason.clone(),
                        usage: None,
                    });
                }
            }

            if let Some(delta) = choice.delta {
                if let Some(content) = delta.content {
                    if !content.is_empty() {
                        events.push(LlmStreamEvent::TextDelta { content });
                    }
                }
                if let Some(tool_calls) = delta.tool_calls {
                    for tc in tool_calls {
                        if let Some(func) = tc.function {
                            events.push(LlmStreamEvent::ToolCallDelta {
                                id: tc.id.unwrap_or_default(),
                                function_name: func.name.unwrap_or_default(),
                                arguments_delta: func.arguments.unwrap_or_default(),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(events)
}

/// Parse a complete SSE response body into a sequence of stream events.
pub fn parse_sse_body(body: &str) -> Result<Vec<LlmStreamEvent>, LlmError> {
    let mut events = Vec::new();
    let mut line_num: u64 = 0;

    for line in body.lines() {
        line_num += 1;
        if let Some(data) = parse_sse_line(line) {
            if data == "[DONE]" {
                break;
            }
            let chunk_events = parse_sse_chunk(data, line_num)?;
            events.extend(chunk_events);
        }
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

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
