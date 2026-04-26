//! Progress state management for `aikit run --progress`.
//!
//! Classifies and formats [`AgentEvent`] payloads into human-readable lines
//! stored in a ring buffer, independent of terminal rendering.

use std::collections::VecDeque;

use crate::{AgentEvent, AgentEventPayload, MessageKind, MessageRole, TokenUsage, UsageSource};

/// Configuration for progress display behaviour.
#[derive(Debug, Clone)]
pub struct ProgressViewConfig {
    /// Maximum number of formatted lines to retain (ring buffer).
    pub max_rows: usize,
    /// Whether to show token usage footer/updates.
    pub show_tokens: bool,
    /// Maximum characters for text truncation.
    pub max_text_width: usize,
    /// Maximum characters for tool output snippets.
    pub max_tool_output_chars: usize,
}

impl Default for ProgressViewConfig {
    fn default() -> Self {
        Self {
            max_rows: 20,
            show_tokens: true,
            max_text_width: 80,
            max_tool_output_chars: 100,
        }
    }
}

/// Progress state manager that classifies agent events into formatted display lines.
///
/// Maintains a fixed-size ring buffer of recent events and optionally tracks
/// token usage for display in a footer. Designed for single-threaded use
/// within a `run_agent_events` callback.
#[derive(Debug, Clone)]
pub struct RunProgress {
    config: ProgressViewConfig,
    rows: VecDeque<String>,
    latest_token_usage: Option<(TokenUsage, UsageSource)>,
    last_assistant_text: Option<String>,
}

impl RunProgress {
    /// Create a new progress tracker with the given configuration.
    pub fn new(config: ProgressViewConfig) -> Self {
        Self {
            config,
            rows: VecDeque::new(),
            latest_token_usage: None,
            last_assistant_text: None,
        }
    }

    /// Process an agent event and update internal state.
    pub fn push(&mut self, _agent_key: &str, event: &AgentEvent) {
        match &event.payload {
            AgentEventPayload::TokenUsageLine { usage, source, .. } => {
                self.latest_token_usage = Some((usage.clone(), source.clone()));
            }
            AgentEventPayload::StreamMessage(sm) => {
                let text = sm.text.replace('\n', " ").replace('\r', "");
                let text = text.trim();
                if text.is_empty() {
                    return;
                }
                let truncated = truncate(text, self.config.max_text_width);
                match sm.role {
                    MessageRole::Assistant => {
                        if Some(text) == self.last_assistant_text.as_deref() {
                            return;
                        }
                        self.add_row(format!("assistant> {}", truncated));
                        self.last_assistant_text = Some(text.to_string());
                    }
                    MessageRole::Tool => match sm.kind {
                        MessageKind::ToolOutput => {
                            let out_truncated = truncate(text, self.config.max_tool_output_chars);
                            if !out_truncated.is_empty() {
                                self.add_row(format!("tool> {}", out_truncated));
                            }
                        }
                        _ => {
                            self.add_row(format!("tool> {}", truncated));
                        }
                    },
                    MessageRole::System => {
                        self.add_row(format!("system> {}", truncated));
                    }
                    MessageRole::User => {
                        self.add_row(format!("user> {}", truncated));
                    }
                }
            }
            AgentEventPayload::JsonLine(_) => {}
            AgentEventPayload::RawLine(text) => {
                let prefix = match event.stream {
                    crate::AgentEventStream::Stdout => "out>",
                    crate::AgentEventStream::Stderr => "err>",
                };
                let truncated = truncate(text, self.config.max_text_width);
                self.add_row(format!("{} {}", prefix, truncated));
            }
            AgentEventPayload::RawBytes(bytes) => {
                let prefix = match event.stream {
                    crate::AgentEventStream::Stdout => "out>",
                    crate::AgentEventStream::Stderr => "err>",
                };
                let text = String::from_utf8_lossy(bytes);
                let truncated = truncate(&text, self.config.max_text_width);
                self.add_row(format!("{} {}", prefix, truncated));
            }
            AgentEventPayload::QuotaExceeded { info, .. } => {
                let truncated = truncate(&info.raw_message, self.config.max_text_width);
                self.add_row(format!("[quota] {}", truncated));
            }
            AgentEventPayload::AikitTextDelta { content, .. }
            | AgentEventPayload::AikitTextFinal { content, .. } => {
                let text = content.replace('\n', " ").replace('\r', "");
                let text = text.trim();
                if !text.is_empty() {
                    self.add_row(format!(
                        "assistant> {}",
                        truncate(text, self.config.max_text_width)
                    ));
                    self.last_assistant_text = Some(text.to_string());
                }
            }
            AgentEventPayload::AikitToolUse { tool_name, .. } => {
                self.add_row(format!("tool> {}", tool_name));
            }
            AgentEventPayload::AikitToolResult {
                output, is_error, ..
            } => {
                let prefix = if *is_error { "tool error>" } else { "tool>" };
                self.add_row(format!(
                    "{} {}",
                    prefix,
                    truncate(output, self.config.max_tool_output_chars)
                ));
            }
            AgentEventPayload::AikitSubagentSpawn { subagent_id, .. } => {
                self.add_row(format!("subagent> spawned {}", subagent_id));
            }
            AgentEventPayload::AikitSubagentResult {
                subagent_id,
                status,
                ..
            } => {
                self.add_row(format!("subagent> {} {}", subagent_id, status));
            }
            AgentEventPayload::AikitContextCompressed {
                original_tokens,
                compressed_tokens,
                ..
            } => {
                self.add_row(format!(
                    "context> compressed {} -> {} tokens",
                    original_tokens, compressed_tokens
                ));
            }
            AgentEventPayload::AikitStepFinish {
                iteration,
                finish_reason,
            } => {
                self.add_row(format!("step> {} {}", iteration, finish_reason));
            }
            AgentEventPayload::RawTransportLine { .. } => {}
        }
    }

