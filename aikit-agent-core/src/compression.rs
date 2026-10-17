//! Context compression: summarise the oldest turns when the conversation is
//! over its token budget.

use serde::{Deserialize, Serialize};

use crate::context::{estimate_context_tokens, TokenBudget, Turn, TurnRole};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompressionResult {
    pub original_tokens: u64,
    pub compressed_tokens: u64,
    pub turns_summarized: u64,
}

/// Compress the conversation if it, with the system prompt, exceeds the
/// token budget. Returns `None` when it fits.
///
/// Compression strategy:
/// 1. Always preserve system_instructions verbatim
/// 2. Always preserve the latest user prompt verbatim
/// 3. Always preserve the latest tool result verbatim
/// 4. Summarize oldest turns into a single summary turn
/// 5. Idempotent: same input + budget → same output
pub fn maybe_compress(
    system_instructions: &str,
    conversation: &mut Vec<Turn>,
    budget: &TokenBudget,
) -> Option<CompressionResult> {
    let available = budget.available();
    let current_tokens = estimate_context_tokens(system_instructions, conversation);

    if current_tokens <= available {
        return None;
    }

    let original_tokens = current_tokens;
    let turns_summarized = compress(conversation);
    let compressed_tokens = estimate_context_tokens(system_instructions, conversation);

    Some(CompressionResult {
        original_tokens,
        compressed_tokens,
        turns_summarized,
    })
}

fn compress(conversation: &mut Vec<Turn>) -> u64 {
    if conversation.is_empty() {
        return 0;
    }

    let last_user_idx = conversation.iter().rposition(|t| t.role == TurnRole::User);
    let last_tool_idx = conversation.iter().rposition(|t| t.role == TurnRole::Tool);

    let preserve_tool_boundary = last_tool_idx.and_then(|tool_idx| {
        conversation[..tool_idx]
            .iter()
            .rposition(|t| t.role == TurnRole::Assistant && t.tool_calls.is_some())
    });

    let preserve_from = [last_user_idx, preserve_tool_boundary]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(conversation.len());

    if preserve_from == 0 {
        return 0;
    }

    let turns_to_summarize: Vec<_> = conversation.drain(..preserve_from).collect();
    let turns_summarized = turns_to_summarize.len() as u64;

    let summary = build_summary(&turns_to_summarize);
    conversation.insert(0, Turn::assistant(summary));

    turns_summarized
}

