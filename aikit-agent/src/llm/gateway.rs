use crate::llm::types::{LlmError, LlmRequest, LlmResponse, LlmStreamHandle};

pub trait LlmGateway: Send + Sync {
    fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;
    fn stream(&self, req: LlmRequest) -> Result<LlmStreamHandle, LlmError>;
}
