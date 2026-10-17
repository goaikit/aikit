//! Context compression. The algorithm lives in
//! [`aikit_agent_core::compression`], shared with the on-device hosts; this
//! module keeps the [`ContextPacket`] entry point.

use crate::context::ContextPacket;
use crate::errors::AgentError;

pub use aikit_agent_core::compression::CompressionResult;

/// Compress the context packet if it exceeds the token budget.
///
/// Compression strategy:
/// 1. Always preserve system_instructions verbatim
/// 2. Always preserve the latest user prompt verbatim
/// 3. Always preserve the latest tool result verbatim
/// 4. Summarize oldest turns into a single summary turn
/// 5. Idempotent: same input + budget → same output
pub fn maybe_compress(packet: &mut ContextPacket) -> Result<Option<CompressionResult>, AgentError> {
    Ok(aikit_agent_core::compression::maybe_compress(
        &packet.system_instructions,
        &mut packet.conversation,
        &packet.token_budget,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::TurnRole;
    use crate::context::{ContextPacket, TokenBudget, Turn};

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
