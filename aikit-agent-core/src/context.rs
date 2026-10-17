//! The conversation the loop keeps, and its token estimate.

use serde::{Deserialize, Serialize};

use crate::llm::types::{LlmMessage, MessageToolCall, MessageToolCallFunction};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl TokenBudget {
    /// The budget the loop has always used: `total` tokens, of which 1000 are
    /// kept for tool definitions and 2000 for the answer.
    pub fn with_total(total: u64) -> Self {
        Self {
            total_budget: total,
            ..Self::default()
        }
    }
}

/// Estimated tokens of a system prompt plus a conversation.
pub fn estimate_context_tokens(system_instructions: &str, conversation: &[Turn]) -> u64 {
    estimate_tokens(system_instructions)
        + conversation.iter().map(estimate_turn_tokens).sum::<u64>()
}

/// The chat messages for a system prompt and a conversation: the system
/// prompt first (unless empty), then one message per user or assistant turn
/// and one per tool result.
pub fn build_messages(system_instructions: &str, conversation: &[Turn]) -> Vec<LlmMessage> {
    let mut messages = Vec::new();

    if !system_instructions.is_empty() {
        messages.push(LlmMessage {
            role: "system".to_string(),
            content: Some(system_instructions.to_string()),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for turn in conversation {
        match turn.role {
            TurnRole::User => {
                messages.push(LlmMessage {
                    role: "user".to_string(),
                    content: Some(turn.content.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            TurnRole::Assistant => {
                let tool_calls = turn.tool_calls.as_ref().map(|calls| {
                    calls
                        .iter()
                        .map(|tc| MessageToolCall {
                            id: tc.id.clone(),
                            call_type: "function".to_string(),
                            function: MessageToolCallFunction {
                                name: tc.name.clone(),
                                arguments: tc.arguments.clone(),
                            },
                        })
                        .collect::<Vec<_>>()
                });
                let content = if turn.content.is_empty() && tool_calls.is_some() {
                    None
                } else {
                    Some(turn.content.clone())
                };
                messages.push(LlmMessage {
                    role: "assistant".to_string(),
                    content,
                    tool_calls,
                    tool_call_id: None,
                });
            }
            TurnRole::Tool => {
                if let Some(results) = &turn.tool_results {
                    for result in results {
                        messages.push(LlmMessage {
                            role: "tool".to_string(),
                            content: Some(result.output.clone()),
                            tool_calls: None,
                            tool_call_id: Some(result.call_id.clone()),
                        });
                    }
                }
            }
        }
    }

    messages
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
    fn test_context_estimated_tokens() {
        let conversation = vec![
            Turn::user("What is Rust?"),
            Turn::assistant("Rust is a systems programming language."),
        ];
        let tokens = estimate_context_tokens("System: you are helpful.", &conversation);
        assert!(tokens > 0);
    }

    #[test]
    fn test_with_total_keeps_the_reserves() {
        assert_eq!(TokenBudget::with_total(5000).available(), 2000);
    }

    #[test]
    fn test_build_messages_includes_tool_calls_and_results() {
        let conversation = vec![
            Turn::user("Read the file"),
            Turn::assistant_with_tool_calls(
                "",
                vec![ContextToolCall {
                    id: "call_abc".to_string(),
                    name: "read_file".to_string(),
                    arguments: r#"{"path": "AGENTS.md"}"#.to_string(),
                }],
            ),
            Turn::tool_result(vec![ContextToolResult {
                call_id: "call_abc".to_string(),
                output: "file contents here".to_string(),
                is_error: false,
            }]),
        ];
        let messages = build_messages("System prompt", &conversation);
        let roles: Vec<&str> = messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "tool"]);
        let assistant = &messages[2];
        assert!(
            assistant.content.is_none(),
            "empty text with tool calls is null"
        );
        let calls = assistant.tool_calls.as_ref().unwrap();
        assert_eq!(calls[0].id, "call_abc");
        assert_eq!(calls[0].function.name, "read_file");
        assert_eq!(messages[3].tool_call_id.as_deref(), Some("call_abc"));
        assert_eq!(messages[3].content.as_deref(), Some("file contents here"));
    }

    #[test]
    fn test_build_messages_leaves_out_an_empty_system_prompt() {
        let messages = build_messages("", &[Turn::user("hi")]);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
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
