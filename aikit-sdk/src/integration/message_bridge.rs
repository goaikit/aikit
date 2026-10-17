//! Recorded-context RPC preparation for an owned module's fixed command. No
//! native API is called and no admission lease or provider proof is supplied.
use super::{
    AgentToolCall, HookEvent, HookRequest, IntegrationError, IntegrationService,
    MessageReservation, MessageState, NativeToolContext, SessionBinding, SessionRef,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::io::Read;

pub const MESSAGE_BRIDGE_PAYLOAD_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedMessageRequest {
    pub schema_version: u32,
    pub application_id: String,
    pub installation_id: String,
    pub agent_key: String,
    /// Outside model-controlled requests, but still claims rather than credentials.
    pub native_context: NativeToolContext,
    pub action: RecordedMessageAction,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecordedMessageAction {
    Poll,
    Recheck {
        operation_id: String,
        reservation_id: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedMessageReply {
    pub schema_version: u32,
    pub reference: SessionRef,
    pub recorded_idle: bool,
    /// A recorded check, never authorization for native I/O by itself.
    pub recorded_ready: bool,
    pub reservation: Option<MessageReservation>,
}

impl RecordedMessageRequest {
    pub fn read(reader: impl Read) -> Result<Self, IntegrationError> {
        let mut bytes = Vec::new();
        reader
            .take(super::AGENT_TOOL_CALL_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > super::AGENT_TOOL_CALL_LIMIT {
            return Err(IntegrationError::Invalid(
                "message bridge envelope exceeds 128 KiB".into(),
            ));
        }
        let frame: Self = serde_json::from_slice(&bytes)?;
        // Reuse canonical bounds/keys/context validation, without inventing a
        // model tool call or app authorization from this synthetic decode.
        AgentToolCall::read(
            serde_json::to_vec(&AgentToolCall {
                schema_version: frame.schema_version,
                application_id: frame.application_id.clone(),
                installation_id: frame.installation_id.clone(),
                agent_key: frame.agent_key.clone(),
                tool_name: "message_bridge".into(),
                native_context: frame.native_context.clone(),
                request: serde_json::json!({}),
            })?
            .as_slice(),
        )?;
        if let RecordedMessageAction::Recheck {
            operation_id,
            reservation_id,
        } = &frame.action
        {
            if [operation_id, reservation_id]
                .iter()
                .any(|v| v.trim().is_empty() || v.len() > 256 || v.contains('\0'))
            {
                return Err(IntegrationError::Invalid(
                    "invalid message bridge operation/reservation".into(),
                ));
            }
        }
        Ok(frame)
    }
}

impl IntegrationService {
    /// Thin CLI delegates bounded stdin here with a trusted installation ID from
    /// its fixed arguments/registration. The frame selects neither DB nor owner.
    /// Only the receipt-selected inherited environment key is consulted.
    pub fn handle_recorded_message_request(
        &self,
        installation_id: &str,
        reader: impl Read,
    ) -> Result<RecordedMessageReply, IntegrationError> {
        let frame = RecordedMessageRequest::read(reader)?;
        let key = self
            .agent_tool_environment_key(installation_id)?
            .ok_or_else(|| {
                IntegrationError::Unsupported(
                    "recorded message preparation requires an owned native module".into(),
                )
            })?;
        let inherited = std::env::var(key).ok();
        self.recorded_message_with_context(installation_id, frame, inherited.as_deref())
    }
    pub(super) fn recorded_message_with_context(
        &self,
        installation_id: &str,
        frame: RecordedMessageRequest,
        inherited: Option<&str>,
    ) -> Result<RecordedMessageReply, IntegrationError> {
        if self.agent_tool_environment_key(installation_id)?.is_none() {
            return Err(IntegrationError::Unsupported(
                "owned native module is unavailable".into(),
            ));
        }
        let installation = self.installed(installation_id)?;
        if !installation
            .spec
            .events
            .contains(&HookEvent::InputSubmitted)
        {
            return Err(IntegrationError::Unsupported(
                "recorded message preparation requires input observations".into(),
            ));
        }
        if frame.installation_id != installation.id
            || frame.application_id != installation.spec.application_id
            || frame.agent_key != installation.spec.agent_key
            || frame.agent_key != "claude"
        {
            return Err(IntegrationError::Invalid(
                "message bridge installation scope does not match".into(),
            ));
        }
        super::agent_tool_session::validate_tool_environment(
            &installation,
            &frame.native_context,
            inherited,
        )?;
        let reference = self.observed_session(installation_id, &frame.native_context.session_id)?;
        if reference.invocation_id.as_deref() != Some(frame.native_context.invocation_id.as_str()) {
            return Err(IntegrationError::StaleSession);
        }
        // Polling is not attachment. Never revive a detached subscription.
        let id: Option<String> = self
            .connection()?
            .query_row(
                "SELECT id FROM session_bindings WHERE reference=?1 AND detached=0",
                [serde_json::to_string(&reference)?],
                |r| r.get(0),
            )
            .optional()?;
        let binding = self.binding(&id.ok_or(IntegrationError::NotFound)?)?;
        let _lock = self.state_lock()?;
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if binding.status_inner(&tx)? != super::SessionStatus::Observed {
            return Err(IntegrationError::StaleSession);
        }
        let idle = binding.recorded_message_idle(&tx)?;
        let mut reply = RecordedMessageReply {
            schema_version: 1,
            reference,
            recorded_idle: idle,
            recorded_ready: false,
            reservation: None,
        };
        if idle {
            match frame.action {
                RecordedMessageAction::Poll => {
                    // Never issue a second ticket while the first is in flight.
                    let pending: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM outgoing_messages WHERE installation_id=?1 AND json_extract(body,'$.binding_id')=?2 AND json_extract(body,'$.state')='dispatching')",
                        params![installation_id,binding.id()],|r|r.get(0))?;
                    if !pending {
                        let operation: Option<String>=tx.query_row("SELECT operation_id FROM outgoing_messages WHERE installation_id=?1 AND json_extract(body,'$.binding_id')=?2 AND json_extract(body,'$.state')='accepted' ORDER BY rowid LIMIT 1",
                            params![installation_id,binding.id()],|r|r.get(0)).optional()?;
                        if let Some(operation) = operation {
                            let receipt = binding.message_status_record(&tx, &operation)?;
                            if receipt.request.content.len() > MESSAGE_BRIDGE_PAYLOAD_LIMIT {
                                return Err(IntegrationError::Unsupported("owned message bridge payload exceeds 64 KiB; no handoff was attempted".into()));
                            }
                            reply.reservation =
                                Some(binding.reserve_message_record(&tx, &operation)?);
                        }
                    }
                }
                RecordedMessageAction::Recheck {
                    operation_id,
                    reservation_id,
                } => {
                    let receipt = binding.message_status_record(&tx, &operation_id)?;
                    reply.recorded_ready = receipt.state == MessageState::Dispatching
                        && receipt.reservation_id.as_deref() == Some(reservation_id.as_str());
                }
            }
        }
        tx.commit()?;
        Ok(reply)
    }
}

impl SessionBinding<'_> {
    fn recorded_message_idle(
        &self,
        connection: &rusqlite::Connection,
    ) -> Result<bool, IntegrationError> {
        // Pending input/tools/proposals or absent outcomes conservatively defer.
        // This snapshot cannot prevent a native action after the transaction.
        let body: Option<String>=connection.query_row("SELECT body FROM hook_invocations WHERE installation_id=?1 AND installation_revision=?2 AND session_id=?3 AND json_extract(body,'$.invocation_id') IS ?4 AND json_extract(body,'$.agent_id') IS NULL AND json_extract(body,'$.event') IN ('input_submitted','before_tool','after_tool','tool_failed','completion_proposed','native_turn_started','native_turn_finished') ORDER BY sequence DESC LIMIT 1",
            params![self.reference.installation_id,self.reference.installation_revision,self.reference.native_session_id,self.reference.invocation_id],|r|r.get(0)).optional()?;
        let request = body
            .map(|body| serde_json::from_str::<HookRequest>(&body))
            .transpose()?;
        Ok(request.is_some_and(|request| {
            request.event == HookEvent::NativeTurnFinished
                && request
                    .native_turn
                    .is_some_and(|turn| turn.outcome.is_some())
        }))
    }
}
