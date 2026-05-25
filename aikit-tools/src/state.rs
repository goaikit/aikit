use std::sync::Arc;

use crate::registry::ToolRegistry;
use crate::runner::AgentRunner;

#[derive(Clone)]
pub struct ToolsState {
    pub registry: Arc<ToolRegistry>,
    pub runner: Arc<dyn AgentRunner>,
}
