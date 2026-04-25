//! Snapshot tests for `RunProgress` event classification.
//!
//! Uses synthetic `AgentEvent` fixtures and `insta` for snapshot assertions.
//! After the canonical event stream change, tests send `StreamMessage` events
//! instead of raw `JsonLine` events.

use aikit_sdk::{
    AgentEvent, AgentEventPayload, AgentEventStream, MessageKind, MessagePhase, MessageRole,
    ProgressViewConfig, RunProgress, StreamMessage, TokenUsage, UsageSource,
};

// -------------------------------------------------------------------------
// Helper constructors
// -------------------------------------------------------------------------

fn stream_event(text: &str, role: MessageRole, kind: MessageKind) -> AgentEvent {
    AgentEvent {
        agent_key: "opencode".to_string(),
        seq: 0,
        stream: AgentEventStream::Stdout,
        payload: AgentEventPayload::StreamMessage(StreamMessage {
            text: text.to_string(),
            phase: MessagePhase::Delta,
            role,
            kind,
            source: AgentEventStream::Stdout,
            raw_line_seq: 0,
            turn_id: None,
        }),
    }
}

fn token_event(agent_key: &str, input: u64, output: u64, total: u64) -> AgentEvent {
    AgentEvent {
        agent_key: agent_key.to_string(),
        seq: 1,
        stream: AgentEventStream::Stdout,
        payload: AgentEventPayload::TokenUsageLine {
            usage: TokenUsage {
                input_tokens: input,
                output_tokens: output,
                total_tokens: Some(total),
                cache_read_tokens: None,
                cache_creation_tokens: None,
                reasoning_tokens: None,
            },
            source: UsageSource::OpenCode,
            raw_agent_line_seq: 0,
        },
    }
}

// -------------------------------------------------------------------------
// Assistant text classification snapshots
// -------------------------------------------------------------------------

#[test]
fn test_opencode_text_event_snapshot() {
    let mut progress = RunProgress::new(ProgressViewConfig::default());
    let event = stream_event(
        "Hello from the agent",
        MessageRole::Assistant,
        MessageKind::Message,
    );
    progress.push("opencode", &event);
    let lines: Vec<_> = progress.formatted_lines().collect();
    insta::assert_debug_snapshot!(lines);
}

