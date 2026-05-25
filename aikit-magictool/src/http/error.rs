use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, Serialize)]
#[serde(tag = "code", content = "message")]
pub enum ToolError {
    #[error("tool not found")]
    #[serde(rename = "tool_not_found")]
    ToolNotFound,
    #[error("input invalid: {0}")]
    #[serde(rename = "input_invalid")]
    InputInvalid(String),
    #[error("chat unavailable")]
    #[serde(rename = "chat_unavailable")]
    ChatUnavailable,
    #[error("agent failed: {0}")]
    #[serde(rename = "agent_failed")]
    AgentFailed(String),
    #[error("output invalid: {0}")]
    #[serde(rename = "output_invalid")]
    OutputInvalid(String),
    #[error("internal error: {0}")]
    #[serde(rename = "internal")]
    Internal(String),
}

impl IntoResponse for ToolError {
    fn into_response(self) -> Response {
        let status = match &self {
            ToolError::ToolNotFound => StatusCode::NOT_FOUND,
            ToolError::InputInvalid(_) => StatusCode::BAD_REQUEST,
            ToolError::ChatUnavailable => StatusCode::NOT_IMPLEMENTED,
            ToolError::AgentFailed(_) => StatusCode::BAD_GATEWAY,
            ToolError::OutputInvalid(_) => StatusCode::BAD_GATEWAY,
            ToolError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = serde_json::json!({ "error": self });
        (status, axum::Json(body)).into_response()
    }
}
