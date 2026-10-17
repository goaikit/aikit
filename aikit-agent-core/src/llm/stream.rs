//! Server-sent events of a streamed chat-completions answer.
//!
//! [`SseParser`] takes the lines one at a time, as they arrive.
//! [`parse_sse_body`] parses a whole body with it.

use crate::llm::types::{LlmError, LlmStreamEvent};
use crate::llm::wire::OpenAiSseChunk;

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
            usage: usage.into(),
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

/// What one line fed to an [`SseParser`] produced.
#[derive(Debug, Clone)]
pub enum SseLine {
    /// Zero or more events: a data chunk, or a line that carries none
    /// (a comment, an `event:` field, a blank line).
    Events(Vec<LlmStreamEvent>),
    /// `data: [DONE]`: the stream is over. Later lines are ignored.
    Done,
}

/// An incremental SSE parser: feed it the lines of the body as they arrive.
///
/// Line numbers in [`LlmError::StreamProtocol`] count every line fed,
/// from 1, the same as [`parse_sse_body`] counts the lines of a whole body.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SseParser {
    line_num: u64,
    done: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `data: [DONE]` has been seen.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Parse one line. A trailing `\r` or `\n` is ignored, so a host can feed
    /// lines with or without their line ending.
    pub fn feed_line(&mut self, line: &str) -> Result<SseLine, LlmError> {
        if self.done {
            return Ok(SseLine::Done);
        }
        self.line_num += 1;
        match parse_sse_line(line) {
            None => Ok(SseLine::Events(Vec::new())),
            Some("[DONE]") => {
                self.done = true;
                Ok(SseLine::Done)
            }
            Some(data) => parse_sse_chunk(data, self.line_num).map(SseLine::Events),
        }
    }
}

/// Parse a complete SSE response body into a sequence of stream events.
pub fn parse_sse_body(body: &str) -> Result<Vec<LlmStreamEvent>, LlmError> {
    let mut parser = SseParser::new();
    let mut events = Vec::new();
    for line in body.lines() {
        match parser.feed_line(line)? {
            SseLine::Events(chunk_events) => events.extend(chunk_events),
            SseLine::Done => break,
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

    #[test]
    fn test_parser_takes_lines_one_at_a_time() {
        let mut p = SseParser::new();
        let first = p
            .feed_line("data: {\"choices\":[{\"delta\":{\"content\":\"he\"}}]}\r\n")
            .unwrap();
        assert!(
            matches!(&first, SseLine::Events(e) if matches!(&e[..], [LlmStreamEvent::TextDelta { content }] if content == "he"))
        );
        assert!(matches!(p.feed_line("").unwrap(), SseLine::Events(e) if e.is_empty()));
        assert!(matches!(p.feed_line(": keep-alive").unwrap(), SseLine::Events(e) if e.is_empty()));
        assert!(matches!(
            p.feed_line("data: [DONE]").unwrap(),
            SseLine::Done
        ));
        assert!(p.is_done());
        // After [DONE] even garbage is ignored.
        assert!(matches!(
            p.feed_line("data: not-json").unwrap(),
            SseLine::Done
        ));
    }

    #[test]
    fn test_parser_counts_lines_like_the_whole_body() {
        let body = "\n: comment\ndata: not-json\n";
        let whole = parse_sse_body(body).unwrap_err();
        let mut p = SseParser::new();
        let mut incremental = None;
        for line in body.lines() {
            if let Err(e) = p.feed_line(line) {
                incremental = Some(e);
                break;
            }
        }
        match (whole, incremental.unwrap()) {
            (
                LlmError::StreamProtocol { line: a, .. },
                LlmError::StreamProtocol { line: b, .. },
            ) => {
                assert_eq!(a, 3);
                assert_eq!(b, 3);
            }
            other => panic!("expected two StreamProtocol errors, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_sse_chunk_usage_and_tool_call() {
        let data = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"f","arguments":"{\"a\""}}]}}],"usage":{"prompt_tokens":7,"completion_tokens":1,"total_tokens":8}}"#;
        let events = parse_sse_chunk(data, 1).unwrap();
        assert!(
            matches!(&events[0], LlmStreamEvent::UsageUpdate { usage } if usage.input_tokens == 7 && usage.total_tokens == Some(8))
        );
        assert!(
            matches!(&events[1], LlmStreamEvent::ToolCallDelta { id, function_name, arguments_delta }
            if id == "c1" && function_name == "f" && arguments_delta == "{\"a\"")
        );
    }
}