    /// Get an iterator over the current formatted lines (ring buffer content).
    pub fn formatted_lines(&self) -> impl Iterator<Item = &str> {
        self.rows.iter().map(|s| s.as_str())
    }

    /// Get current token usage footer text if `show_tokens` is enabled.
    pub fn token_footer(&self) -> Option<String> {
        if !self.config.show_tokens {
            return None;
        }
        let (usage, source) = self.latest_token_usage.as_ref()?;
        let source_label = match source {
            UsageSource::Codex => "codex",
            UsageSource::Claude => "claude",
            UsageSource::Gemini => "gemini",
            UsageSource::OpenCode => "opencode",
            UsageSource::Cursor => "cursor",
            UsageSource::Aikit => "aikit",
        };
        let computed_total = usage.input_tokens + usage.output_tokens;
        let agent_total_suffix = match usage.total_tokens {
            Some(t) if t != computed_total => format!(" agent_total={}", t),
            _ => String::new(),
        };
        Some(format!(
            "[tokens] {}  in={} out={} total={}{}",
            source_label,
            usage.input_tokens,
            usage.output_tokens,
            computed_total,
            agent_total_suffix
        ))
    }

    /// Clear all progress state.
    pub fn clear(&mut self) {
        self.rows.clear();
        self.latest_token_usage = None;
        self.last_assistant_text = None;
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    fn add_row(&mut self, row: String) {
        if self.config.max_rows == 0 {
            return;
        }
        while self.rows.len() >= self.config.max_rows {
            self.rows.pop_front();
        }
        self.rows.push_back(row);
    }
}

/// Truncate a string to at most `max_chars` characters, appending `…` if truncated.
fn truncate(s: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    if s.len() <= max_chars {
        s
    } else {
        // Find the last char boundary within max_chars
        let mut end = max_chars;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentEventPayload, AgentEventStream, StreamMessage};

    fn make_stream_event(text: &str, role: MessageRole, kind: MessageKind) -> (String, AgentEvent) {
        let event = AgentEvent {
            agent_key: "opencode".to_string(),
            seq: 0,
            stream: AgentEventStream::Stdout,
            payload: AgentEventPayload::StreamMessage(StreamMessage {
                text: text.to_string(),
                phase: crate::MessagePhase::Delta,
                role,
                kind,
                source: AgentEventStream::Stdout,
                raw_line_seq: 0,
                turn_id: None,
            }),
        };
        ("opencode".to_string(), event)
    }

    #[test]
    fn test_opencode_text_event() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let (key, event) =
            make_stream_event("hello world", MessageRole::Assistant, MessageKind::Message);
        progress.push(&key, &event);
        let lines: Vec<_> = progress.formatted_lines().collect();
        assert_eq!(lines, vec!["assistant> hello world"]);
    }

