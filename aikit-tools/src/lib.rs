pub mod error;
pub mod handlers;
pub mod reference;
pub mod registry;
pub mod runner;
pub mod schema;
pub mod state;
pub mod tool;

pub use aikit_agent::AgentInternalEvent;
pub use error::ToolsError;
pub use registry::ToolRegistry;
pub use runner::{AgentRunner, ProductionAgentRunner};
pub use state::ToolsState;
pub use tool::AgentTool;

use axum::{
    routing::{get, post},
    Router,
};

pub fn router(state: ToolsState) -> Router {
    Router::new()
        .route("/aitools", get(handlers::list_tools_handler))
        .route(
            "/aitools/{ns}/{tool}/schema",
            get(handlers::get_schema_handler),
        )
        .route("/aitools/{ns}/{tool}", post(handlers::invoke_handler))
        .with_state(state)
}

pub fn default_registry_state() -> ToolsState {
    let mut registry = ToolRegistry::new();
    registry.register(reference::draft_contact_tool());
    ToolsState {
        registry: std::sync::Arc::new(registry),
        runner: std::sync::Arc::new(ProductionAgentRunner),
    }
}
