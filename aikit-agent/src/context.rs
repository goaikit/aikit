//! Conversation turns and token budgets. The turns live in
//! [`aikit_agent_core::context`], shared with the on-device hosts; the
//! [`ContextPacket`], which carries the native skill catalog, stays here.

use crate::skills::SkillMetadata;

pub use aikit_agent_core::context::{
    estimate_tokens, estimate_turn_tokens, ContextToolCall, ContextToolResult, TokenBudget, Turn,
    TurnRole,
};

#[derive(Debug, Clone)]
pub struct ContextPacket {
    pub system_instructions: String,
    pub conversation: Vec<Turn>,
    pub skills_summary: Vec<SkillMetadata>,
    pub token_budget: TokenBudget,
}

impl ContextPacket {
    pub fn new(system_instructions: String, token_budget: TokenBudget) -> Self {
        Self {
            system_instructions,
            conversation: Vec::new(),
            skills_summary: Vec::new(),
            token_budget,
        }
    }

    pub fn add_turn(&mut self, turn: Turn) {
        self.conversation.push(turn);
    }

    /// Estimate the total token count for the context packet.
    pub fn estimated_tokens(&self) -> u64 {
        aikit_agent_core::context::estimate_context_tokens(
            &self.system_instructions,
            &self.conversation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_budget_available() {
        let budget = TokenBudget {
            total_budget: 12000,
            reserve_for_tools: 1000,
            reserve_for_output: 2000,
        };
        assert_eq!(budget.available(), 9000);
    }

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_basic() {
        // 3 words → 3 + 1 = 4 tokens
        let tokens = estimate_tokens("hello world foo");
        assert!(tokens > 0);
    }

    #[test]
    fn test_context_packet_estimated_tokens() {
        let mut packet = ContextPacket::new(
            "System: you are helpful.".to_string(),
            TokenBudget::default(),
        );
        packet.add_turn(Turn::user("What is Rust?"));
        packet.add_turn(Turn::assistant("Rust is a systems programming language."));
        let tokens = packet.estimated_tokens();
        assert!(tokens > 0);
    }

    #[test]
    fn test_turn_role_serde_round_trip() {
        let roles = [TurnRole::User, TurnRole::Assistant, TurnRole::Tool];
        for role in &roles {
            let json = serde_json::to_string(role).unwrap();
            let back: TurnRole = serde_json::from_str(&json).unwrap();
            assert_eq!(role, &back);
        }
    }

    #[test]
    fn test_turn_serde_round_trip() {
        let turn = Turn {
            role: TurnRole::User,
            content: "hello".to_string(),
            tool_calls: None,
            tool_results: None,
        };
        let json = serde_json::to_string(&turn).unwrap();
        let back: Turn = serde_json::from_str(&json).unwrap();
        assert_eq!(back.content, "hello");
        assert!(matches!(back.role, TurnRole::User));
    }

    #[test]
    fn test_context_tool_call_serde_round_trip() {
        let call = ContextToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"foo"}"#.to_string(),
        };
        let json = serde_json::to_string(&call).unwrap();
        let back: ContextToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "call-1");
        assert_eq!(back.name, "read_file");
    }

    #[test]
    fn test_context_tool_result_serde_round_trip() {
        let result = ContextToolResult {
            call_id: "call-1".to_string(),
            output: "file content".to_string(),
            is_error: false,
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: ContextToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.call_id, "call-1");
        assert!(!back.is_error);
    }
}