    #[test]
    fn test_opencode_step_start_suppressed() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let val: serde_json::Value =
            serde_json::from_str(r#"{"type":"step_start","timestamp":123}"#).unwrap();
        let event = AgentEvent {
            agent_key: "opencode".to_string(),
            seq: 0,
            stream: AgentEventStream::Stdout,
            payload: AgentEventPayload::JsonLine(val),
        };
        progress.push("opencode", &event);
        assert_eq!(progress.formatted_lines().count(), 0);
    }

    #[test]
    fn test_opencode_tool_use_event() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let (key, event) = make_stream_event(
            "total 8\ndrwxr-xr-x",
            MessageRole::Tool,
            MessageKind::ToolOutput,
        );
        progress.push(&key, &event);
        let lines: Vec<_> = progress.formatted_lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("tool>"));
    }

    #[test]
    fn test_ring_buffer_overflow() {
        let config = ProgressViewConfig {
            max_rows: 3,
            ..Default::default()
        };
        let mut progress = RunProgress::new(config);
        for i in 0..5 {
            let (key, event) = make_stream_event(
                &format!("msg {i}"),
                MessageRole::Assistant,
                MessageKind::Message,
            );
            progress.push(&key, &event);
        }
        let lines: Vec<_> = progress.formatted_lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("msg 2"));
        assert!(lines[1].contains("msg 3"));
        assert!(lines[2].contains("msg 4"));
    }

    #[test]
    fn test_token_footer() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let event = AgentEvent {
            agent_key: "opencode".to_string(),
            seq: 1,
            stream: AgentEventStream::Stdout,
            payload: AgentEventPayload::TokenUsageLine {
                usage: TokenUsage {
                    input_tokens: 100,
                    output_tokens: 50,
                    total_tokens: Some(150),
                    cache_read_tokens: None,
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                source: UsageSource::OpenCode,
                raw_agent_line_seq: 0,
            },
        };
        progress.push("opencode", &event);
        let footer = progress.token_footer().unwrap();
        assert!(footer.contains("in=100"));
        assert!(footer.contains("out=50"));
        assert!(footer.contains("total=150"));
    }

    #[test]
    fn test_token_footer_disabled() {
        let config = ProgressViewConfig {
            show_tokens: false,
            ..Default::default()
        };
        let mut progress = RunProgress::new(config);
        let event = AgentEvent {
            agent_key: "opencode".to_string(),
            seq: 1,
            stream: AgentEventStream::Stdout,
            payload: AgentEventPayload::TokenUsageLine {
                usage: TokenUsage {
                    input_tokens: 100,
                    output_tokens: 50,
                    total_tokens: Some(150),
                    cache_read_tokens: None,
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                source: UsageSource::OpenCode,
                raw_agent_line_seq: 0,
            },
        };
        progress.push("opencode", &event);
        assert!(progress.token_footer().is_none());
    }

    #[test]
    fn test_raw_line_stdout() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let event = AgentEvent {
            agent_key: "claude".to_string(),
            seq: 0,
            stream: AgentEventStream::Stdout,
            payload: AgentEventPayload::RawLine("hello raw".to_string()),
        };
        progress.push("claude", &event);
        let lines: Vec<_> = progress.formatted_lines().collect();
        assert_eq!(lines, vec!["out> hello raw"]);
    }

    #[test]
    fn test_clear() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let (key, event) = make_stream_event("hello", MessageRole::Assistant, MessageKind::Message);
        progress.push(&key, &event);
        assert_eq!(progress.formatted_lines().count(), 1);
        progress.clear();
        assert_eq!(progress.formatted_lines().count(), 0);
        assert!(progress.token_footer().is_none());
    }

    #[test]
    fn test_text_deduplication() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let (key, event1) =
            make_stream_event("hello world", MessageRole::Assistant, MessageKind::Message);
        let (_, event2) =
            make_stream_event("hello world", MessageRole::Assistant, MessageKind::Message);
        progress.push(&key, &event1);
        progress.push(&key, &event2);
        let lines: Vec<_> = progress.formatted_lines().collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "assistant> hello world");
    }

    #[test]
    fn test_text_leading_newline_normalized() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let (key, event) =
            make_stream_event("\nHello", MessageRole::Assistant, MessageKind::Message);
        progress.push(&key, &event);
        let lines: Vec<_> = progress.formatted_lines().collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "assistant> Hello");
        assert!(!lines[0].contains('\n'));
    }

    #[test]
    fn test_text_whitespace_only_suppressed() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let (key, event) = make_stream_event("   ", MessageRole::Assistant, MessageKind::Message);
        progress.push(&key, &event);
        assert_eq!(progress.formatted_lines().count(), 0);
    }

    #[test]
    fn test_text_dedup_reset_after_clear() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let (key, event1) =
            make_stream_event("hello", MessageRole::Assistant, MessageKind::Message);
        progress.push(&key, &event1);
        assert_eq!(progress.formatted_lines().count(), 1);
        progress.clear();
        let (_, event2) = make_stream_event("hello", MessageRole::Assistant, MessageKind::Message);
        progress.push(&key, &event2);
        let lines: Vec<_> = progress.formatted_lines().collect();
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_token_footer_agent_total_differs() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let event = AgentEvent {
            agent_key: "opencode".to_string(),
            seq: 1,
            stream: AgentEventStream::Stdout,
            payload: AgentEventPayload::TokenUsageLine {
                usage: TokenUsage {
                    input_tokens: 270,
                    output_tokens: 359,
                    total_tokens: Some(22058),
                    cache_read_tokens: None,
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                source: UsageSource::OpenCode,
                raw_agent_line_seq: 0,
            },
        };
        progress.push("opencode", &event);
        let footer = progress.token_footer().unwrap();
        assert!(footer.contains("total=629"), "footer: {}", footer);
        assert!(footer.contains("agent_total=22058"), "footer: {}", footer);
    }

    #[test]
    fn test_token_footer_agent_total_same() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let event = AgentEvent {
            agent_key: "opencode".to_string(),
            seq: 1,
            stream: AgentEventStream::Stdout,
            payload: AgentEventPayload::TokenUsageLine {
                usage: TokenUsage {
                    input_tokens: 100,
                    output_tokens: 50,
                    total_tokens: Some(150),
                    cache_read_tokens: None,
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                source: UsageSource::OpenCode,
                raw_agent_line_seq: 0,
            },
        };
        progress.push("opencode", &event);
        let footer = progress.token_footer().unwrap();
        assert!(footer.contains("total=150"), "footer: {}", footer);
        assert!(!footer.contains("agent_total="), "footer: {}", footer);
    }

    #[test]
    fn test_system_role_display() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let (key, event) =
            make_stream_event("session started", MessageRole::System, MessageKind::Status);
        progress.push(&key, &event);
        let lines: Vec<_> = progress.formatted_lines().collect();
        assert_eq!(lines, vec!["system> session started"]);
    }

    #[test]
    fn test_tool_role_display() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let (key, event) = make_stream_event("ls -la", MessageRole::Tool, MessageKind::Message);
        progress.push(&key, &event);
        let lines: Vec<_> = progress.formatted_lines().collect();
        assert_eq!(lines, vec!["tool> ls -la"]);
    }
}
