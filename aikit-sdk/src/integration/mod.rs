//! Reusable integration services for application consumers.
//!
//! The managed gateway store is extracted from the existing serving code. It
//! retains its recovery semantics and single-host ownership lock. External hook
//! invocations must not open it as independent gateway hosts: doing so would
//! interrupt managed sessions. External-session support is developed separately.

/// Existing managed-session persistence used by the HTTP gateway and embeddings.
pub mod gateway_store;

mod agent_tool_session;
mod agent_tools;
mod capabilities;
mod codex;
mod codex_messages;
mod command;
mod cursor;
mod hooks;
mod install;
mod message_append;
mod message_bridge;
mod messages;
mod pi;
mod sessions;
mod tool_effects;
pub use agent_tools::{
    render_agent_tool_plugin, AgentToolCall, AgentToolPlugin, AgentToolSpec, NativeToolContext,
    AGENT_TOOL_CALL_LIMIT, AGENT_TOOL_REQUEST_LIMIT, AGENT_TOOL_SPEC_LIMIT,
};
pub use capabilities::{
    CapabilityAssessment, CapabilityReport, CapabilityRequirementsError, IntegrationCapability,
    SessionMode, Support,
};
pub use codex_messages::{
    inspect_codex_message_snapshot, CodexMessageInspection, CodexMessageLocation, CodexMessageMatch,
};
pub use hooks::{
    Decision, DecisionFuture, HandlerError, HookHandler, HookPage, HookRecord, HookRequest,
    HookResponse, NativeTurn, NativeTurnOutcome, NativeTurnReason,
};
pub use install::{
    AgentToolFileChange, AppliedInstallationGuard, CompletionBlockingPolicy, HookCommand,
    HookEvent, InstallPlan, InstallSpec, Installation, InstallationStatus, IntegrationError,
    IntegrationService, SettingChange, StateUpgradeReport,
};
pub use message_append::{MessageAppendCheckpoint, MessageAppendIntent, MessageAppendStored};
pub use message_bridge::{
    MessageHandoffOutcome, MessageHandoffRecord, RecordedMessageAction, RecordedMessageReply,
    RecordedMessageRequest, MESSAGE_BRIDGE_PAYLOAD_LIMIT,
};
pub use messages::{
    MessageEvidence, MessageObservation, MessagePage, MessageProof, MessageReceipt, MessageRequest,
    MessageReservation, MessageState, MESSAGE_CONTENT_LIMIT,
};
pub use sessions::{RecordedSessionGuard, SessionBinding, SessionRef, SessionStatus};
pub use tool_effects::{TextReplacement, ToolEffect};