#[test]
fn test_opencode_step_start_suppressed_snapshot() {
    let mut progress = RunProgress::new(ProgressViewConfig::default());
    let val: serde_json::Value =
        serde_json::from_str(r#"{"type":"step_start","timestamp":1234567890}"#).unwrap();
    let event = AgentEvent {
        agent_key: "opencode".to_string(),
        seq: 0,
        stream: AgentEventStream::Stdout,
        payload: AgentEventPayload::JsonLine(val),
    };
    progress.push("opencode", &event);
    let lines: Vec<_> = progress.formatted_lines().collect();
    insta::assert_debug_snapshot!(lines);
}

#[test]
fn test_opencode_tool_use_success_snapshot() {
    let mut progress = RunProgress::new(ProgressViewConfig::default());
    let event = stream_event(
        "total 8\ndrwxr-xr-x 2 user user 4096 Jan 1 00:00 .",
        MessageRole::Tool,
        MessageKind::ToolOutput,
    );
    progress.push("opencode", &event);
    let lines: Vec<_> = progress.formatted_lines().collect();
    insta::assert_debug_snapshot!(lines);
}

#[test]
fn test_opencode_tool_use_failure_snapshot() {
    let mut progress = RunProgress::new(ProgressViewConfig::default());
    let event = stream_event(
        "cat: /nonexistent: No such file or directory",
        MessageRole::Tool,
        MessageKind::ToolOutput,
    );
    progress.push("opencode", &event);
    let lines: Vec<_> = progress.formatted_lines().collect();
    insta::assert_debug_snapshot!(lines);
}

#[test]
fn test_opencode_tool_use_no_exit_snapshot() {
    let mut progress = RunProgress::new(ProgressViewConfig::default());
    let event = stream_event("myhost", MessageRole::Tool, MessageKind::ToolOutput);
    progress.push("opencode", &event);
    let lines: Vec<_> = progress.formatted_lines().collect();
    insta::assert_debug_snapshot!(lines);
}

#[test]
fn test_opencode_step_finish_stop_suppressed_snapshot() {
    let mut progress = RunProgress::new(ProgressViewConfig::default());
    let val: serde_json::Value =
        serde_json::from_str(r#"{"type":"step_finish","part":{"reason":"stop"}}"#).unwrap();
    let event = AgentEvent {
        agent_key: "opencode".to_string(),
        seq: 0,
        stream: AgentEventStream::Stdout,
        payload: AgentEventPayload::JsonLine(val),
    };
    progress.push("opencode", &event);
    let lines: Vec<_> = progress.formatted_lines().collect();
    insta::assert_debug_snapshot!(lines);
}

#[test]
fn test_opencode_step_finish_error_shown_snapshot() {
    let mut progress = RunProgress::new(ProgressViewConfig::default());
    let event = stream_event("error", MessageRole::System, MessageKind::Status);
    progress.push("opencode", &event);
    let lines: Vec<_> = progress.formatted_lines().collect();
    insta::assert_debug_snapshot!(lines);
}

// -------------------------------------------------------------------------
// tool(invalid) suppression snapshot — now handled by normalizer
// -------------------------------------------------------------------------

#[test]
fn test_opencode_tool_use_invalid_suppressed_snapshot() {
    let mut progress = RunProgress::new(ProgressViewConfig::default());
    let val: serde_json::Value = serde_json::from_str(
        r#"{"type":"tool_use","part":{"tool":"invalid","input":{},"output":""}}"#,
    )
    .unwrap();
    let event = AgentEvent {
        agent_key: "opencode".to_string(),
        seq: 0,
        stream: AgentEventStream::Stdout,
        payload: AgentEventPayload::JsonLine(val),
    };
    progress.push("opencode", &event);
    let lines: Vec<_> = progress.formatted_lines().collect();
    insta::assert_debug_snapshot!(lines);
}

// -------------------------------------------------------------------------
// Token usage footer snapshots
// -------------------------------------------------------------------------

#[test]
fn test_token_footer_with_usage_snapshot() {
    let mut progress = RunProgress::new(ProgressViewConfig::default());
    let event = token_event("opencode", 1000, 250, 1250);
    progress.push("opencode", &event);
    let footer = progress.token_footer();
    insta::assert_debug_snapshot!(footer);
}

#[test]
fn test_token_footer_disabled_snapshot() {
    let config = ProgressViewConfig {
        show_tokens: false,
        ..Default::default()
    };
    let mut progress = RunProgress::new(config);
    let event = token_event("opencode", 1000, 250, 1250);
    progress.push("opencode", &event);
    let footer = progress.token_footer();
    insta::assert_debug_snapshot!(footer);
}

// -------------------------------------------------------------------------
// Ring buffer overflow snapshot
// -------------------------------------------------------------------------

#[test]
fn test_ring_buffer_overflow_snapshot() {
    let config = ProgressViewConfig {
        max_rows: 3,
        ..Default::default()
    };
    let mut progress = RunProgress::new(config);
    for i in 0..6u32 {
        let event = stream_event(
            &format!("message number {i}"),
            MessageRole::Assistant,
            MessageKind::Message,
        );
        progress.push("opencode", &event);
    }
    let lines: Vec<_> = progress.formatted_lines().collect();
    insta::assert_debug_snapshot!(lines);
}

// -------------------------------------------------------------------------
// Fallback classification for non-OpenCode agents
// -------------------------------------------------------------------------

#[test]
fn test_non_opencode_agent_fallback_snapshot() {
    let mut progress = RunProgress::new(ProgressViewConfig::default());
    let event = stream_event("Hello", MessageRole::Assistant, MessageKind::Message);
    progress.push("claude", &event);
    let lines: Vec<_> = progress.formatted_lines().collect();
    insta::assert_debug_snapshot!(lines);
}

#[test]
fn test_raw_line_stdout_snapshot() {
    let mut progress = RunProgress::new(ProgressViewConfig::default());
    let event = AgentEvent {
        agent_key: "codex".to_string(),
        seq: 0,
        stream: AgentEventStream::Stdout,
        payload: AgentEventPayload::RawLine("plain text output".to_string()),
    };
    progress.push("codex", &event);
    let lines: Vec<_> = progress.formatted_lines().collect();
    insta::assert_debug_snapshot!(lines);
}

#[test]
fn test_raw_line_stderr_snapshot() {
    let mut progress = RunProgress::new(ProgressViewConfig::default());
    let event = AgentEvent {
        agent_key: "codex".to_string(),
        seq: 0,
        stream: AgentEventStream::Stderr,
        payload: AgentEventPayload::RawLine("error output line".to_string()),
    };
    progress.push("codex", &event);
    let lines: Vec<_> = progress.formatted_lines().collect();
    insta::assert_debug_snapshot!(lines);
}

// -------------------------------------------------------------------------
// Multiple events in sequence
// -------------------------------------------------------------------------

#[test]
fn test_multiple_events_sequence_snapshot() {
    let mut progress = RunProgress::new(ProgressViewConfig::default());

    let events = vec![
        stream_event(
            "Starting analysis",
            MessageRole::Assistant,
            MessageKind::Message,
        ),
        stream_event("/home/user", MessageRole::Tool, MessageKind::ToolOutput),
        stream_event("Done.", MessageRole::Assistant, MessageKind::Message),
    ];

    for event in &events {
        progress.push("opencode", event);
    }

    let lines: Vec<_> = progress.formatted_lines().collect();
    insta::assert_debug_snapshot!(lines);
}

// -------------------------------------------------------------------------
// Clear behaviour
// -------------------------------------------------------------------------

#[test]
fn test_clear_resets_state_snapshot() {
    let mut progress = RunProgress::new(ProgressViewConfig::default());
    let event = stream_event("before clear", MessageRole::Assistant, MessageKind::Message);
    progress.push("opencode", &event);
    let token_ev = token_event("opencode", 100, 50, 150);
    progress.push("opencode", &token_ev);

    progress.clear();

    let lines: Vec<_> = progress.formatted_lines().collect();
    let footer = progress.token_footer();
    insta::assert_debug_snapshot!((lines, footer));
}
