use super::types::{AgentEventStream, MessageKind, MessagePhase, MessageRole, StreamMessage};

pub fn normalize_json_line(
    agent_key: &str,
    stream: AgentEventStream,
    value: &serde_json::Value,
    raw_line_seq: u64,
) -> Vec<StreamMessage> {
    let known = ["codex", "claude", "gemini", "opencode", "agent"];
    if !known.contains(&agent_key) {
        tracing::warn!(
            target: "aikit_sdk::runner::normalize",
            agent_key = %agent_key,
            "E_NORMALIZE_UNKNOWN_AGENT: unknown agent key"
        );
        return Vec::new();
    }

    let messages = match agent_key {
        "codex" => normalize_codex(value, stream, raw_line_seq),
        "claude" => normalize_claude(value, stream, raw_line_seq),
        "gemini" => normalize_gemini(value, stream, raw_line_seq),
        "opencode" => normalize_opencode(value, stream, raw_line_seq),
        "agent" => normalize_agent(value, stream, raw_line_seq),
        _ => Vec::new(),
    };

    let filtered: Vec<StreamMessage> = messages
        .into_iter()
        .filter(|m| {
            if m.text.trim().is_empty() {
                tracing::debug!(
                    target: "aikit_sdk::runner::normalize",
                    "E_NORMALIZE_EMPTY_TEXT: matched rule but text is empty"
                );
                false
            } else {
                true
            }
        })
        .collect();

    tracing::debug!(
        target: "aikit_sdk::runner::normalize",
        agent_key = %agent_key,
        count = filtered.len(),
        unmapped = filtered.is_empty() && !value.as_object().map_or(true, |o| o.is_empty()),
        "normalized json line"
    );

    filtered
}

pub(super) fn normalize_codex(
    value: &serde_json::Value,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> Vec<StreamMessage> {
    let mut results = Vec::new();
    let line_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    let mk = |text: String, role: MessageRole, kind: MessageKind| StreamMessage {
        text,
        phase: MessagePhase::Final,
        role,
        kind,
        source: stream,
        raw_line_seq,
        turn_id: None,
    };

    match line_type {
        // ── Current codex-cli "thread/turn/item" schema (>= 0.13x) ──────────────
        // Emit on terminal item state only (`item.completed`) to avoid duplicating
        // the streamed `item.started` event for the same item.
        "item.completed" => {
            if let Some(item) = value.get("item") {
                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match item_type {
                    "agent_message" => {
                        if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                            results.push(mk(
                                t.to_string(),
                                MessageRole::Assistant,
                                MessageKind::Message,
                            ));
                        }
                    }
                    "reasoning" => {
                        if let Some(t) = item
                            .get("text")
                            .and_then(|v| v.as_str())
                            .or_else(|| item.get("summary").and_then(|v| v.as_str()))
                        {
                            results.push(mk(
                                t.to_string(),
                                MessageRole::Assistant,
                                MessageKind::Reasoning,
                            ));
                        }
                    }
                    "command_execution" => {
                        if let Some(cmd) = item.get("command").and_then(|v| v.as_str()) {
                            results.push(mk(
                                cmd.to_string(),
                                MessageRole::Tool,
                                MessageKind::Message,
                            ));
                        }
                        if let Some(out) = item.get("aggregated_output").and_then(|v| v.as_str()) {
                            if !out.trim().is_empty() {
                                results.push(mk(
                                    out.to_string(),
                                    MessageRole::Tool,
                                    MessageKind::ToolOutput,
                                ));
                            }
                        }
                    }
                    "file_change" => {
                        if let Some(arr) = item.get("changes").and_then(|c| c.as_array()) {
                            let summary = arr
                                .iter()
                                .filter_map(|c| {
                                    let path = c.get("path").and_then(|v| v.as_str())?;
                                    let kind =
                                        c.get("kind").and_then(|v| v.as_str()).unwrap_or("change");
                                    Some(format!("{kind} {path}"))
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            if !summary.is_empty() {
                                results.push(mk(
                                    format!("file_change: {summary}"),
                                    MessageRole::Tool,
                                    MessageKind::Message,
                                ));
                            }
                        }
                    }
                    // Unknown item type: surface any text it carries.
                    _ => {
                        if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                            results.push(mk(
                                t.to_string(),
                                MessageRole::Assistant,
                                MessageKind::Message,
                            ));
                        }
                    }
                }
            }
        }
        // ── Failure events — surface so a failed turn is never a silent empty run ──
        "error" => {
            if let Some(msg) = value.get("message").and_then(|v| v.as_str()) {
                results.push(mk(
                    msg.to_string(),
                    MessageRole::System,
                    MessageKind::Status,
                ));
            }
        }
        "turn.failed" => {
            if let Some(msg) = value
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|v| v.as_str())
            {
                results.push(mk(
                    msg.to_string(),
                    MessageRole::System,
                    MessageKind::Status,
                ));
            }
        }
        // ── Lifecycle frames carry no message text — intentionally ignored ──────
        "thread.started" | "turn.started" | "turn.completed" | "item.started" => {}
        // ── Legacy codex schema (older CLI): message / action / output ──────────
        "message" => {
            let role_str = value.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(content) = value.get("content").and_then(|v| v.as_str()) {
                let role = match role_str {
                    "system" => MessageRole::System,
                    "user" => MessageRole::User,
                    _ => MessageRole::Assistant,
                };
                let kind = if role_str == "system" {
                    MessageKind::Status
                } else {
                    MessageKind::Message
                };
                results.push(mk(content.to_string(), role, kind));
            }
        }
        "action" => {
            if let Some(cmd) = value.get("command").and_then(|v| v.as_str()) {
                results.push(mk(cmd.to_string(), MessageRole::Tool, MessageKind::Message));
            }
        }
        "output" => {
            if let Some(stdout) = value.get("stdout").and_then(|v| v.as_str()) {
                results.push(mk(
                    stdout.to_string(),
                    MessageRole::Tool,
                    MessageKind::ToolOutput,
                ));
            }
        }
        // ── Unknown line type: legacy fallback for a top-level `item.text` ──────
        _ => {
            if let Some(text) = value
                .get("item")
                .and_then(|item| item.get("text"))
                .and_then(|v| v.as_str())
            {
                results.push(mk(
                    text.to_string(),
                    MessageRole::Assistant,
                    MessageKind::Message,
                ));
            }
        }
    }

    results
}

