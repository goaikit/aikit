use crate::skills::SkillMetadata;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnRole {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextToolResult {
    pub call_id: String,
    pub output: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub role: TurnRole,
    pub content: String,
    pub tool_calls: Option<Vec<ContextToolCall>>,
    pub tool_results: Option<Vec<ContextToolResult>>,
}

impl Turn {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: TurnRole::User,
            content: content.into(),
            tool_calls: None,
            tool_results: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: TurnRole::Assistant,
            content: content.into(),
            tool_calls: None,
            tool_results: None,
        }
    }

    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        calls: Vec<ContextToolCall>,
    ) -> Self {
        Self {
            role: TurnRole::Assistant,
            content: content.into(),
            tool_calls: Some(calls),
            tool_results: None,
        }
    }

    pub fn tool_result(results: Vec<ContextToolResult>) -> Self {
        Self {
            role: TurnRole::Tool,
            content: String::new(),
            tool_calls: None,
            tool_results: Some(results),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TokenBudget {
    pub total_budget: u64,
    pub reserve_for_tools: u64,
    pub reserve_for_output: u64,
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self {
            total_budget: 12000,
            reserve_for_tools: 1000,
            reserve_for_output: 2000,
        }
    }
}

impl TokenBudget {
    pub fn available(&self) -> u64 {
        self.total_budget
            .saturating_sub(self.reserve_for_tools)
            .saturating_sub(self.reserve_for_output)
    }
}

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
        estimate_tokens(&self.system_instructions)
            + self
                .conversation
                .iter()
                .map(estimate_turn_tokens)
                .sum::<u64>()
    }
}

/// Rough token estimate: whitespace-split word count.
pub fn estimate_tokens(text: &str) -> u64 {
    let words = text.split_whitespace().count() as u64;
    // Add a small overhead factor for tokenization overhead
    words.saturating_add(words / 3)
}

pub fn estimate_turn_tokens(turn: &Turn) -> u64 {
    let mut total = estimate_tokens(&turn.content);
    if let Some(calls) = &turn.tool_calls {
        for call in calls {
            total += estimate_tokens(&call.name) + estimate_tokens(&call.arguments);
        }
    }
    if let Some(results) = &turn.tool_results {
        for result in results {
            total += estimate_tokens(&result.output);
        }
    }
    // Role overhead
    total + 4
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
