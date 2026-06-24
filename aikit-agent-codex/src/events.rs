use crate::protocol::RequestId;
use serde_json::Value;

/// Inbound server message routed to the caller via the events channel.
#[derive(Debug, Clone)]
pub enum ServerMessage {
    /// JSON-RPC notification (no id). Examples: `turn/started`, `item/*`,
    /// `turn/completed`.
    Notification(ServerNotification),

    /// JSON-RPC request initiated by the server (e.g. an approval prompt).
    /// Reply with [`crate::CodexClient::reply_server_request`].
    ServerRequest(ServerRequest),
}

/// A JSON-RPC notification from the server.
#[derive(Debug, Clone)]
pub struct ServerNotification {
    /// Method name, e.g. `turn/completed`.
    pub method: String,
    /// Raw `params` payload.
    pub params: Value,
}

impl ServerNotification {
    /// Classify the method into a known kind for ergonomic matching.
    pub fn kind(&self) -> ServerNotificationKind {
        ServerNotificationKind::from_method(&self.method)
    }
}

/// Typed classification of common app-server notification methods.
///
/// Unknown methods map to [`ServerNotificationKind::Other`]; inspect
/// `ServerNotification::method` and `params` for those.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerNotificationKind {
    ThreadStarted,
    ThreadStatusChanged,
    ThreadClosed,
    TurnStarted,
    TurnCompleted,
    ItemStarted,
    ItemCompleted,
    AgentMessageDelta,
    CommandExecutionOutputDelta,
    Other,
}

impl ServerNotificationKind {
    /// Map a wire method name to a known kind.
    pub fn from_method(method: &str) -> Self {
        match method {
            "thread/started" => Self::ThreadStarted,
            "thread/status/changed" => Self::ThreadStatusChanged,
            "thread/closed" => Self::ThreadClosed,
            "turn/started" => Self::TurnStarted,
            "turn/completed" => Self::TurnCompleted,
            "item/started" => Self::ItemStarted,
            "item/completed" => Self::ItemCompleted,
            "item/agentMessage/delta" => Self::AgentMessageDelta,
            "item/commandExecution/outputDelta" => Self::CommandExecutionOutputDelta,
            _ => Self::Other,
        }
    }
}

/// A JSON-RPC request initiated by the server (e.g. `thread/approveCommand`).
#[derive(Debug, Clone)]
pub struct ServerRequest {
    /// Id to echo back in the reply.
    pub id: RequestId,
    /// Method name.
    pub method: String,
    /// Raw `params` payload.
    pub params: Value,
}
