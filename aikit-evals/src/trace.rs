//! Trace event types for eval case execution

use aikit_sdk::{AgentEvent, AgentEventPayload, TerminalOutcome};
use serde::{Deserialize, Serialize};

/// A single line in a trace.jsonl file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    /// Sequence number (0-based)
    pub seq: usize,
    /// Event payload
    pub payload: TracePayload,
}

/// Payload of a trace event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TracePayload {
    /// A raw JSON line from the agent (tool commands, structured output)
    RawJson { data: serde_json::Value },
    /// A raw text line from stdout
    RawLine { line: String },
    /// A raw bytes chunk (base64-encoded)
    RawBytes { b64: String },
    /// Execution error
    Error { message: String },
    /// Case timed out
    Timeout,
    /// Token usage event emitted by the SDK during agent execution
    TokenUsageLine {
        usage: serde_json::Value,
        source: String,
        raw_agent_line_seq: u64,
    },
    /// Canonical text output from the agent (not a command)
    Message { text: String, role: String },
    /// A structured tool invocation decoded from the agent's output.
    /// This is what `max_tool_calls` counts.
    ToolUse {
        call_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    /// The result of a structured tool invocation; `call_id` correlates to the
    /// originating `ToolUse`.
    ToolResult {
        call_id: String,
        output: serde_json::Value,
        is_error: bool,
    },
    /// The agent's own report that a turn or run finished, decoded from the
    /// line the backend already sends (claude `result`, codex `turn.*`, pi
    /// `turn_end`).
    ///
    /// This is the evidence that separates "the agent answered badly" from
    /// "there is no answer to score". Several CLIs report a provider failure
    /// here and still exit zero, so without it a dead run is indistinguishable
    /// from a quiet one — and a dead run passes every negative check.
    ///
    /// `cost_usd` is per frame, not cumulative: pi emits one frame per turn.
    /// Sum them; never take the last one.
    Terminal {
        outcome: TerminalOutcome,
        #[serde(default)]
        reason: Option<String>,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        cost_usd: Option<f64>,
    },
    /// An `AgentEventPayload` variant this crate does not model yet.
    ///
    /// `AgentEventPayload` is `#[non_exhaustive]`, so new SDK variants land
    /// here rather than being silently mislabelled as another payload type.
    Unknown { payload_type: String, raw: String },
}

/// Render a unit-like enum value as a lowercase string, falling back to its
/// `Debug` form if it does not serialize to a JSON string.
fn enum_tag<T: Serialize + std::fmt::Debug>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_lowercase()))
        .unwrap_or_else(|| format!("{:?}", value).to_lowercase())
}

/// Name of an `AgentEventPayload` variant, taken from its serde tag so the
/// trace records *which* unmodelled variant was seen, not just that one was.
fn agent_event_payload_tag(payload: &AgentEventPayload) -> String {
    match serde_json::to_value(payload) {
        Ok(serde_json::Value::Object(map)) => map
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        Ok(serde_json::Value::String(tag)) => tag,
        _ => "unknown".to_string(),
    }
}

