use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolsError {
    #[error("tool not found: {0}")]
    ToolNotFound(String),

    #[error("input invalid: {0}")]
    InputInvalid(String),

    #[error("agent failed: {0}")]
    AgentFailed(String),

    #[error("agent produced no output")]
    AgentNoOutput,

    #[error("output invalid: {0}")]
    OutputInvalid(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ToolsError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            ToolsError::ToolNotFound(msg) => (StatusCode::NOT_FOUND, "tool_not_found", msg.clone()),
            ToolsError::InputInvalid(msg) => {
                (StatusCode::BAD_REQUEST, "input_invalid", msg.clone())
            }
            ToolsError::AgentFailed(msg) => (StatusCode::BAD_GATEWAY, "agent_failed", msg.clone()),
            ToolsError::AgentNoOutput => (
                StatusCode::BAD_GATEWAY,
                "agent_no_output",
                "agent produced no output".to_string(),
            ),
            ToolsError::OutputInvalid(msg) => {
                (StatusCode::BAD_GATEWAY, "output_invalid", msg.clone())
            }
            ToolsError::Internal(msg) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal", msg.clone())
            }
        };

        let body = json!({ "error": { "code": code, "message": message } }).to_string();
        (
            status,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response()
    }
}
