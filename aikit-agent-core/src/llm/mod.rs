//! Model types, the OpenAI chat-completions wire shapes, and the SSE parser.

pub mod stream;
pub mod types;
pub mod wire;

pub use stream::{SseLine, SseParser};
pub use types::{
    FunctionDefinition, LlmError, LlmMessage, LlmRequest, LlmResponse, LlmStreamEvent,
    LlmStreamHandle, LlmUsage, ToolCall, ToolCallFunction, ToolChoice, ToolDefinition,
};