/// Convert aikit-sdk AgentEvent to internal TraceEvent
pub fn agent_events_to_trace(events: &[AgentEvent]) -> Vec<TraceEvent> {
    events
        .iter()
        .map(|ev| {
            let payload = match &ev.payload {
                AgentEventPayload::JsonLine(value) => TracePayload::RawJson {
                    data: value.clone(),
                },
                AgentEventPayload::RawLine(line) => TracePayload::RawLine { line: line.clone() },
                AgentEventPayload::RawBytes(bytes) => {
                    use base64::{engine::general_purpose::STANDARD, Engine as _};
                    TracePayload::RawBytes {
                        b64: STANDARD.encode(bytes),
                    }
                }
                AgentEventPayload::TokenUsageLine {
                    usage,
                    source,
                    raw_agent_line_seq,
                } => TracePayload::TokenUsageLine {
                    usage: serde_json::to_value(usage).unwrap_or(serde_json::Value::Null),
                    source: enum_tag(source),
                    raw_agent_line_seq: *raw_agent_line_seq,
                },
                AgentEventPayload::StreamMessage(message) => TracePayload::Message {
                    text: message.text.clone(),
                    role: enum_tag(&message.role),
                },
                AgentEventPayload::ToolUse {
                    call_id,
                    tool_name,
                    input,
                } => TracePayload::ToolUse {
                    call_id: call_id.clone(),
                    tool_name: tool_name.clone(),
                    input: input.clone(),
                },
                AgentEventPayload::AikitToolUse {
                    call_id,
                    tool_name,
                    tool_input,
                } => TracePayload::ToolUse {
                    call_id: call_id.clone(),
                    tool_name: tool_name.clone(),
                    input: tool_input.clone(),
                },
                AgentEventPayload::Terminal {
                    outcome,
                    reason,
                    message,
                    cost_usd,
                } => TracePayload::Terminal {
                    outcome: *outcome,
                    reason: reason.clone(),
                    message: message.clone(),
                    cost_usd: *cost_usd,
                },
                AgentEventPayload::ToolResult {
                    call_id,
                    output,
                    is_error,
                } => TracePayload::ToolResult {
                    call_id: call_id.clone(),
                    output: output.clone(),
                    is_error: *is_error,
                },
                AgentEventPayload::AikitToolResult {
                    call_id,
                    output,
                    is_error,
                } => TracePayload::ToolResult {
                    call_id: call_id.clone(),
                    output: serde_json::Value::String(output.clone()),
                    is_error: *is_error,
                },
                // `AgentEventPayload` is `#[non_exhaustive]`; anything not
                // modelled above is preserved verbatim rather than being
                // recorded as `raw_json`, which would inflate command counts.
                other => TracePayload::Unknown {
                    payload_type: agent_event_payload_tag(other),
                    raw: format!("{:?}", other),
                },
            };
            TraceEvent {
                seq: ev.seq as usize,
                payload,
            }
        })
        .collect()
}

/// Convert raw stdout lines to trace events
pub fn stdout_to_trace(stdout: &[u8]) -> Vec<TraceEvent> {
    let text = String::from_utf8_lossy(stdout);
    let mut events = Vec::new();

    for (seq, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Try to parse as JSON first
        let payload = if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            TracePayload::RawJson { data: value }
        } else {
            TracePayload::RawLine {
                line: line.to_string(),
            }
        };

        events.push(TraceEvent { seq, payload });
    }

    events
}

/// The agent's own verdict on the run, or `None` when it never gave one.
///
/// **The last status-bearing terminal event decides.** pi emits one per turn,
/// so a run that errors on turn two and recovers on turn three is a success;
/// taking the first frame, or any frame that reports an error, would call it a
/// failure. A backend that emits exactly one frame is unaffected by the rule.
pub fn terminal_outcome(
    events: &[TraceEvent],
) -> Option<(TerminalOutcome, Option<String>, Option<String>)> {
    events.iter().rev().find_map(|e| match &e.payload {
        TracePayload::Terminal {
            outcome,
            reason,
            message,
            ..
        } => Some((*outcome, reason.clone(), message.clone())),
        _ => None,
    })
}

/// Vendor-reported cost for the run, summed over terminal frames.
///
/// `None` when no frame carried a cost: absent means the backend reported
/// nothing, never zero. Never estimated from token counts and a price table —
/// a stale estimate is indistinguishable from a real number once it is written
/// to an artifact (ADR 0020).
pub fn terminal_cost_usd(events: &[TraceEvent]) -> Option<f64> {
    let mut total: Option<f64> = None;
    for e in events {
        if let TracePayload::Terminal {
            cost_usd: Some(c), ..
        } = &e.payload
        {
            total = Some(total.unwrap_or(0.0) + c);
        }
    }
    total
}

/// Parse a trace JSONL blob back into events, discarding unparseable lines.
pub fn parse_trace_jsonl(trace_jsonl: &str) -> Vec<TraceEvent> {
    trace_jsonl
        .lines()
        .filter_map(|line| serde_json::from_str::<TraceEvent>(line).ok())
        .collect()
}

