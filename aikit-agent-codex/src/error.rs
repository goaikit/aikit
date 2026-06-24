use serde_json::Value;

/// Errors returned by [`crate::CodexClient`].
#[derive(Debug, thiserror::Error)]
pub enum CodexError {
    /// `codex app-server` failed to spawn.
    #[error("failed to spawn codex: {0}")]
    Spawn(#[from] std::io::Error),

    /// A JSON (de)serialization failure on the wire.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// The server replied with a JSON-RPC `error` object for a request.
    #[error("server returned error for '{method}' (code {code}): {message}")]
    ServerError {
        method: String,
        code: i64,
        message: String,
        data: Option<Value>,
    },

    /// The transport closed before the server replied to `method`.
    #[error("connection closed before '{method}' replied")]
    Closed { method: String },

    /// Writing a framed message to the server's stdin failed.
    #[error("transport write failed: {0}")]
    Send(String),

    /// `initialize` was called more than once on the same connection.
    #[error("already initialized; initialize() can only be called once per connection")]
    AlreadyInitialized,

    /// A helper received a parameter value the Codex protocol does not accept.
    #[error("invalid parameter '{name}': {message}")]
    InvalidParameter { name: String, message: String },

    /// `request_with_timeout` exceeded its deadline.
    #[error("timed out waiting for '{method}' reply after {timeout_secs}s")]
    RequestTimeout { method: String, timeout_secs: u64 },
}

/// Convenience `Result` alias.
pub type Result<T, E = CodexError> = std::result::Result<T, E>;
