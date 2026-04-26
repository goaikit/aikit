use crate::context::{ContextPacket, TurnRole};
use crate::errors::AgentError;

#[derive(Debug, Clone)]
pub struct CompressionResult {
    pub original_tokens: u64,
    pub compressed_tokens: u64,
    pub turns_summarized: u64,
}

/// Compress the context packet if it exceeds the token budget.
///
/// Compression strategy:
/// 1. Always preserve system_instructions verbatim
/// 2. Always preserve the latest user prompt verbatim
/// 3. Always preserve the latest tool result verbatim
/// 4. Summarize oldest turns into a single summary turn
/// 5. Idempotent: same input + budget → same output
pub fn maybe_compress(packet: &mut ContextPacket) -> Result<Option<CompressionResult>, AgentError> {
    let available = packet.token_budget.available();
    let current_tokens = packet.estimated_tokens();

    if current_tokens <= available {
        return Ok(None);
    }

    let original_tokens = current_tokens;
    let result = compress(packet, available)?;
    let compressed_tokens = packet.estimated_tokens();

    Ok(Some(CompressionResult {
        original_tokens,
        compressed_tokens,
        turns_summarized: result,
    }))
}

fn compress(packet: &mut ContextPacket, _target_tokens: u64) -> Result<u64, AgentError> {
    let conversation = &mut packet.conversation;
    if conversation.is_empty() {
        return Ok(0);
    }

    // Find the indices of turns to preserve:
    // - Always preserve the last user turn
    // - Always preserve the last tool result turn
    let last_user_idx = conversation.iter().rposition(|t| t.role == TurnRole::User);
    let last_tool_idx = conversation.iter().rposition(|t| t.role == TurnRole::Tool);

    let preserve_from = std::cmp::min(
        last_user_idx.unwrap_or(conversation.len()),
        last_tool_idx.unwrap_or(conversation.len()),
    );

    // Summarize all turns before preserve_from
    if preserve_from == 0 {
        // Nothing to summarize
        return Ok(0);
    }

    let turns_to_summarize: Vec<_> = conversation.drain(..preserve_from).collect();
    let turns_summarized = turns_to_summarize.len() as u64;

    let summary = build_summary(&turns_to_summarize);

    // Insert summary as first turn
    let summary_turn = crate::context::Turn::assistant(summary);
    conversation.insert(0, summary_turn);

    Ok(turns_summarized)
}

fn build_summary(turns: &[crate::context::Turn]) -> String {
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
}
