use super::tool::ToolDef;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecError {
    #[error("agent invocation failed: {0}")]
    AgentFailed(String),
    #[error("output did not conform to output schema: {0}")]
    OutputInvalid(String),
    #[error("internal error: {0}")]
    Internal(String),
}

pub trait ToolExecutor: Send + Sync {
    fn execute(
        &self,
        tool: &ToolDef,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, ExecError>;
}

#[derive(Debug, Clone)]
pub enum ChatEvent {
    Started { session_id: String },
    Delta(String),
    Final(String),
    Error(String),
}

pub trait ToolChat: Send + Sync {
    fn run_turn(
        &self,
        tool: &ToolDef,
        session_id: Option<&str>,
        msg: &str,
        sink: &mut dyn FnMut(ChatEvent),
    ) -> Result<(), ExecError>;

    fn finalize(&self, tool: &ToolDef, session_id: &str) -> Result<serde_json::Value, ExecError>;
}
