//! Durable candidate row correlation around native append. These are adapter
//! claims, not authenticated receipt/consumption proof or permission to send.
use super::{IntegrationError, MessageReservation, MessageState, SessionBinding, SessionStatus};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessageAppendIntent {
    /// Stable provider row identity sampled before storage, never a payload search.
    pub native_row_id: String,
    /// Adapter-owned provider/version/event identity; a claim, not attestation.
    pub native_source: String,
    /// Exact provider-framed bytes, separate from the reservation's logical hash.
    pub framed_payload_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessageAppendStored {
    /// May differ from the intent when a competing handler rewrote the message.
    pub stored_payload_sha256: String,
    /// Resolves to retained native result/row evidence; this service does no I/O.
    pub evidence_reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageAppendCheckpoint {
    pub schema_version: u32,
    pub reservation: MessageReservation,
    pub intent: MessageAppendIntent,
    pub prepared_at_unix_ms: u64,
    pub stored: Option<MessageAppendStored>,
    pub stored_at_unix_ms: Option<u64>,
    /// Version-two candidate input evidence. Never delivery/consumption proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<MessageInputCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessageInputIntent {
    pub native_turn_id: String,
    pub step_index: u64,
    pub api_snapshot_sha256: String,
    pub framed_payload_sha256: String,
    pub step_event_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessageInputDispatched {
    pub core_received_sha256: String,
    pub core_trace_sha256: String,
    /// Reference to separately retained native evidence, not an attestation.
    pub evidence_reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessageInputCheckpoint {
    pub intent: MessageInputIntent,
    pub prepared_at_unix_ms: u64,
    pub dispatched: Option<MessageInputDispatched>,
    pub dispatched_at_unix_ms: Option<u64>,
}

pub(super) fn validate_input_intent(intent: &MessageInputIntent) -> Result<(), IntegrationError> {
    if !bounded(&intent.native_turn_id, 256)
        || intent.step_index > 9_007_199_254_740_991
        || !digest(&intent.api_snapshot_sha256)
        || !digest(&intent.framed_payload_sha256)
        || !digest(&intent.step_event_sha256)
    {
        return Err(invalid());
    }
    Ok(())
}
pub(super) fn validate_input_dispatched(
    value: &MessageInputDispatched,
) -> Result<(), IntegrationError> {
    if !digest(&value.core_received_sha256)
        || !digest(&value.core_trace_sha256)
        || !bounded(&value.evidence_reference, 4096)
    {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> IntegrationError {
    IntegrationError::Invalid("invalid native append checkpoint".into())
}
fn conflict() -> IntegrationError {
    IntegrationError::Conflict("native append checkpoint scope/content changed".into())
}
fn bounded(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.contains('\0')
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
pub(super) fn validate_intent(intent: &MessageAppendIntent) -> Result<(), IntegrationError> {
    if !bounded(&intent.native_row_id, 256)
        || !bounded(&intent.native_source, 1024)
        || !digest(&intent.framed_payload_sha256)
    {
        return Err(invalid());
    }
    Ok(())
}
pub(super) fn validate_stored(stored: &MessageAppendStored) -> Result<(), IntegrationError> {
    if !digest(&stored.stored_payload_sha256) || !bounded(&stored.evidence_reference, 4096) {
        return Err(invalid());
    }
    Ok(())
}
fn now() -> Result<u64, IntegrationError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|time| u64::try_from(time.as_millis()).ok())
        .ok_or_else(invalid)
}

impl SessionBinding<'_> {
    fn append_reservation(
        &self,
        connection: &Connection,
        reservation: &MessageReservation,
    ) -> Result<MessageState, IntegrationError> {
        let receipt = self.message_status_record(connection, &reservation.request.operation_id)?;
        if receipt.binding_id != reservation.binding_id
            || receipt.target != reservation.target
            || receipt.request != reservation.request
            || receipt.payload_sha256 != reservation.payload_sha256
            || receipt.reservation_id.as_deref() != Some(reservation.reservation_id.as_str())
        {
            return Err(conflict());
        }
        Ok(receipt.state)
    }

    fn append_checkpoint(
        &self,
        connection: &Connection,
        operation: &str,
    ) -> Result<Option<MessageAppendCheckpoint>, IntegrationError> {
        let receipt = self.message_status_record(connection, operation)?;
        let row: Option<(String, String)> = connection.query_row(
            "SELECT native_row_id,body FROM message_append_checkpoints WHERE installation_id=?1 AND operation_id=?2",
            params![self.reference.installation_id, operation], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        let checkpoint = row
            .as_ref()
            .map(|(_, body)| serde_json::from_str::<MessageAppendCheckpoint>(body))
            .transpose()?;
        if let Some(value) = &checkpoint {
            validate_intent(&value.intent)?;
            if !matches!(value.schema_version, 1 | 2)
                || (value.schema_version == 2) != value.input.is_some()
                || value.reservation.binding_id != receipt.binding_id
                || row.as_ref().map(|(id, _)| id) != Some(&value.intent.native_row_id)
                || value.reservation.target != receipt.target
                || value.reservation.request != receipt.request
                || value.reservation.payload_sha256 != receipt.payload_sha256
                || Some(&value.reservation.reservation_id) != receipt.reservation_id.as_ref()
                || value.stored.is_some() != value.stored_at_unix_ms.is_some()
            {
                return Err(invalid());
            }
            if let Some(stored) = &value.stored {
                validate_stored(stored)?;
            }
            if let Some(input) = &value.input {
                validate_input_intent(&input.intent)?;
                if value
                    .stored
                    .as_ref()
                    .map(|stored| &stored.stored_payload_sha256)
                    != Some(&value.intent.framed_payload_sha256)
                    || input.intent.framed_payload_sha256 != value.intent.framed_payload_sha256
                    || input.prepared_at_unix_ms == 0
                    || input.dispatched.is_some() != input.dispatched_at_unix_ms.is_some()
                {
                    return Err(invalid());
                }
                if let Some(dispatched) = &input.dispatched {
                    validate_input_dispatched(dispatched)?;
                    if dispatched.core_received_sha256 != input.intent.step_event_sha256 {
                        return Err(invalid());
                    }
                }
            }
        }
        Ok(checkpoint)
    }

    /// Historical lookup, including after detach/replacement. A missing row is
    /// unknown, not native absence, delivery failure, or authorization to resend.
    pub fn message_append_checkpoint(
        &self,
        operation: &str,
    ) -> Result<Option<MessageAppendCheckpoint>, IntegrationError> {
        self.append_checkpoint(&self.service.connection()?, operation)
    }

    /// Retain the exact native row identity before invoking its storage handler.
    /// Does not hold a transaction across native I/O, supply native admission or
    /// turn any recorded identity into an authenticated caller. Exact retry is
    /// idempotent; a different row/content or row reused by another operation
    /// conflicts. A new intent requires this original active Dispatching target.
    pub fn prepare_message_append(
        &self,
        reservation: &MessageReservation,
        intent: MessageAppendIntent,
    ) -> Result<MessageAppendCheckpoint, IntegrationError> {
        let _lock = self.service.state_lock()?;
        let mut connection = self.service.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let checkpoint = self.prepare_message_append_record(&tx, reservation, intent)?;
        tx.commit()?;
        Ok(checkpoint)
    }

    pub(super) fn prepare_message_append_record(
        &self,
        tx: &Connection,
        reservation: &MessageReservation,
        intent: MessageAppendIntent,
    ) -> Result<MessageAppendCheckpoint, IntegrationError> {
        validate_intent(&intent)?;
        let state = self.append_reservation(tx, reservation)?;
        if let Some(existing) = self.append_checkpoint(tx, &reservation.request.operation_id)? {
            if existing.intent != intent {
                return Err(conflict());
            }
            return Ok(existing);
        }
        match self.status_inner(tx)? {
            SessionStatus::Observed => (),
            SessionStatus::Ended => return Err(IntegrationError::SessionEnded),
            SessionStatus::Detached => return Err(IntegrationError::Detached),
            SessionStatus::Stale => return Err(IntegrationError::StaleSession),
        }
        if state != MessageState::Dispatching {
            return Err(conflict());
        }
        let reused: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM message_append_checkpoints WHERE installation_id=?1 AND native_row_id=?2)",
            params![self.reference.installation_id, intent.native_row_id], |row| row.get(0))?;
        if reused {
            return Err(conflict());
        }
        let checkpoint = MessageAppendCheckpoint {
            schema_version: 1,
            reservation: reservation.clone(),
            intent,
            prepared_at_unix_ms: now()?,
            stored: None,
            stored_at_unix_ms: None,
            input: None,
        };
        tx.execute("INSERT INTO message_append_checkpoints(installation_id,operation_id,native_row_id,body) VALUES (?1,?2,?3,?4)",
            params![self.reference.installation_id, reservation.request.operation_id, checkpoint.intent.native_row_id,
                serde_json::to_string(&checkpoint)?])?;
        Ok(checkpoint)
    }

    /// Retain a post-storage candidate against its already prepared native row.
    /// Late original-target observations remain recordable after detach/restart;
    /// they never revive the target, acknowledge delivery or change consumption.
    /// A rewritten digest is retained as a mismatch, never replaced with the
    /// original digest. Calling this never sends or retries a native message.
    pub fn record_message_append_stored(
        &self,
        reservation: &MessageReservation,
        native_row_id: &str,
        stored: MessageAppendStored,
    ) -> Result<MessageAppendCheckpoint, IntegrationError> {
        let _lock = self.service.state_lock()?;
        let mut connection = self.service.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let checkpoint =
            self.record_message_append_stored_record(&tx, reservation, native_row_id, stored)?;
        tx.commit()?;
        Ok(checkpoint)
    }

    pub(super) fn record_message_append_stored_record(
        &self,
        tx: &Connection,
        reservation: &MessageReservation,
        native_row_id: &str,
        stored: MessageAppendStored,
    ) -> Result<MessageAppendCheckpoint, IntegrationError> {
        validate_stored(&stored)?;
        self.append_reservation(tx, reservation)?;
        let mut checkpoint = self
            .append_checkpoint(tx, &reservation.request.operation_id)?
            .ok_or(IntegrationError::NotFound)?;
        if checkpoint.intent.native_row_id != native_row_id {
            return Err(conflict());
        }
        if let Some(existing) = &checkpoint.stored {
            if *existing != stored {
                return Err(conflict());
            }
            return Ok(checkpoint);
        }
        checkpoint.stored = Some(stored);
        checkpoint.stored_at_unix_ms = Some(now()?);
        tx.execute("UPDATE message_append_checkpoints SET body=?3 WHERE installation_id=?1 AND operation_id=?2",
            params![self.reference.installation_id, reservation.request.operation_id, serde_json::to_string(&checkpoint)?])?;
        Ok(checkpoint)
    }

    /// Retain one exact pre-dispatch candidate against an already stored row.
    /// Not a native lease or consumption proof. Exact retries are idempotent;
    /// another Turn/step/digest conflicts rather than replacing uncertainty.
    pub fn prepare_message_input(
        &self,
        reservation: &MessageReservation,
        intent: MessageInputIntent,
    ) -> Result<MessageAppendCheckpoint, IntegrationError> {
        let _lock = self.service.state_lock()?;
        let mut connection = self.service.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = self.prepare_message_input_record(&tx, reservation, intent)?;
        tx.commit()?;
        Ok(value)
    }

    pub(super) fn prepare_message_input_record(
        &self,
        tx: &Connection,
        reservation: &MessageReservation,
        intent: MessageInputIntent,
    ) -> Result<MessageAppendCheckpoint, IntegrationError> {
        validate_input_intent(&intent)?;
        self.append_reservation(tx, reservation)?;
        let mut value = self
            .append_checkpoint(tx, &reservation.request.operation_id)?
            .ok_or(IntegrationError::NotFound)?;
        if let Some(input) = &value.input {
            if input.intent != intent {
                return Err(conflict());
            }
            return Ok(value);
        }
        match self.status_inner(tx)? {
            SessionStatus::Observed => (),
            SessionStatus::Ended => return Err(IntegrationError::SessionEnded),
            SessionStatus::Detached => return Err(IntegrationError::Detached),
            SessionStatus::Stale => return Err(IntegrationError::StaleSession),
        }
        if value
            .stored
            .as_ref()
            .map(|stored| &stored.stored_payload_sha256)
            != Some(&value.intent.framed_payload_sha256)
            || intent.framed_payload_sha256 != value.intent.framed_payload_sha256
        {
            return Err(conflict());
        }
        value.schema_version = 2;
        value.input = Some(MessageInputCheckpoint {
            intent,
            prepared_at_unix_ms: now()?,
            dispatched: None,
            dispatched_at_unix_ms: None,
        });
        self.save_append_checkpoint(tx, &value)?;
        Ok(value)
    }

    /// Retain a late observation for the exact prepared step, even after detach.
    /// This never sends, acknowledges, consumes, or revives an original target.
    pub fn record_message_input_dispatched(
        &self,
        reservation: &MessageReservation,
        intent: &MessageInputIntent,
        dispatched: MessageInputDispatched,
    ) -> Result<MessageAppendCheckpoint, IntegrationError> {
        let _lock = self.service.state_lock()?;
        let mut connection = self.service.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value =
            self.record_message_input_dispatched_record(&tx, reservation, intent, dispatched)?;
        tx.commit()?;
        Ok(value)
    }

    pub(super) fn record_message_input_dispatched_record(
        &self,
        tx: &Connection,
        reservation: &MessageReservation,
        intent: &MessageInputIntent,
        dispatched: MessageInputDispatched,
    ) -> Result<MessageAppendCheckpoint, IntegrationError> {
        validate_input_intent(intent)?;
        validate_input_dispatched(&dispatched)?;
        self.append_reservation(tx, reservation)?;
        let mut value = self
            .append_checkpoint(tx, &reservation.request.operation_id)?
            .ok_or(IntegrationError::NotFound)?;
        let input = value.input.as_mut().ok_or(IntegrationError::NotFound)?;
        if &input.intent != intent || dispatched.core_received_sha256 != intent.step_event_sha256 {
            return Err(conflict());
        }
        if let Some(existing) = &input.dispatched {
            if existing != &dispatched {
                return Err(conflict());
            }
            return Ok(value);
        }
        input.dispatched = Some(dispatched);
        input.dispatched_at_unix_ms = Some(now()?);
        self.save_append_checkpoint(tx, &value)?;
        Ok(value)
    }

    fn save_append_checkpoint(
        &self,
        tx: &Connection,
        value: &MessageAppendCheckpoint,
    ) -> Result<(), IntegrationError> {
        tx.execute("UPDATE message_append_checkpoints SET body=?3 WHERE installation_id=?1 AND operation_id=?2",
            params![self.reference.installation_id, value.reservation.request.operation_id, serde_json::to_string(value)?])?;
        Ok(())
    }
}
