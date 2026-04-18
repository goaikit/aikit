//! Progress state management for `aikit run --progress`.
//!
//! Classifies and formats [`AgentEvent`] payloads into human-readable lines
//! stored in a ring buffer, independent of terminal rendering.

use std::collections::VecDeque;

use crate::{AgentEvent, AgentEventPayload, TokenUsage, UsageSource};

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
    pub fn push(&mut self, agent_key: &str, event: &AgentEvent) {
        match &event.payload {
            AgentEventPayload::TokenUsageLine { usage, source, .. } => {
                self.latest_token_usage = Some((usage.clone(), source.clone()));
            }
            AgentEventPayload::JsonLine(val) => {
                let val = val.clone();
                let line = self.classify_json(agent_key, &val);
                if let Some(l) = line {
                    self.add_row(l);
                }
            }
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

    /// Classify a JSON event value into a formatted display string.
    ///
    /// Returns `None` for events that should be suppressed (e.g. `step_start`).
    fn classify_json(&mut self, agent_key: &str, val: &serde_json::Value) -> Option<String> {
        if agent_key == "opencode" {
            self.classify_opencode(val)
        } else {
            Self::classify_fallback(val, self.config.max_text_width)
        }
    }

    /// OpenCode-specific event classification.
    fn classify_opencode(&mut self, val: &serde_json::Value) -> Option<String> {
        let event_type = val.get("type")?.as_str()?;

        match event_type {
            "step_start" => None, // suppress by default

            "text" => {
                // {"type":"text","part":{"text":"..."}}
                let text = val
                    .get("part")
                    .and_then(|p| p.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                // Normalize: replace newlines with spaces, then trim whitespace
                let normalized = text.replace('\n', " ").replace('\r', "");
                let normalized = normalized.trim();
                if normalized.is_empty() {
                    return None;
                }
                // Deduplicate consecutive identical assistant rows
                if Some(normalized) == self.last_assistant_text.as_deref() {
                    return None;
                }
                let truncated = truncate(normalized, self.config.max_text_width);
                let row = format!("assistant> {}", truncated);
                self.last_assistant_text = Some(normalized.to_string());
                Some(row)
            }

            "tool_use" => {
                // {"type":"tool_use","part":{"tool":"bash","input":{"command":"..."},"output":"...","exit":0}}
                let part = val.get("part")?;
                let tool_name = part.get("tool").and_then(|t| t.as_str()).unwrap_or("tool");
                // Suppress intermediate tool events with unresolved tool name
                if tool_name == "invalid" {
                    return None;
                }
                let command = part
                    .get("input")
                    .and_then(|i| i.get("command"))
                    .and_then(|c| c.as_str())
                    .or_else(|| part.get("input").and_then(|i| i.as_str()))
                    .unwrap_or("");
                let exit_code = part.get("exit").and_then(|e| e.as_i64());
                let output = part.get("output").and_then(|o| o.as_str()).unwrap_or("");

                let cmd_truncated = truncate(command, self.config.max_text_width);
                let out_truncated = truncate(output, self.config.max_tool_output_chars);

                let exit_str = match exit_code {
                    Some(0) => " [ok]".to_string(),
                    Some(n) => format!(" [exit={}]", n),
                    None => String::new(),
                };

                let mut line = format!("tool({})> {}{}", tool_name, cmd_truncated, exit_str);
                if !out_truncated.is_empty() {
                    line.push_str(&format!(" | {}", out_truncated));
                }
                Some(line)
            }

            "step_finish" => {
                let reason = val
                    .get("part")
                    .and_then(|p| p.get("reason"))
                    .and_then(|r| r.as_str())
                    .unwrap_or("done");
                // Suppress normal intermediate step reasons
                if matches!(reason, "stop" | "done" | "tool-calls") {
                    None
                } else {
                    Some(format!("step_finish: {}", reason))
                }
            }

            other => Self::classify_fallback_with_type(other, val, self.config.max_text_width),
        }
    }

    /// Compact fallback for non-OpenCode agents.
    fn classify_fallback(val: &serde_json::Value, max_width: usize) -> Option<String> {
        if let Some(t) = val.get("type").and_then(|v| v.as_str()) {
            Some(Self::classify_fallback_with_type(t, val, max_width)?)
        } else {
            let raw = val.to_string();
            Some(truncate(&raw, max_width).to_string())
        }
    }

    fn classify_fallback_with_type(
        event_type: &str,
        val: &serde_json::Value,
        max_width: usize,
    ) -> Option<String> {
        let raw = val.to_string();
        let truncated = truncate(&raw, max_width);
        Some(format!("[{}] {}", event_type, truncated))
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
    use crate::{AgentEventPayload, AgentEventStream};

    fn make_json_event(agent_key: &str, json: &str) -> (String, AgentEvent) {
        let val: serde_json::Value = serde_json::from_str(json).unwrap();
        let event = AgentEvent {
            agent_key: agent_key.to_string(),
            seq: 0,
            stream: AgentEventStream::Stdout,
            payload: AgentEventPayload::JsonLine(val),
        };
        (agent_key.to_string(), event)
    }

    #[test]
    fn test_opencode_text_event() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let (key, event) = make_json_event(
            "opencode",
            r#"{"type":"text","part":{"text":"hello world"}}"#,
        );
        progress.push(&key, &event);
        let lines: Vec<_> = progress.formatted_lines().collect();
        assert_eq!(lines, vec!["assistant> hello world"]);
    }

    #[test]
    fn test_opencode_step_start_suppressed() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let (key, event) = make_json_event("opencode", r#"{"type":"step_start","timestamp":123}"#);
        progress.push(&key, &event);
        assert_eq!(progress.formatted_lines().count(), 0);
    }

    #[test]
    fn test_opencode_tool_use_event() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let (key, event) = make_json_event(
            "opencode",
            r#"{"type":"tool_use","part":{"tool":"bash","input":{"command":"ls -la"},"exit":0,"output":"total 8\ndrwxr-xr-x"}}"#,
        );
        progress.push(&key, &event);
        let lines: Vec<_> = progress.formatted_lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("bash"));
        assert!(lines[0].contains("ls -la"));
        assert!(lines[0].contains("[ok]"));
    }

    #[test]
    fn test_ring_buffer_overflow() {
        let config = ProgressViewConfig {
            max_rows: 3,
            ..Default::default()
        };
        let mut progress = RunProgress::new(config);
        for i in 0..5 {
            let (key, event) = make_json_event(
                "opencode",
                &format!(r#"{{"type":"text","part":{{"text":"msg {i}"}}}}"#),
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
        let (key, event) =
            make_json_event("opencode", r#"{"type":"text","part":{"text":"hello"}}"#);
        progress.push(&key, &event);
        assert_eq!(progress.formatted_lines().count(), 1);
        progress.clear();
        assert_eq!(progress.formatted_lines().count(), 0);
        assert!(progress.token_footer().is_none());
    }

    #[test]
    fn test_tool_invalid_suppressed() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let (key, event) = make_json_event(
            "opencode",
            r#"{"type":"tool_use","part":{"tool":"invalid","input":{},"output":""}}"#,
        );
        progress.push(&key, &event);
        assert_eq!(progress.formatted_lines().count(), 0);
    }

    #[test]
    fn test_text_deduplication() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let json = r#"{"type":"text","part":{"text":"hello world"}}"#;
        let (key, event1) = make_json_event("opencode", json);
        let (_, event2) = make_json_event("opencode", json);
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
            make_json_event("opencode", r#"{"type":"text","part":{"text":"\nHello"}}"#);
        progress.push(&key, &event);
        let lines: Vec<_> = progress.formatted_lines().collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "assistant> Hello");
        assert!(!lines[0].contains('\n'));
    }

    #[test]
    fn test_text_whitespace_only_suppressed() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let (key, event) = make_json_event("opencode", r#"{"type":"text","part":{"text":"   "}}"#);
        progress.push(&key, &event);
        assert_eq!(progress.formatted_lines().count(), 0);
    }

    #[test]
    fn test_text_dedup_reset_after_clear() {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let json = r#"{"type":"text","part":{"text":"hello"}}"#;
        let (key, event1) = make_json_event("opencode", json);
        progress.push(&key, &event1);
        assert_eq!(progress.formatted_lines().count(), 1);
        progress.clear();
        let (_, event2) = make_json_event("opencode", json);
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
}
