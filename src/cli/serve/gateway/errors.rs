//! Stable HTTP error vocabulary. Diagnostics are never used as wire codes.
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub fn response(error: anyhow::Error) -> Response {
    let message = error.to_string();
    let (status, code, retry) = classify(&error);
    (
        status,
        Json(json!({"error":{"code":code,"message":message,"retry":retry}})),
    )
        .into_response()
}

fn classify(error: &anyhow::Error) -> (StatusCode, &str, &str) {
    use StatusCode as S;
    if let Some(e) = error.downcast_ref::<rusqlite::Error>() {
        return if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
            (S::NOT_FOUND, "not_found", "never")
        } else {
            (
                S::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                "inspect_receipt",
            )
        };
    }
    match error.to_string().as_str() {
        "workspace_not_allowed" => (S::FORBIDDEN, "workspace_not_allowed", "never"),
        "session_capacity_exceeded" => (
            S::SERVICE_UNAVAILABLE,
            "session_capacity_exceeded",
            "same_command_id",
        ),
        "subscriber_capacity_exceeded" => (
            S::SERVICE_UNAVAILABLE,
            "subscriber_capacity_exceeded",
            "backoff",
        ),
        "session_history_capacity_exceeded" => (
            S::SERVICE_UNAVAILABLE,
            "session_history_capacity_exceeded",
            "operator_action",
        ),
        "host_draining" => (S::SERVICE_UNAVAILABLE, "host_draining", "same_command_id"),
        "persistence_unavailable" => (
            S::SERVICE_UNAVAILABLE,
            "storage_unavailable",
            "inspect_receipt",
        ),
        "idempotency_conflict" => (S::CONFLICT, "idempotency_conflict", "never"),
        "session_not_active" => (S::CONFLICT, "session_not_active", "never"),
        "cursor_expired" => (S::GONE, "cursor_expired", "resynchronize"),
        "invalid_cursor" | "cursor_ahead" => (S::BAD_REQUEST, "invalid_cursor", "never"),
        "invalid_command_id" | "prompt_required" | "prompt_too_large" => {
            (S::BAD_REQUEST, "invalid_request", "never")
        }
        _ => (
            S::INTERNAL_SERVER_ERROR,
            "internal_error",
            "inspect_receipt",
        ),
    }
}

pub fn command_failure(message: &str) -> aikit_sdk::runner::session::CommandFailure {
    let (code, retry) = match message {
        "command_queue_full" => ("command_queue_full", "new_command_after_backoff"),
        "request_expired_or_resolved" => ("request_expired_or_resolved", "never"),
        "session_not_active" => ("session_not_active", "never"),
        "unsupported_operation" => ("unsupported_operation", "never"),
        "turn_not_idle" => ("turn_not_idle", "never"),
        "text_required" | "prompt_too_large" => ("invalid_request", "never"),
        _ => ("native_operation_failed", "inspect_receipt"),
    };
    aikit_sdk::runner::session::CommandFailure {
        code: code.into(),
        retry: retry.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_do_not_become_codes_or_authorize_blind_retry() {
        assert_eq!(
            classify(&anyhow::anyhow!("disk /private/path failed")),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "inspect_receipt"
            )
        );
        assert_eq!(
            classify(&anyhow::anyhow!("idempotency_conflict")).2,
            "never"
        );
        assert_eq!(
            classify(&anyhow::anyhow!("session_capacity_exceeded")).2,
            "same_command_id"
        );
    }
}
