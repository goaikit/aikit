pub mod gateway;
pub mod mock;
pub mod openai_compat;
pub mod stream;
pub mod types;

pub use gateway::LlmGateway;
pub use openai_compat::resolve_api_key;
pub use types::{
    FunctionDefinition, LlmError, LlmMessage, LlmRequest, LlmResponse, LlmStreamEvent,
    LlmStreamHandle, LlmUsage, ToolCall, ToolCallFunction, ToolChoice, ToolDefinition,
};