/// Serialize trace events to JSONL format
pub fn trace_to_jsonl(events: &[TraceEvent]) -> String {
    events
        .iter()
        .filter_map(|e| serde_json::to_string(e).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdout_to_trace_text_lines() {
        let stdout = b"hello world\nfoo bar\n";
        let events = stdout_to_trace(stdout);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 0);
        assert!(
            matches!(&events[0].payload, TracePayload::RawLine { line } if line == "hello world")
        );
    }

    #[test]
    fn test_stdout_to_trace_json_lines() {
        let stdout = b"{\"key\": \"value\"}\nplain line\n";
        let events = stdout_to_trace(stdout);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0].payload, TracePayload::RawJson { .. }));
        assert!(matches!(&events[1].payload, TracePayload::RawLine { .. }));
    }

    #[test]
    fn test_trace_to_jsonl() {
        let events = vec![TraceEvent {
            seq: 0,
            payload: TracePayload::RawLine {
                line: "test".to_string(),
            },
        }];
        let jsonl = trace_to_jsonl(&events);
        assert!(jsonl.contains("\"seq\":0"));
        assert!(jsonl.contains("raw_line"));
    }

    #[test]
    fn test_token_usage_line_serializes_as_distinct_type() {
        let event = TraceEvent {
            seq: 7,
            payload: TracePayload::TokenUsageLine {
                usage: serde_json::json!({"input_tokens": 1234, "output_tokens": 567}),
                source: "claude".to_string(),
                raw_agent_line_seq: 6,
            },
        };
        let jsonl = trace_to_jsonl(std::slice::from_ref(&event));
        assert!(
            jsonl.contains("\"type\":\"token_usage_line\""),
            "expected token_usage_line type tag, got: {}",
            jsonl
        );
        assert!(
            !jsonl.contains("\"type\":\"raw_json\""),
            "token_usage_line must not serialize as raw_json, got: {}",
            jsonl
        );
        let deserialized: TraceEvent = serde_json::from_str(&jsonl).unwrap();
        assert!(
            matches!(deserialized.payload, TracePayload::TokenUsageLine { .. }),
            "deserialized payload must be TokenUsageLine"
        );
    }

    #[test]
    fn test_agent_events_to_trace_maps_raw_payload_variants() {
        use aikit_sdk::AgentEventStream;
        let events = vec![
            AgentEvent {
                agent_key: "codex".to_string(),
                seq: 0,
                stream: AgentEventStream::Stdout,
                payload: AgentEventPayload::JsonLine(serde_json::json!({"cmd": "ls"})),
            },
            AgentEvent {
                agent_key: "codex".to_string(),
                seq: 1,
                stream: AgentEventStream::Stdout,
                payload: AgentEventPayload::RawLine("plain".to_string()),
            },
            AgentEvent {
                agent_key: "codex".to_string(),
                seq: 2,
                stream: AgentEventStream::Stdout,
                payload: AgentEventPayload::RawBytes(vec![0xff, 0x00]),
            },
        ];
        let trace = agent_events_to_trace(&events);

        assert!(matches!(
            &trace[0].payload,
            TracePayload::RawJson { data } if data == &serde_json::json!({"cmd": "ls"})
        ));
        assert!(matches!(
            &trace[1].payload,
            TracePayload::RawLine { line } if line == "plain"
        ));
        assert!(matches!(
            &trace[2].payload,
            TracePayload::RawBytes { b64 } if b64 == "/wA="
        ));
    }

    #[test]
    fn test_agent_events_to_trace_maps_token_usage_and_tool_results() {
        use aikit_sdk::{AgentEventStream, TokenUsage, UsageSource};
        let events = vec![
            AgentEvent {
                agent_key: "codex".to_string(),
                seq: 0,
                stream: AgentEventStream::Stdout,
                payload: AgentEventPayload::TokenUsageLine {
                    usage: TokenUsage {
                        input_tokens: 3,
                        output_tokens: 4,
                        total_tokens: Some(7),
                        cache_read_tokens: None,
                        cache_creation_tokens: None,
                        reasoning_tokens: None,
                    },
                    source: UsageSource::Codex,
                    raw_agent_line_seq: 9,
                },
            },
            AgentEvent {
                agent_key: "codex".to_string(),
                seq: 1,
                stream: AgentEventStream::Stdout,
                payload: AgentEventPayload::ToolResult {
                    call_id: "item_1".to_string(),
                    output: serde_json::json!("ok"),
                    is_error: false,
                },
            },
        ];
        let trace = agent_events_to_trace(&events);

        assert!(matches!(
            &trace[0].payload,
            TracePayload::TokenUsageLine {
                source,
                raw_agent_line_seq: 9,
                ..
            } if source == "codex"
        ));
        assert!(matches!(
            &trace[1].payload,
            TracePayload::ToolResult {
                call_id,
                output,
                is_error: false
            } if call_id == "item_1" && output == &serde_json::json!("ok")
        ));
    }

    #[test]
    fn test_agent_events_to_trace_maps_aikit_tool_aliases() {
        use aikit_sdk::AgentEventStream;
        let events = vec![
            AgentEvent {
                agent_key: "aikit".to_string(),
                seq: 0,
                stream: AgentEventStream::Stdout,
                payload: AgentEventPayload::AikitToolUse {
                    call_id: "call_a".to_string(),
                    tool_name: "read_file".to_string(),
                    tool_input: serde_json::json!({"path": "README.md"}),
                },
            },
            AgentEvent {
                agent_key: "aikit".to_string(),
                seq: 1,
                stream: AgentEventStream::Stdout,
                payload: AgentEventPayload::AikitToolResult {
                    call_id: "call_a".to_string(),
                    output: "contents".to_string(),
                    is_error: true,
                },
            },
        ];
        let trace = agent_events_to_trace(&events);

        assert!(matches!(
            &trace[0].payload,
            TracePayload::ToolUse {
                call_id,
                tool_name,
                input
            } if call_id == "call_a"
                && tool_name == "read_file"
                && input == &serde_json::json!({"path": "README.md"})
        ));
        assert!(matches!(
            &trace[1].payload,
            TracePayload::ToolResult {
                call_id,
                output,
                is_error: true
            } if call_id == "call_a" && output == &serde_json::json!("contents")
        ));
    }

    #[test]
    fn test_stdout_to_trace_skips_blank_lines() {
        let events = stdout_to_trace(b"\n  \nvisible\n");
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].payload,
            TracePayload::RawLine { line } if line == "visible"
        ));
    }

    fn stream_message_event(seq: u64, text: &str) -> AgentEvent {
        use aikit_sdk::{AgentEventStream, MessageKind, MessagePhase, MessageRole, StreamMessage};
        AgentEvent {
            agent_key: "claude".to_string(),
            seq,
            stream: AgentEventStream::Stdout,
            payload: AgentEventPayload::StreamMessage(StreamMessage {
                text: text.to_string(),
                phase: MessagePhase::Final,
                role: MessageRole::Assistant,
                kind: MessageKind::Message,
                source: AgentEventStream::Stdout,
                raw_line_seq: seq,
                turn_id: None,
            }),
        }
    }

    fn tool_use_event(seq: u64, tool_name: &str) -> AgentEvent {
        use aikit_sdk::AgentEventStream;
        AgentEvent {
            agent_key: "claude".to_string(),
            seq,
            stream: AgentEventStream::Stdout,
            payload: AgentEventPayload::ToolUse {
                call_id: format!("call_{}", seq),
                tool_name: tool_name.to_string(),
                input: serde_json::json!({"command": "ls"}),
            },
        }
    }

    /// Regression test for the defect where an agent that ran **zero** tools
    /// still reported `command_count: 2`: `StreamMessage` fell through to the
    /// catch-all arm, was stored as `raw_json`, and was then counted as a
    /// command. Text output is not a command.
    #[test]
    fn test_text_only_run_counts_zero_commands() {
        use crate::checks::count_command_events;
        let events = vec![
            stream_message_event(0, "Here are the three options you asked for."),
            stream_message_event(1, "Let me know which you would like."),
        ];
        let jsonl = trace_to_jsonl(&agent_events_to_trace(&events));

        assert!(
            !jsonl.contains("\"type\":\"raw_json\""),
            "stream messages must not be recorded as raw_json, got: {}",
            jsonl
        );
        assert_eq!(
            count_command_events(&jsonl),
            0,
            "an agent that invoked no tools must report 0 commands, got: {}",
            jsonl
        );
    }

    #[test]
    fn test_tool_use_events_are_counted_as_commands() {
        use crate::checks::count_command_events;
        let events = vec![
            stream_message_event(0, "I will list the directory."),
            tool_use_event(1, "bash"),
            stream_message_event(2, "Now reading the file."),
            tool_use_event(3, "read_file"),
        ];
        let jsonl = trace_to_jsonl(&agent_events_to_trace(&events));

        assert_eq!(
            count_command_events(&jsonl),
            2,
            "expected exactly the two tool_use events to count, got: {}",
            jsonl
        );
        assert!(
            jsonl.contains("\"tool_name\":\"bash\""),
            "tool name must be preserved in the trace, got: {}",
            jsonl
        );
    }

    #[test]
    fn test_codex_tool_use_event_counts_non_zero_commands() {
        use crate::checks::count_command_events;
        use aikit_sdk::AgentEventStream;
        let mut events = vec![
            stream_message_event(0, "I will inspect the tree"),
            AgentEvent {
                agent_key: "codex".to_string(),
                seq: 1,
                stream: AgentEventStream::Stdout,
                payload: AgentEventPayload::ToolUse {
                    call_id: "item_1".to_string(),
                    tool_name: "shell".to_string(),
                    input: serde_json::json!({"command": "ls -la"}),
                },
            },
            AgentEvent {
                agent_key: "codex".to_string(),
                seq: 2,
                stream: AgentEventStream::Stdout,
                payload: AgentEventPayload::ToolResult {
                    call_id: "item_1".to_string(),
                    output: serde_json::json!("total 0\n"),
                    is_error: false,
                },
            },
        ];
        events[0].agent_key = "codex".to_string();
        let jsonl = trace_to_jsonl(&agent_events_to_trace(&events));

        assert_eq!(
            count_command_events(&jsonl),
            1,
            "codex structured tool_use must produce a non-zero command count, got: {}",
            jsonl
        );
    }

    #[test]
    fn test_unmodelled_payload_is_not_counted_as_a_command() {
        use crate::checks::count_command_events;
        use aikit_sdk::AgentEventStream;
        let events = vec![AgentEvent {
            agent_key: "claude".to_string(),
            seq: 0,
            stream: AgentEventStream::Stdout,
            payload: AgentEventPayload::RawTransportLine {
                raw: "{\"debug\": true}".to_string(),
                stream: AgentEventStream::Stdout,
                seq: 0,
            },
        }];
        let jsonl = trace_to_jsonl(&agent_events_to_trace(&events));

        assert!(
            jsonl.contains("\"type\":\"unknown\""),
            "unmodelled payloads must serialize as `unknown`, got: {}",
            jsonl
        );
        assert_eq!(
            count_command_events(&jsonl),
            0,
            "unmodelled payloads must not count as commands, got: {}",
            jsonl
        );
    }

    #[test]
    fn test_count_raw_json_excludes_token_usage_line() {
        use crate::checks::count_raw_json_events;
        let events = vec![
            TraceEvent {
                seq: 0,
                payload: TracePayload::TokenUsageLine {
                    usage: serde_json::json!({"input_tokens": 100, "output_tokens": 50}),
                    source: "claude".to_string(),
                    raw_agent_line_seq: 0,
                },
            },
            TraceEvent {
                seq: 1,
                payload: TracePayload::TokenUsageLine {
                    usage: serde_json::json!({"input_tokens": 200, "output_tokens": 100}),
                    source: "codex".to_string(),
                    raw_agent_line_seq: 1,
                },
            },
        ];
        let jsonl = trace_to_jsonl(&events);
        let count = count_raw_json_events(&jsonl);
        assert_eq!(
            count, 0,
            "count_raw_json_events must return 0 for token_usage_line-only traces, got: {}",
            count
        );
    }

    // ── R2: the terminal frame survives normalization ──────────────────────

    fn terminal_event(
        seq: usize,
        outcome: TerminalOutcome,
        reason: &str,
        cost: Option<f64>,
    ) -> TraceEvent {
        TraceEvent {
            seq,
            payload: TracePayload::Terminal {
                outcome,
                reason: Some(reason.to_string()),
                message: None,
                cost_usd: cost,
            },
        }
    }

    #[test]
    fn test_terminal_payload_survives_the_jsonl_round_trip() {
        let events = vec![TraceEvent {
            seq: 0,
            payload: TracePayload::Terminal {
                outcome: TerminalOutcome::Error,
                reason: Some("error".to_string()),
                message: Some("Request timed out.".to_string()),
                cost_usd: Some(0.000_909_72),
            },
        }];
        let parsed = parse_trace_jsonl(&trace_to_jsonl(&events));
        assert_eq!(parsed.len(), 1);
        match &parsed[0].payload {
            TracePayload::Terminal {
                outcome,
                reason,
                message,
                cost_usd,
            } => {
                assert_eq!(*outcome, TerminalOutcome::Error);
                assert_eq!(reason.as_deref(), Some("error"));
                assert_eq!(message.as_deref(), Some("Request timed out."));
                assert_eq!(*cost_usd, Some(0.000_909_72));
            }
            other => panic!("expected Terminal, got {other:?}"),
        }
    }

    #[test]
    fn test_last_status_bearing_terminal_event_decides_the_run() {
        // pi emits one frame per turn. A run that errors and then recovers is
        // a success, so the decision reads the last frame, not the first.
        let recovered = vec![
            terminal_event(0, TerminalOutcome::Error, "error", None),
            terminal_event(1, TerminalOutcome::Success, "end_turn", None),
        ];
        assert_eq!(
            terminal_outcome(&recovered).map(|(o, _, _)| o),
            Some(TerminalOutcome::Success)
        );

        let gave_up = vec![
            terminal_event(0, TerminalOutcome::Success, "end_turn", None),
            terminal_event(1, TerminalOutcome::Error, "error", None),
        ];
        assert_eq!(
            terminal_outcome(&gave_up).map(|(o, _, _)| o),
            Some(TerminalOutcome::Error)
        );
    }

    #[test]
    fn test_terminal_outcome_is_none_when_the_stream_carried_no_frame() {
        let events = vec![TraceEvent {
            seq: 0,
            payload: TracePayload::RawLine {
                line: "hello".to_string(),
            },
        }];
        assert!(terminal_outcome(&events).is_none());
    }

    #[test]
    fn test_cost_sums_per_turn_frames() {
        // pi reports cost per turn, not cumulatively, so the run's cost is the
        // sum of its frames.
        let events = vec![
            terminal_event(0, TerminalOutcome::Success, "end_turn", Some(0.000_909_72)),
            terminal_event(1, TerminalOutcome::Success, "end_turn", Some(0.008_472_96)),
            terminal_event(2, TerminalOutcome::Success, "end_turn", Some(0.003_537_52)),
        ];
        let total = terminal_cost_usd(&events).unwrap();
        assert!((total - 0.012_920_2).abs() < 1e-9, "{total}");
    }

    #[test]
    fn test_cost_is_none_when_no_frame_reported_one() {
        // Absent, never zero: a backend that reports nothing must not look
        // like a run that was free (ADR 0020, R5).
        let events = vec![terminal_event(
            0,
            TerminalOutcome::Success,
            "end_turn",
            None,
        )];
        assert_eq!(terminal_cost_usd(&events), None);
    }

    #[test]
    fn test_agent_terminal_event_becomes_a_trace_terminal_payload() {
        // The defect this whole change starts from: the agent said
        // `stopReason: error` and normalization dropped it.
        use aikit_sdk::AgentEventStream;
        let events = vec![AgentEvent {
            agent_key: "pi".to_string(),
            seq: 0,
            stream: AgentEventStream::Stdout,
            payload: AgentEventPayload::Terminal {
                outcome: TerminalOutcome::Error,
                reason: Some("error".to_string()),
                message: Some("Request timed out.".to_string()),
                cost_usd: Some(0.001),
            },
        }];
        let trace = agent_events_to_trace(&events);
        assert_eq!(trace.len(), 1);
        assert!(
            matches!(
                &trace[0].payload,
                TracePayload::Terminal {
                    outcome: TerminalOutcome::Error,
                    ..
                }
            ),
            "{:?}",
            trace[0].payload
        );
    }
}
