pub mod backend;
pub mod core;
pub mod http;
pub mod reference;

pub use core::{
    executor::{ChatEvent, ExecError, ToolChat, ToolExecutor},
    mock::{MockChat, MockExecutor},
    registry::{ToolListEntry, ToolRegistry},
    tool::{compose_prompt, ToolDef},
    validate::validate_value,
};
pub use http::{error::ToolError, handlers::router, state::MagicToolState};

#[cfg(feature = "agent")]
pub fn default_registry_state() -> MagicToolState {
    use crate::backend::{PipelineExecutor, SessionChat};
    use crate::reference::draft_lead_tool;
    use std::sync::Arc;

    let mut registry = ToolRegistry::new();
    registry.register(draft_lead_tool());

    MagicToolState {
        registry: Arc::new(registry),
        executor: Arc::new(PipelineExecutor),
        chat: Some(Arc::new(SessionChat)),
    }
}
