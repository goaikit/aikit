//! Recorded-context RPC preparation for an owned module's fixed command. No
//! native API is called and no admission lease or provider proof is supplied.
use super::{
    AgentToolCall, HookEvent, HookRequest, IntegrationError, IntegrationService,
    MessageAppendCheckpoint, MessageAppendIntent, MessageAppendStored, MessageInputDispatched,
    MessageInputIntent, MessageReservation, MessageState, NativeToolContext, SessionBinding,
    SessionRef,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::io::Read;

/// Raw native return/error retained for qualification. These are claims, not
/// authenticated native events and never create acknowledgement/consumption.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MessageHandoffOutcome {
    Returned { result: serde_json::Value },
    Uncertain { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageHandoffRecord {
    pub schema_version: u32,
    pub reservation: MessageReservation,
    pub outcome: MessageHandoffOutcome,
    pub recorded_at_unix_ms: u64,
}

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
    /// Validate inherited scope and read idle state without attaching/reserving.
    Inspect,
    Poll,
    Recheck {
        operation_id: String,
        reservation_id: String,
    },
    RecordHandoff {
        operation_id: String,
        reservation_id: String,
        outcome: MessageHandoffOutcome,
    },
    PrepareAppend {
        operation_id: String,
        reservation_id: String,
        intent: MessageAppendIntent,
    },
    RecordAppendStored {
        operation_id: String,
        reservation_id: String,
        native_row_id: String,
        stored: MessageAppendStored,
    },
    PrepareInput {
        operation_id: String,
        reservation_id: String,
        intent: MessageInputIntent,
    },
    RecordInputDispatched {
        operation_id: String,
        reservation_id: String,
        intent: MessageInputIntent,
        dispatched: MessageInputDispatched,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedMessageReply {
    pub schema_version: u32,
    pub reference: SessionRef,
    pub recorded_idle: bool,
    /// A recorded check, never authorization for native I/O by itself.
    pub recorded_ready: bool,
    /// Poll's newly reserved operation, or the exact existing reservation
    /// confirmed by a ready Recheck. None when no dispatch is authorized by
    /// the recorded check; this is still not a native admission lease.
    pub reservation: Option<MessageReservation>,
    /// Exact rechecked operation state. Terminal/Unknown retires the local
    /// ticket without resending, so one uncertain operation need not block others.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_state: Option<MessageState>,
    /// Candidate correlation only; never qualified native receipt/consumption.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append_checkpoint: Option<MessageAppendCheckpoint>,
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
        }
        | RecordedMessageAction::RecordHandoff {
            operation_id,
            reservation_id,
            ..
        }
        | RecordedMessageAction::PrepareAppend {
            operation_id,
            reservation_id,
            ..
        }
        | RecordedMessageAction::RecordAppendStored {
            operation_id,
            reservation_id,
            ..
        }
        | RecordedMessageAction::PrepareInput {
            operation_id,
            reservation_id,
            ..
        }
        | RecordedMessageAction::RecordInputDispatched {
            operation_id,
            reservation_id,
            ..
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
        if let RecordedMessageAction::RecordHandoff { outcome, .. } = &frame.action {
            validate_handoff(outcome)?;
        }
        match &frame.action {
            RecordedMessageAction::PrepareInput { intent, .. } => {
                super::message_append::validate_input_intent(intent)?;
            }
            RecordedMessageAction::RecordInputDispatched {
                intent, dispatched, ..
            } => {
                super::message_append::validate_input_intent(intent)?;
                super::message_append::validate_input_dispatched(dispatched)?;
            }
            RecordedMessageAction::PrepareAppend { intent, .. } => {
                super::message_append::validate_intent(intent)?
            }
            RecordedMessageAction::RecordAppendStored {
                native_row_id,
                stored,
                ..
            } => {
                if native_row_id.trim().is_empty()
                    || native_row_id.len() > 256
                    || native_row_id.contains('\0')
                {
                    return Err(IntegrationError::Invalid(
                        "invalid append row identity".into(),
                    ));
                }
                super::message_append::validate_stored(stored)?;
            }
            _ => (),
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
        let Some(id) = id else {
            let detached: bool = self.connection()?.query_row(
                "SELECT EXISTS(SELECT 1 FROM session_bindings WHERE reference=?1 AND detached=1)",
                [serde_json::to_string(&reference)?],
                |row| row.get(0),
            )?;
            if detached {
                return Err(IntegrationError::Detached);
            }
            if matches!(
                frame.action,
                RecordedMessageAction::Poll | RecordedMessageAction::Inspect
            ) {
                // The native module may start before the application attaches.
                // Wait without creating a binding, claiming a ticket or treating
                // a genuinely detached subscription as a new attachment.
                return Ok(RecordedMessageReply {
                    schema_version: 1,
                    reference,
                    recorded_idle: false,
                    recorded_ready: false,
                    reservation: None,
                    recorded_state: None,
                    append_checkpoint: None,
                });
            }
            return Err(IntegrationError::NotFound);
        };
        let binding = self.binding(&id)?;
        let inspecting = matches!(frame.action, RecordedMessageAction::Inspect);
        let _lock = if inspecting {
            None
        } else if matches!(
            frame.action,
            RecordedMessageAction::RecordHandoff { .. }
                | RecordedMessageAction::PrepareAppend { .. }
                | RecordedMessageAction::RecordAppendStored { .. }
                | RecordedMessageAction::PrepareInput { .. }
                | RecordedMessageAction::RecordInputDispatched { .. }
        ) {
            Some(self.handoff_record_lock()?)
        } else {
            Some(self.state_lock()?)
        };
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(if inspecting {
            TransactionBehavior::Deferred
        } else {
            TransactionBehavior::Immediate
        })?;
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
            recorded_state: None,
            append_checkpoint: None,
        };
        if let RecordedMessageAction::PrepareInput {
            operation_id,
            reservation_id,
            intent,
        }
        | RecordedMessageAction::RecordInputDispatched {
            operation_id,
            reservation_id,
            intent,
            ..
        } = &frame.action
        {
            let reservation =
                reservation_from_receipt(&binding.message_status_record(&tx, operation_id)?)?;
            if reservation.reservation_id != *reservation_id {
                return Err(IntegrationError::Conflict(
                    "input reservation does not match".into(),
                ));
            }
            reply.append_checkpoint = Some(match &frame.action {
                RecordedMessageAction::PrepareInput { .. } => {
                    binding.prepare_message_input_record(&tx, &reservation, intent.clone())?
                }
                RecordedMessageAction::RecordInputDispatched { dispatched, .. } => binding
                    .record_message_input_dispatched_record(
                        &tx,
                        &reservation,
                        intent,
                        dispatched.clone(),
                    )?,
                _ => unreachable!(),
            });
        } else if let RecordedMessageAction::RecordHandoff {
            operation_id,
            reservation_id,
            outcome,
        } = &frame.action
        {
            // A native return can arrive during the resulting Turn. Never gate
            // retaining that return on the idle snapshot.
            binding.record_handoff_record(&tx, operation_id, reservation_id, outcome)?;
        } else if let RecordedMessageAction::PrepareAppend {
            operation_id,
            reservation_id,
            intent,
        } = &frame.action
        {
            let reservation =
                reservation_from_receipt(&binding.message_status_record(&tx, operation_id)?)?;
            if reservation.reservation_id != *reservation_id {
                return Err(IntegrationError::Conflict(
                    "append reservation does not match".into(),
                ));
            }
            reply.append_checkpoint =
                Some(binding.prepare_message_append_record(&tx, &reservation, intent.clone())?);
        } else if let RecordedMessageAction::RecordAppendStored {
            operation_id,
            reservation_id,
            native_row_id,
            stored,
        } = &frame.action
        {
            let reservation =
                reservation_from_receipt(&binding.message_status_record(&tx, operation_id)?)?;
            if reservation.reservation_id != *reservation_id {
                return Err(IntegrationError::Conflict(
                    "append reservation does not match".into(),
                ));
            }
            reply.append_checkpoint = Some(binding.record_message_append_stored_record(
                &tx,
                &reservation,
                native_row_id,
                stored.clone(),
            )?);
        } else if idle {
            match frame.action {
                RecordedMessageAction::Inspect => {}
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
                    if receipt.reservation_id.as_deref() != Some(reservation_id.as_str()) {
                        return Err(IntegrationError::Conflict(
                            "message recheck reservation does not match".into(),
                        ));
                    }
                    reply.recorded_state = Some(receipt.state);
                    reply.recorded_ready = receipt.state == MessageState::Dispatching
                        && receipt.reservation_id.as_deref() == Some(reservation_id.as_str());
                    if reply.recorded_ready {
                        // Return the immutable operation actually checked. A
                        // same-session ready boolean cannot identify its bytes.
                        reply.reservation = Some(reservation_from_receipt(&receipt)?);
                    }
                }
                RecordedMessageAction::RecordHandoff { .. }
                | RecordedMessageAction::PrepareAppend { .. }
                | RecordedMessageAction::RecordAppendStored { .. }
                | RecordedMessageAction::PrepareInput { .. }
                | RecordedMessageAction::RecordInputDispatched { .. } => {
                    unreachable!("handled above")
                }
            }
        }
        tx.commit()?;
        Ok(reply)
    }
}

impl SessionBinding<'_> {
    /// Resolve retained candidate evidence for this exact historical operation.
    /// A returned value is not qualified acknowledgement or model consumption.
    pub fn message_handoff(
        &self,
        operation: &str,
    ) -> Result<Option<MessageHandoffRecord>, IntegrationError> {
        let connection = self.service.connection()?;
        let receipt = self.message_status_record(&connection, operation)?;
        let body: Option<String> = connection
            .query_row(
                "SELECT body FROM message_handoffs WHERE installation_id=?1 AND operation_id=?2",
                params![self.reference.installation_id, operation],
                |r| r.get(0),
            )
            .optional()?;
        let record = body
            .map(|v| serde_json::from_str::<MessageHandoffRecord>(&v))
            .transpose()?;
        if let Some(record) = &record {
            if record.schema_version != 1
                || record.reservation != reservation_from_receipt(&receipt)?
            {
                return Err(IntegrationError::Invalid(
                    "stored handoff scope does not match".into(),
                ));
            }
            validate_handoff(&record.outcome)?;
        }
        Ok(record)
    }

    fn record_handoff_record(
        &self,
        tx: &rusqlite::Connection,
        operation: &str,
        reservation_id: &str,
        outcome: &MessageHandoffOutcome,
    ) -> Result<(), IntegrationError> {
        validate_handoff(outcome)?;
        let receipt = self.message_status_record(tx, operation)?;
        let reservation = reservation_from_receipt(&receipt)?;
        if reservation.reservation_id != reservation_id {
            return Err(IntegrationError::Conflict(
                "handoff reservation does not match".into(),
            ));
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT body FROM message_handoffs WHERE installation_id=?1 AND operation_id=?2",
                params![self.reference.installation_id, operation],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(body) = existing {
            let existing: MessageHandoffRecord = serde_json::from_str(&body)?;
            if existing.schema_version != 1
                || existing.reservation != reservation
                || existing.outcome != *outcome
            {
                return Err(IntegrationError::Conflict("handoff replay changed".into()));
            }
            return Ok(());
        }
        let recorded_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|v| u64::try_from(v.as_millis()).ok())
            .ok_or_else(|| IntegrationError::Invalid("system clock unavailable".into()))?;
        let record = MessageHandoffRecord {
            schema_version: 1,
            reservation,
            outcome: outcome.clone(),
            recorded_at_unix_ms,
        };
        tx.execute(
            "INSERT INTO message_handoffs(installation_id,operation_id,body) VALUES (?1,?2,?3)",
            params![
                self.reference.installation_id,
                operation,
                serde_json::to_string(&record)?
            ],
        )?;
        // Without qualified proof the only conservative projection is Unknown.
        // Late qualified evidence may already have resolved this operation; leave
        // that independent projection untouched.
        if receipt.state == MessageState::Dispatching {
            let mut uncertain = receipt;
            uncertain.state = MessageState::Unknown;
            tx.execute(
                "UPDATE outgoing_messages SET body=?3 WHERE installation_id=?1 AND operation_id=?2",
                params![
                    self.reference.installation_id,
                    operation,
                    serde_json::to_string(&uncertain)?
                ],
            )?;
        }
        Ok(())
    }

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

fn validate_handoff(outcome: &MessageHandoffOutcome) -> Result<(), IntegrationError> {
    match outcome {
        MessageHandoffOutcome::Returned { result }
            if serde_json::to_vec(result)?.len() <= 98304 =>
        {
            Ok(())
        }
        MessageHandoffOutcome::Uncertain { reason }
            if matches!(
                reason.as_str(),
                "native_submit_threw" | "native_receipt_unavailable"
            ) =>
        {
            Ok(())
        }
        _ => Err(IntegrationError::Invalid(
            "invalid or oversized native handoff outcome".into(),
        )),
    }
}

fn reservation_from_receipt(
    receipt: &super::MessageReceipt,
) -> Result<MessageReservation, IntegrationError> {
    Ok(MessageReservation {
        binding_id: receipt.binding_id.clone(),
        target: receipt.target.clone(),
        request: receipt.request.clone(),
        payload_sha256: receipt.payload_sha256.clone(),
        reservation_id: receipt.reservation_id.clone().ok_or_else(|| {
            IntegrationError::Conflict("message has no handoff reservation".into())
        })?,
    })
}
