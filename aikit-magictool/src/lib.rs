pub mod backend;
pub mod core;
pub mod http;

pub use core::{
    executor::{ChatEvent, ExecError, ToolChat, ToolExecutor},
    mock::{MockChat, MockExecutor},
    registry::{ToolListEntry, ToolRegistry},
    tool::{compose_prompt, ToolDef},
    validate::validate_value,
};
pub use http::{error::ToolError, handlers::router, state::MagicToolState};

#[cfg(feature = "agent")]
pub fn state_with_registry(registry: ToolRegistry) -> MagicToolState {
    use crate::backend::{PipelineExecutor, SessionChat};
    use std::sync::Arc;
    MagicToolState {
        registry: Arc::new(registry),
        executor: Arc::new(PipelineExecutor),
        chat: Some(Arc::new(SessionChat)),
    }
}
