//! Shared tool-approval types for session bridges.
//!
//! Both [`super::claude_session`] and [`super::codex_session`] expose a
//! synchronous permission callback using these shared types; callers write one
//! handler and attach it to whichever session they open.

use std::sync::Arc;

/// A request for the caller to approve or deny a tool call mid-session.
#[derive(Debug, Clone)]
pub struct ToolApprovalRequest {
    /// Tool name or server-request method (e.g. `Bash`, `thread/approveCommand`).
    pub tool_name: String,
    /// Structured tool input / request params.
    pub input: serde_json::Value,
    /// Originating tool-use id, when provided by the backend.
    pub tool_use_id: Option<String>,
}

/// The caller's decision on a [`ToolApprovalRequest`].
#[derive(Debug, Clone)]
pub enum ToolDecision {
    /// Allow the call with its original input.
    Allow,
    /// Allow the call, substituting `input` for the original.
    AllowWith { input: serde_json::Value },
    /// Deny the call with a message shown to the agent.
    Deny { message: String },
}

/// Synchronous permission callback, invoked on the bridge thread.
///
/// Must be fast and non-blocking — the bridge cannot process further events
/// until this returns. `Send + Sync` allows the callback to live on the
/// bridge alongside the session client.
pub type PermissionCallback = Arc<dyn Fn(ToolApprovalRequest) -> ToolDecision + Send + Sync>;
