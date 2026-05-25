use crate::core::{
    executor::{ToolChat, ToolExecutor},
    registry::ToolRegistry,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct MagicToolState {
    pub registry: Arc<ToolRegistry>,
    pub executor: Arc<dyn ToolExecutor>,
    pub chat: Option<Arc<dyn ToolChat>>,
}
