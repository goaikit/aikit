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
fn validate_intent(intent: &MessageAppendIntent) -> Result<(), IntegrationError> {
    if !bounded(&intent.native_row_id, 256)
        || !bounded(&intent.native_source, 1024)
        || !digest(&intent.framed_payload_sha256)
    {
        return Err(invalid());
    }
    Ok(())
}
fn validate_stored(stored: &MessageAppendStored) -> Result<(), IntegrationError> {
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
            if value.schema_version != 1
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
        validate_intent(&intent)?;
        let _lock = self.service.state_lock()?;
        let mut connection = self.service.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state = self.append_reservation(&tx, reservation)?;
        if let Some(existing) = self.append_checkpoint(&tx, &reservation.request.operation_id)? {
            if existing.intent != intent {
                return Err(conflict());
            }
            return Ok(existing);
        }
        match self.status_inner(&tx)? {
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
        };
        tx.execute("INSERT INTO message_append_checkpoints(installation_id,operation_id,native_row_id,body) VALUES (?1,?2,?3,?4)",
            params![self.reference.installation_id, reservation.request.operation_id, checkpoint.intent.native_row_id,
                serde_json::to_string(&checkpoint)?])?;
        tx.commit()?;
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
        validate_stored(&stored)?;
        let _lock = self.service.state_lock()?;
        let mut connection = self.service.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.append_reservation(&tx, reservation)?;
        let mut checkpoint = self
            .append_checkpoint(&tx, &reservation.request.operation_id)?
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
        tx.commit()?;
        Ok(checkpoint)
    }
}
