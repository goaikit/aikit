//! Versioned, transport-independent session contract. Agent payloads remain canonical.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub const PROTOCOL_VERSION: u32 = 1;

/// Session backends include native protocols that are not one-shot CLI runners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionBackend {
    Claude,
    Codex,
    Pi,
    Cursor,
    Gemini,
    #[serde(rename = "opencode")]
    OpenCode,
    Aikit,
    Grok,
    Antigravity,
}
pub const BACKENDS: &[SessionBackend] = &[
    SessionBackend::Claude,
    SessionBackend::Codex,
    SessionBackend::Pi,
    SessionBackend::Cursor,
    SessionBackend::Gemini,
    SessionBackend::OpenCode,
    SessionBackend::Aikit,
    SessionBackend::Grok,
    SessionBackend::Antigravity,
];
impl SessionBackend {
    pub fn key(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Pi => "pi",
            Self::Cursor => "cursor",
            Self::Gemini => "gemini",
            Self::OpenCode => "opencode",
            Self::Aikit => "aikit",
            Self::Grok => "grok",
            Self::Antigravity => "antigravity",
        }
    }
}

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
pub struct SessionCapabilities {
    pub send_turn: bool,
    pub interrupt: bool,
    pub resume: bool,
    pub permissions: bool,
    pub questions: bool,
    pub steer: bool,
    pub follow_up: bool,
    pub set_model: bool,
    pub context_usage: bool,
}

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicy {
    #[default]
    Ask,
    Deny,
    Allow,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSession {
    pub command_id: String,
    pub backend: SessionBackend,
    pub cwd: PathBuf,
    pub prompt: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub resume: Option<String>,
    #[serde(default)]
    pub permission_policy: PermissionPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionCommand {
    pub command_id: String,
    #[serde(flatten)]
    pub action: SessionAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionAction {
    SendTurn {
        text: String,
    },
    Interrupt,
    Steer {
        text: String,
    },
    FollowUp {
        text: String,
    },
    SetModel {
        model: String,
    },
    ContextUsage,
    Respond {
        request_id: String,
        response: RequestResponse,
    },
    Close,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequestResponse {
    Allow {
        #[serde(default)]
        option_id: Option<String>,
    },
    Deny,
    Answers {
        answers: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Opening,
    Running,
    Idle,
    Closing,
    Closed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionInfo {
    pub host_id: String,
    pub session_id: String,
    pub backend: SessionBackend,
    pub cwd: PathBuf,
    pub status: SessionStatus,
    pub capabilities: SessionCapabilities,
    pub native_session_id: Option<String>,
    pub active_turn_id: Option<String>,
    pub last_sequence: u64,
}

/// Accepted is durable acceptance; dispatched is local transport acceptance.
/// Neither implies that a turn completed or that an uncertain action is safe to retry.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    Accepted,
    Dispatched,
    Failed,
    OutcomeUnknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CommandReceipt {
    pub command_id: String,
    pub session_id: String,
    pub status: CommandStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<CommandFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CommandFailure {
    pub code: String,
    /// Advice, never automatic authorization to repeat native side effects.
    pub retry: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PendingRequest {
    pub request_id: String,
    pub kind: String,
    pub payload: Value,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionEvent {
    pub version: u32,
    pub host_id: String,
    pub session_id: String,
    pub sequence: u64,
    pub turn_id: Option<String>,
    pub timestamp_ms: u64,
    #[serde(flatten)]
    pub event: SessionEventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum SessionEventKind {
    /// Client input accepted by the host, separate from native agent output.
    Input {
        text: String,
    },
    Agent(#[schemars(with = "serde_json::Value")] super::types::AgentEvent),
    State(SessionStatus),
    Command(CommandReceipt),
    Request(PendingRequest),
    RequestResolved {
        request_id: String,
        reason: String,
    },
    Error {
        code: String,
        message: String,
    },
    Native {
        protocol: String,
        value: Value,
    },
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Generated from the same Rust types accepted by the SDK and HTTP handlers.
/// Agent event payloads retain the existing canonical vocabulary unchanged.
pub fn schemas() -> Value {
    serde_json::json!({
        "protocol_version":PROTOCOL_VERSION,
        "create_session":schemars::schema_for!(CreateSession),
        "command":schemars::schema_for!(SessionCommand),
        "receipt":schemars::schema_for!(CommandReceipt),
        "session":schemars::schema_for!(SessionInfo),
        "event":schemars::schema_for!(SessionEvent),
        "response":schemars::schema_for!(RequestResponse)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_command_schema_accepts_real_wire_shape() {
        let command = serde_json::json!({"command_id":"c","type":"close"});
        let parsed: SessionCommand = serde_json::from_value(command.clone()).unwrap();
        assert!(matches!(parsed.action, SessionAction::Close));
        let schemas = schemas();
        assert!(jsonschema::is_valid(&schemas["command"], &command));
        assert!(!jsonschema::is_valid(
            &schemas["command"],
            &serde_json::json!({"command_id":"c","type":"unknown"})
        ));
    }
    #[test]
    fn all_backend_keys_round_trip() {
        for backend in BACKENDS {
            assert_eq!(
                serde_json::from_value::<SessionBackend>(serde_json::json!(backend.key())).unwrap(),
                *backend
            );
        }
    }
    #[test]
    fn reject_unknown_action() {
        assert!(serde_json::from_value::<SessionAction>(
            serde_json::json!({"type":"execute_shell"})
        )
        .is_err());
    }
    #[test]
    fn canonical_payload_survives_envelope() {
        let original = super::super::types::AgentEvent {
            agent_key: "claude".into(),
            seq: 9,
            stream: super::super::types::AgentEventStream::Stdout,
            payload: super::super::types::AgentEventPayload::ToolUse {
                call_id: "tool1".into(),
                tool_name: "Read".into(),
                input: serde_json::json!({"path":"a"}),
            },
        };
        let event = SessionEvent {
            version: 1,
            host_id: "host".into(),
            session_id: "session".into(),
            sequence: 10,
            turn_id: None,
            timestamp_ms: 0,
            event: SessionEventKind::Agent(original.clone()),
        };
        let decoded: SessionEvent =
            serde_json::from_value(serde_json::to_value(event).unwrap()).unwrap();
        let SessionEventKind::Agent(actual) = decoded.event else {
            panic!("lost agent frame")
        };
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(original).unwrap()
        );
    }
}