pub(super) fn normalize_claude(
    value: &serde_json::Value,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> Vec<StreamMessage> {
    let mut results = Vec::new();
    let line_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if line_type == "assistant" {
        if let Some(content) = value.get("message").and_then(|m| m.get("content")) {
            if let Some(arr) = content.as_array() {
                for item in arr {
                    if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                            results.push(StreamMessage {
                                text: text.to_string(),
                                phase: MessagePhase::Delta,
                                role: MessageRole::Assistant,
                                kind: MessageKind::Message,
                                source: stream,
                                raw_line_seq,
                                turn_id: None,
                            });
                        }
                    }
                }
            }
        }
    }

    if line_type == "result" {
        if let Some(result_text) = value.get("result").and_then(|v| v.as_str()) {
            let turn_id = value
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            results.push(StreamMessage {
                text: result_text.to_string(),
                phase: MessagePhase::Final,
                role: MessageRole::Assistant,
                kind: MessageKind::Message,
                source: stream,
                raw_line_seq,
                turn_id,
            });
        }
    }

    results
}

pub(super) fn normalize_gemini(
    value: &serde_json::Value,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> Vec<StreamMessage> {
    let mut results = Vec::new();
    let line_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    // Current gemini CLI stream-json shape:
    //   {"type":"message","role":"assistant","content":"...","delta":true}
    //   {"type":"message","role":"assistant","content":"..."}                  (final)
    //   {"type":"result","stats":{...}}                                        (run done)
    //   {"type":"init","session_id":"..."}                                     (ignored)
    //   {"type":"message","role":"user","content":"..."}                       (echo, skip)
    if line_type == "message" && value.get("role").and_then(|v| v.as_str()) == Some("assistant") {
        if let Some(text) = value.get("content").and_then(|v| v.as_str()) {
            let is_delta = value
                .get("delta")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let turn_id = value
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            results.push(StreamMessage {
                text: text.to_string(),
                phase: if is_delta {
                    MessagePhase::Delta
                } else {
                    MessagePhase::Final
                },
                role: MessageRole::Assistant,
                kind: MessageKind::Message,
                source: stream,
                raw_line_seq,
                turn_id,
            });
        }
    }

    // Legacy/alternative gemini shape (Gemini API direct):
    //   {"candidates":[{"content":{"parts":[{"text":"..."}]}}]}
    if let Some(candidates) = value.get("candidates").and_then(|v| v.as_array()) {
        for candidate in candidates {
            if let Some(parts) = candidate
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
            {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        results.push(StreamMessage {
                            text: text.to_string(),
                            phase: MessagePhase::Delta,
                            role: MessageRole::Assistant,
                            kind: MessageKind::Message,
                            source: stream,
                            raw_line_seq,
                            turn_id: None,
                        });
                    }
                }
            }
        }
    }

    // Original `{"type":"result","result":"..."}` shape (some gemini versions)
    if line_type == "result" {
        if let Some(result_text) = value.get("result").and_then(|v| v.as_str()) {
            results.push(StreamMessage {
                text: result_text.to_string(),
                phase: MessagePhase::Final,
                role: MessageRole::Assistant,
                kind: MessageKind::Message,
                source: stream,
                raw_line_seq,
                turn_id: None,
            });
        }
    }

    results
}