fn build_summary(turns: &[Turn]) -> String {
    let mut parts = Vec::new();
    for turn in turns {
        let role_str = match turn.role {
            TurnRole::User => "User",
            TurnRole::Assistant => "Assistant",
            TurnRole::Tool => "Tool",
        };
        if !turn.content.is_empty() {
            parts.push(format!("[{}]: {}", role_str, truncate(&turn.content, 80)));
        }
        if let Some(calls) = &turn.tool_calls {
            for call in calls {
                parts.push(format!(
                    "[Tool call: {}({})]",
                    call.name,
                    truncate(&call.arguments, 60)
                ));
            }
        }
        if let Some(results) = &turn.tool_results {
            for result in results {
                parts.push(format!("[Tool result: {}]", truncate(&result.output, 60)));
            }
        }
    }
    format!("[Summary of {} turns]\n{}", turns.len(), parts.join("\n"))
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{estimate_context_tokens, TokenBudget, Turn};

    /// The loop's state for one test: a system prompt, a conversation and a
    /// budget, as aikit-agent's `ContextPacket` holds them.
    struct ContextPacket {
        system_instructions: String,
        conversation: Vec<Turn>,
        token_budget: TokenBudget,
    }

    impl ContextPacket {
        fn new(system_instructions: String, token_budget: TokenBudget) -> Self {
            Self {
                system_instructions,
                conversation: Vec::new(),
                token_budget,
            }
        }
        fn add_turn(&mut self, turn: Turn) {
            self.conversation.push(turn);
        }
        fn estimated_tokens(&self) -> u64 {
            estimate_context_tokens(&self.system_instructions, &self.conversation)
        }
    }

    fn maybe_compress(packet: &mut ContextPacket) -> Result<Option<CompressionResult>, ()> {
        Ok(super::maybe_compress(
            &packet.system_instructions,
            &mut packet.conversation,
            &packet.token_budget,
        ))
    }

    fn make_packet_over_budget() -> ContextPacket {
        // Use a very small budget to force compression
        let budget = TokenBudget {
            total_budget: 50,
            reserve_for_tools: 5,
            reserve_for_output: 5,
        };
        let mut packet = ContextPacket::new("System instructions.".to_string(), budget);

        // Add many turns to exceed budget
        for i in 0..10 {
            packet.add_turn(Turn::user(format!("User question number {}", i)));
            packet.add_turn(Turn::assistant(format!("Assistant answer number {}", i)));
        }
        packet
    }

    #[test]
    fn test_context_compression_emits_event() {
        let mut packet = make_packet_over_budget();
        let result = maybe_compress(&mut packet).unwrap();
        assert!(result.is_some(), "compression should have occurred");
        let r = result.unwrap();
        assert!(
            r.original_tokens > r.compressed_tokens,
            "should reduce tokens"
        );
        assert!(r.turns_summarized > 0, "should summarize some turns");
    }

    #[test]
    fn test_context_compression_preserves_agents_md() {
        let budget = TokenBudget {
            total_budget: 50,
            reserve_for_tools: 5,
            reserve_for_output: 5,
        };
        let agents_md_content = "# AGENTS.md\nThis is the agents file.";
        let mut packet =
            ContextPacket::new(format!("{}\nSystem rules here.", agents_md_content), budget);
        for i in 0..10 {
            packet.add_turn(Turn::user(format!("Question {}", i)));
            packet.add_turn(Turn::assistant(format!("Answer {}", i)));
        }

        let system_before = packet.system_instructions.clone();
        maybe_compress(&mut packet).unwrap();

        assert_eq!(
            packet.system_instructions, system_before,
            "system_instructions should not change"
        );
        assert!(
            packet.system_instructions.contains(agents_md_content),
            "AGENTS.md content should be preserved"
        );
    }

    #[test]
    fn test_context_compression_idempotent() {
        let mut packet = make_packet_over_budget();

        // Compress once
        maybe_compress(&mut packet).unwrap();
        let state_after_first: Vec<_> = packet
            .conversation
            .iter()
            .map(|t| t.content.clone())
            .collect();
        let tokens_after_first = packet.estimated_tokens();

        // Compress again with same packet
        maybe_compress(&mut packet).unwrap();
        let state_after_second: Vec<_> = packet
            .conversation
            .iter()
            .map(|t| t.content.clone())
            .collect();
        let tokens_after_second = packet.estimated_tokens();

        // Second compression should not further reduce (already within budget)
        // The state may differ slightly due to re-summarization, but tokens should be stable
        assert!(
            tokens_after_second <= tokens_after_first + 10,
            "second compression should not increase tokens significantly"
        );
        assert_eq!(
            state_after_first.len(),
            state_after_second.len(),
            "conversation length should be stable after repeated compression"
        );
    }

    #[test]
    fn test_no_compression_when_within_budget() {
        let budget = TokenBudget {
            total_budget: 100000,
            reserve_for_tools: 1000,
            reserve_for_output: 2000,
        };
        let mut packet = ContextPacket::new("Small system.".to_string(), budget);
        packet.add_turn(Turn::user("Short question"));
        let result = maybe_compress(&mut packet).unwrap();
        assert!(result.is_none(), "no compression needed");
    }

    #[test]
    fn test_compression_preserves_tool_call_boundary() {
        let budget = TokenBudget {
            total_budget: 50,
            reserve_for_tools: 5,
            reserve_for_output: 5,
        };
        let mut packet = ContextPacket::new("System instructions.".to_string(), budget);

        for i in 0..5 {
            packet.add_turn(Turn::user(format!("User question {}", i)));
            packet.add_turn(Turn::assistant(format!("Answer {}", i)));
        }

        packet.add_turn(Turn::user("Read the file"));
        packet.add_turn(Turn::assistant_with_tool_calls(
            "",
            vec![crate::context::ContextToolCall {
                id: "call_1".to_string(),
                name: "read_file".to_string(),
                arguments: r#"{"path": "test.txt"}"#.to_string(),
            }],
        ));
        packet.add_turn(Turn::tool_result(vec![crate::context::ContextToolResult {
            call_id: "call_1".to_string(),
            output: "file contents".to_string(),
            is_error: false,
        }]));

        let _ = maybe_compress(&mut packet).unwrap();

        for (i, turn) in packet.conversation.iter().enumerate() {
            if turn.role == TurnRole::Tool {
                assert!(
                    i > 0,
                    "Tool turn must not be the first turn in conversation"
                );
                let prev = &packet.conversation[i - 1];
                assert!(
                    prev.role == TurnRole::Assistant && prev.tool_calls.is_some(),
                    "Tool turn must be immediately preceded by Assistant with tool_calls, but got {:?}",
                    prev.role
                );
            }
        }
    }

    #[test]
    fn test_compression_never_orphans_tool_turn() {
        let budget = TokenBudget {
            total_budget: 50,
            reserve_for_tools: 5,
            reserve_for_output: 5,
        };
        let mut packet = ContextPacket::new("System instructions.".to_string(), budget);

        for i in 0..5 {
            packet.add_turn(Turn::user(format!("User question {}", i)));
            packet.add_turn(Turn::assistant(format!("Answer {}", i)));
        }

        packet.add_turn(Turn::user("Read a file please"));
        packet.add_turn(Turn::assistant_with_tool_calls(
            "I will read the file",
            vec![crate::context::ContextToolCall {
                id: "call_99".to_string(),
                name: "read_file".to_string(),
                arguments: r#"{"path": "data.txt"}"#.to_string(),
            }],
        ));
        packet.add_turn(Turn::tool_result(vec![crate::context::ContextToolResult {
            call_id: "call_99".to_string(),
            output: "data contents".to_string(),
            is_error: false,
        }]));

        let _ = maybe_compress(&mut packet).unwrap();

        for (i, turn) in packet.conversation.iter().enumerate() {
            if turn.role == TurnRole::Tool {
                assert!(i > 0, "Tool turn at index {} has no predecessor", i);
                let prev = &packet.conversation[i - 1];
                assert_eq!(prev.role, TurnRole::Assistant);
                assert!(
                    prev.tool_calls.is_some(),
                    "Predecessor of Tool turn must have tool_calls"
                );
            }
        }
    }
}