pub(super) fn normalize_opencode(
    value: &serde_json::Value,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> Vec<StreamMessage> {
    let mut results = Vec::new();
    let event_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if event_type == "text" {
        if let Some(text) = value
            .get("part")
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
        {
            results.push(StreamMessage {
                text: text.to_string(),
                phase: MessagePhase::Final,
                role: MessageRole::Assistant,
                kind: MessageKind::Message,
                source: stream,
                raw_line_seq,
                turn_id: None,
            });
        }
    }

    if event_type == "tool_use" {
        if let Some(output) = value
            .get("part")
            .and_then(|p| p.get("output"))
            .and_then(|o| o.as_str())
        {
            results.push(StreamMessage {
                text: output.to_string(),
                phase: MessagePhase::Final,
                role: MessageRole::Tool,
                kind: MessageKind::ToolOutput,
                source: stream,
                raw_line_seq,
                turn_id: None,
            });
        }
    }

    if event_type == "message" {
        let role_str = value.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(content) = value.get("content").and_then(|v| v.as_str()) {
            let role = if role_str == "assistant" {
                MessageRole::Assistant
            } else if role_str == "system" {
                MessageRole::System
            } else {
                MessageRole::Assistant
            };
            results.push(StreamMessage {
                text: content.to_string(),
                phase: MessagePhase::Final,
                role,
                kind: MessageKind::Message,
                source: stream,
                raw_line_seq,
                turn_id: None,
            });
        }
    }

    results
}

pub(super) fn normalize_agent(
    value: &serde_json::Value,
    stream: AgentEventStream,
    raw_line_seq: u64,
) -> Vec<StreamMessage> {
    let mut results = Vec::new();

    let event_key = value
        .get("event")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("type").and_then(|v| v.as_str()))
        .unwrap_or("");

    if event_key == "message" {
        if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
            results.push(StreamMessage {
                text: text.to_string(),
                phase: MessagePhase::Delta,
                role: MessageRole::Assistant,
                kind: MessageKind::Message,
                source: stream,
                raw_line_seq,
                turn_id: None,
            });
        }
    }

    if event_key == "result" {
        if let Some(result_text) = value.get("result").and_then(|v| v.as_str()) {
            results.push(StreamMessage {
                text: result_text.to_string(),
                phase: MessagePhase::Final,
                role: MessageRole::Assistant,
                kind: MessageKind::Message,
                source: stream,
                raw_line_seq,
                turn_id: None,
            });
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::types::{AgentEventStream, MessageKind, MessagePhase, MessageRole};

    #[test]
    fn test_normalize_codex_item_agent_message() {
        // codex-cli >= 0.13x schema: item.completed / item.type=agent_message
        let line = serde_json::json!({
            "type": "item.completed",
            "item": {"id": "item_0", "type": "agent_message", "text": "Done."}
        });
        let out = normalize_codex(&line, AgentEventStream::Stdout, 0);
        assert_eq!(out.len(), 1, "got {:?}", out);
        assert_eq!(out[0].text, "Done.");
        assert_eq!(out[0].role, MessageRole::Assistant);
        assert_eq!(out[0].kind, MessageKind::Message);
        assert_eq!(out[0].phase, MessagePhase::Final);
    }

    #[test]
    fn test_normalize_codex_item_command_execution() {
        let line = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "item_1", "type": "command_execution",
                "command": "ls -la", "aggregated_output": "file.txt\n",
                "exit_code": 0, "status": "completed"
            }
        });
        let out = normalize_codex(&line, AgentEventStream::Stdout, 0);
        assert_eq!(out.len(), 2, "command + output; got {:?}", out);
        assert_eq!(out[0].text, "ls -la");
        assert_eq!(out[0].role, MessageRole::Tool);
        assert_eq!(out[0].kind, MessageKind::Message);
        assert_eq!(out[1].text, "file.txt\n");
        assert_eq!(out[1].kind, MessageKind::ToolOutput);
    }

    #[test]
    fn test_normalize_codex_item_file_change() {
        let line = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "item_2", "type": "file_change", "status": "completed",
                "changes": [{"path": "/tmp/a.md", "kind": "add"}]
            }
        });
        let out = normalize_codex(&line, AgentEventStream::Stdout, 0);
        assert_eq!(out.len(), 1, "got {:?}", out);
        assert_eq!(out[0].text, "file_change: add /tmp/a.md");
        assert_eq!(out[0].role, MessageRole::Tool);
    }

    #[test]
    fn test_normalize_codex_error_and_turn_failed_are_surfaced() {
        // These were previously dropped, making a failed turn an invisible empty run.
        let err = serde_json::json!({"type": "error", "message": "The '' model is not supported"});
        let out = normalize_codex(&err, AgentEventStream::Stdout, 0);
        assert_eq!(out.len(), 1, "error must surface; got {:?}", out);
        assert_eq!(out[0].role, MessageRole::System);
        assert_eq!(out[0].kind, MessageKind::Status);
        assert!(out[0].text.contains("not supported"));

        let failed = serde_json::json!({"type": "turn.failed", "error": {"message": "boom"}});
        let out2 = normalize_codex(&failed, AgentEventStream::Stdout, 0);
        assert_eq!(out2.len(), 1, "turn.failed must surface; got {:?}", out2);
        assert_eq!(out2[0].text, "boom");
    }

    #[test]
    fn test_normalize_codex_lifecycle_frames_ignored() {
        for t in [
            "thread.started",
            "turn.started",
            "turn.completed",
            "item.started",
        ] {
            let line = serde_json::json!({"type": t});
            assert!(
                normalize_codex(&line, AgentEventStream::Stdout, 0).is_empty(),
                "lifecycle frame {t} should be ignored"
            );
        }
    }

    #[test]
    fn test_normalize_codex_legacy_message_shape_still_works() {
        // Older codex schema.
        let line = serde_json::json!({"type":"message","role":"assistant","content":"hi"});
        let out = normalize_codex(&line, AgentEventStream::Stdout, 0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "hi");
    }

    #[test]
    fn test_normalize_gemini_delta_message_is_delta() {
        let line = serde_json::json!({
            "type": "message",
            "role": "assistant",
            "content": "I'm doing well, thank you!",
            "delta": true
        });
        let out = normalize_gemini(&line, AgentEventStream::Stdout, 0);
        assert_eq!(out.len(), 1, "should emit one StreamMessage; got {:?}", out);
        let m = &out[0];
        assert_eq!(m.text, "I'm doing well, thank you!");
        assert_eq!(m.phase, MessagePhase::Delta);
        assert_eq!(m.role, MessageRole::Assistant);
        assert_eq!(m.kind, MessageKind::Message);
    }

    #[test]
    fn test_normalize_gemini_final_message_is_final() {
        // No `delta` key, or `delta:false`, ⇒ Final.
        let line = serde_json::json!({
            "type": "message",
            "role": "assistant",
            "content": "Done.",
        });
        let out = normalize_gemini(&line, AgentEventStream::Stdout, 0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].phase, MessagePhase::Final);

        let line2 = serde_json::json!({
            "type": "message",
            "role": "assistant",
            "content": "Done.",
            "delta": false
        });
        let out2 = normalize_gemini(&line2, AgentEventStream::Stdout, 0);
        assert_eq!(out2[0].phase, MessagePhase::Final);
    }

    #[test]
    fn test_normalize_gemini_user_echo_and_init_are_ignored() {
        let user = serde_json::json!({
            "type": "message",
            "role": "user",
            "content": "Hi, how are you?"
        });
        assert!(normalize_gemini(&user, AgentEventStream::Stdout, 0).is_empty());

        let init = serde_json::json!({"type":"init","session_id":"abc","model":"gemini-3"});
        assert!(normalize_gemini(&init, AgentEventStream::Stdout, 0).is_empty());

        let result_with_stats = serde_json::json!({"type":"result","stats":{"total_tokens":10}});
        // No `result` text → no StreamMessage emitted.
        assert!(normalize_gemini(&result_with_stats, AgentEventStream::Stdout, 0).is_empty());
    }

    #[test]
    fn test_normalize_gemini_legacy_candidates_shape_still_works() {
        let line = serde_json::json!({
            "candidates": [{
                "content": { "parts": [{"text": "hello"}] }
            }]
        });
        let out = normalize_gemini(&line, AgentEventStream::Stdout, 0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "hello");
        assert_eq!(out[0].phase, MessagePhase::Delta);
    }
}
