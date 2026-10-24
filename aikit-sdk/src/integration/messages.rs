//! Durable local transport records for existing sessions. These primitives send
//! nothing, attest no native caller and grant no provider capability. A qualified
//! adapter must supply admission through handoff and resolve native proof refs.
use super::{IntegrationError, SessionBinding, SessionRef, SessionStatus};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MESSAGE_CONTENT_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessageRequest {
    pub operation_id: String,
    /// Exact UTF-8 bytes. Native framing/limits belong to the adapter; no truncation.
    pub content: String,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageState {
    Accepted,
    Dispatching,
    Acknowledged,
    Unknown,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MessageProof {
    Acknowledgement {
        receipt_reference: String,
    },
    Consumption {
        input_reference: String,
        event_reference: String,
    },
    Failure {
        failure_reference: String,
    },
}
/// Trusted adapter input, not an owner API or authenticated native event. Merely
/// storing references does not qualify them; they must resolve to retained proof.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessageObservation {
    pub event_id: String,
    pub source: String,
    pub proof: MessageProof,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageEvidence {
    pub cursor: u64,
    /// Local receipt time, assigned once; not the native event's timestamp.
    pub recorded_at_unix_ms: u64,
    pub observation: MessageObservation,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageReceipt {
    pub schema_version: u32,
    pub binding_id: String,
    pub target: SessionRef,
    pub request: MessageRequest,
    pub payload_sha256: String,
    pub state: MessageState,
    pub reservation_id: Option<String>,
    pub acknowledgement: Option<MessageEvidence>,
    pub consumption: Option<MessageEvidence>,
    pub failure: Option<MessageEvidence>,
}
/// One durable reservation. No SQLite transaction/installation lock survives
/// return. This is NOT native idle admission, liveness or authorization to send.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageReservation {
    pub binding_id: String,
    pub target: SessionRef,
    pub request: MessageRequest,
    pub payload_sha256: String,
    pub reservation_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePage {
    pub records: Vec<MessageEvidence>,
    pub next_cursor: u64,
}

fn valid(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.contains('\0')
}
fn conflict() -> IntegrationError {
    IntegrationError::Conflict("message operation/evidence conflicts".into())
}
fn validate_observation(observation: &MessageObservation) -> Result<(), IntegrationError> {
    if !valid(&observation.event_id, 256) || !valid(&observation.source, 1024) {
        return Err(IntegrationError::Invalid(
            "invalid message observation identity/source".into(),
        ));
    }
    let references = match &observation.proof {
        MessageProof::Acknowledgement { receipt_reference } => vec![receipt_reference],
        MessageProof::Consumption {
            input_reference,
            event_reference,
        } => vec![input_reference, event_reference],
        MessageProof::Failure { failure_reference } => vec![failure_reference],
    };
    if references.iter().any(|reference| !valid(reference, 4096)) {
        return Err(IntegrationError::Invalid(
            "invalid native message evidence reference".into(),
        ));
    }
    Ok(())
}
fn decode_evidence(
    cursor: i64,
    event_id: &str,
    body: &str,
) -> Result<MessageEvidence, IntegrationError> {
    let evidence: MessageEvidence = serde_json::from_str(body)?;
    if cursor <= 0
        || evidence.cursor != cursor as u64
        || evidence.recorded_at_unix_ms == 0
        || evidence.observation.event_id != event_id
    {
        return Err(IntegrationError::Invalid(
            "message evidence differs from its journal row".into(),
        ));
    }
    validate_observation(&evidence.observation)?;
    Ok(evidence)
}
fn referenced_evidence<'a>(
    receipt: &'a MessageReceipt,
    proof: &MessageProof,
) -> Option<&'a MessageEvidence> {
    match proof {
        MessageProof::Acknowledgement { .. } => receipt.acknowledgement.as_ref(),
        MessageProof::Consumption { .. } => receipt.consumption.as_ref(),
        MessageProof::Failure { .. } => receipt.failure.as_ref(),
    }
}
fn load(
    connection: &Connection,
    installation: &str,
    operation: &str,
) -> Result<Option<MessageReceipt>, IntegrationError> {
    let body: Option<String> = connection
        .query_row(
            "SELECT body FROM outgoing_messages WHERE installation_id=?1 AND operation_id=?2",
            params![installation, operation],
            |row| row.get(0),
        )
        .optional()?;
    let receipt = body
        .map(|value| serde_json::from_str::<MessageReceipt>(&value))
        .transpose()?;
    if let Some(value) = &receipt {
        let attempt = value
            .reservation_id
            .as_deref()
            .is_some_and(|id| valid(id, 256));
        let ack = value.acknowledgement.is_some();
        let consumed = value.consumption.is_some();
        let failed = value.failure.is_some();
        let phase_valid = match value.state {
            MessageState::Accepted => {
                value.reservation_id.is_none() && !ack && !consumed && !failed
            }
            MessageState::Dispatching | MessageState::Unknown => {
                attempt && !ack && !consumed && !failed
            }
            MessageState::Acknowledged => attempt && (ack || consumed) && !failed,
            MessageState::Failed => attempt && failed && !ack && !consumed,
        };
        if value.schema_version != 1
            || value.target.installation_id != installation
            || value.request.operation_id != operation
            || !phase_valid
            || value.payload_sha256
                != format!("{:x}", Sha256::digest(value.request.content.as_bytes()))
        {
            return Err(IntegrationError::Invalid(
                "invalid stored message schema/scope/state/digest".into(),
            ));
        }
        for (kind, field) in [&value.acknowledgement, &value.consumption, &value.failure]
            .into_iter()
            .enumerate()
        {
            let Some(evidence) = field else {
                continue;
            };
            // Resolve every projected field to this operation's immutable journal
            // row. JSON cursor/kind/reference claims cannot stand in for storage.
            if evidence.cursor > i64::MAX as u64
                || !matches!(
                    (kind, &evidence.observation.proof),
                    (0, MessageProof::Acknowledgement { .. })
                        | (1, MessageProof::Consumption { .. })
                        | (2, MessageProof::Failure { .. })
                )
            {
                return Err(IntegrationError::Invalid(
                    "message receipt evidence has the wrong field or cursor".into(),
                ));
            }
            let stored: Option<(i64,String,String)> = connection.query_row(
                "SELECT cursor,event_id,body FROM message_observations WHERE installation_id=?1 AND operation_id=?2 AND cursor=?3",
                params![installation,operation,evidence.cursor as i64], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
            ).optional()?;
            let (cursor, event_id, body) = stored.ok_or_else(|| {
                IntegrationError::Invalid("message receipt evidence journal row is missing".into())
            })?;
            if decode_evidence(cursor, &event_id, &body)? != *evidence {
                return Err(IntegrationError::Invalid(
                    "message receipt evidence differs from its journal row".into(),
                ));
            }
        }
    }
    Ok(receipt)
}
fn save(connection: &Connection, receipt: &MessageReceipt) -> Result<(), IntegrationError> {
    connection.execute(
        "UPDATE outgoing_messages SET body=?3 WHERE installation_id=?1 AND operation_id=?2",
        params![
            receipt.target.installation_id,
            receipt.request.operation_id,
            serde_json::to_string(receipt)?
        ],
    )?;
    Ok(())
}

impl SessionBinding<'_> {
    /// Accept locally only. The ID is unique within this installation, so retry
    /// against a replacement binding cannot redirect an existing operation.
    /// Exact retries recover the original receipt even after detach/end/staleness.
    pub fn stage_message(
        &self,
        request: MessageRequest,
    ) -> Result<MessageReceipt, IntegrationError> {
        if !valid(&request.operation_id, 256)
            || request.content.is_empty()
            || request.content.len() > MESSAGE_CONTENT_LIMIT
        {
            return Err(IntegrationError::Invalid(
                "invalid message ID/content or content exceeds 4 MiB".into(),
            ));
        }
        let _lock = self.service.state_lock()?;
        let mut connection = self.service.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = load(&tx, &self.reference.installation_id, &request.operation_id)? {
            if existing.binding_id != self.id
                || existing.target != self.reference
                || existing.request != request
            {
                return Err(conflict());
            }
            return Ok(existing);
        }
        self.require_recorded_message_target(&tx)?;
        let receipt = MessageReceipt {
            schema_version: 1,
            binding_id: self.id.clone(),
            target: self.reference.clone(),
            payload_sha256: format!("{:x}", Sha256::digest(request.content.as_bytes())),
            request,
            state: MessageState::Accepted,
            reservation_id: None,
            acknowledgement: None,
            consumption: None,
            failure: None,
        };
        tx.execute(
            "INSERT INTO outgoing_messages(installation_id,operation_id,body) VALUES (?1,?2,?3)",
            params![
                self.reference.installation_id,
                receipt.request.operation_id,
                serde_json::to_string(&receipt)?
            ],
        )?;
        tx.commit()?;
        Ok(receipt)
    }
    fn require_recorded_message_target(
        &self,
        connection: &Connection,
    ) -> Result<(), IntegrationError> {
        match self.status_inner(connection)? {
            SessionStatus::Observed => Ok(()),
            SessionStatus::Ended => Err(IntegrationError::SessionEnded),
            SessionStatus::Detached => Err(IntegrationError::Detached),
            SessionStatus::Stale => Err(IntegrationError::StaleSession),
        }
    }
    /// Historical local lookup; never follows a replacement or sends anything.
    /// Remains available for the original binding after detach/end/replacement.
    pub fn message_status(&self, operation: &str) -> Result<MessageReceipt, IntegrationError> {
        let mut connection = self.service.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
        self.message_status_record(&tx, operation)
    }
    pub(super) fn message_status_record(
        &self,
        connection: &Connection,
        operation: &str,
    ) -> Result<MessageReceipt, IntegrationError> {
        let receipt = load(connection, &self.reference.installation_id, operation)?
            .ok_or(IntegrationError::NotFound)?;
        if receipt.binding_id != self.id || receipt.target != self.reference {
            return Err(IntegrationError::NotFound);
        }
        Ok(receipt)
    }
    /// Record the sole attempt before any possible native handoff. A qualified
    /// adapter must separately hold native admission through I/O. None is supplied
    /// by this local reservation. Never reserve Dispatching/Unknown/Failed again.
    pub fn reserve_message(&self, operation: &str) -> Result<MessageReservation, IntegrationError> {
        let _lock = self.service.state_lock()?;
        let mut connection = self.service.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let reservation = self.reserve_message_record(&tx, operation)?;
        tx.commit()?;
        Ok(reservation)
    }
    pub(super) fn reserve_message_record(
        &self,
        tx: &Connection,
        operation: &str,
    ) -> Result<MessageReservation, IntegrationError> {
        self.require_recorded_message_target(tx)?;
        if self.reference.invocation_id.is_none() {
            return Err(IntegrationError::Unsupported(
                "message reservation requires an observed invocation generation".into(),
            ));
        }
        let mut receipt = load(tx, &self.reference.installation_id, operation)?
            .ok_or(IntegrationError::NotFound)?;
        if receipt.binding_id != self.id
            || receipt.target != self.reference
            || receipt.state != MessageState::Accepted
        {
            return Err(conflict());
        }
        let reservation = MessageReservation {
            binding_id: self.id.clone(),
            target: self.reference.clone(),
            request: receipt.request.clone(),
            payload_sha256: receipt.payload_sha256.clone(),
            reservation_id: uuid::Uuid::new_v4().to_string(),
        };
        receipt.state = MessageState::Dispatching;
        receipt.reservation_id = Some(reservation.reservation_id.clone());
        save(tx, &receipt)?;
        Ok(reservation)
    }
    /// Conservative local recovery, not a native query. Lost handoff/receipt is
    /// Unknown; no reset/resend is performed. Later matching native proof may fill
    /// the independent evidence. Native reconciliation remains adapter work.
    pub fn recover_message_record(
        &self,
        operation: &str,
    ) -> Result<MessageReceipt, IntegrationError> {
        let _lock = self.service.state_lock()?;
        let mut connection = self.service.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut receipt = load(&tx, &self.reference.installation_id, operation)?
            .ok_or(IntegrationError::NotFound)?;
        if receipt.binding_id != self.id || receipt.target != self.reference {
            return Err(IntegrationError::NotFound);
        }
        if receipt.state == MessageState::Dispatching {
            receipt.state = MessageState::Unknown;
            save(&tx, &receipt)?;
        }
        tx.commit()?;
        Ok(receipt)
    }
    /// Trusted adapter projection only. Old-target proof can resolve an uncertain
    /// historical operation; it cannot change its target or initiate another send.
    pub fn record_message_observation(
        &self,
        reservation: &MessageReservation,
        observation: MessageObservation,
    ) -> Result<MessageReceipt, IntegrationError> {
        validate_observation(&observation)?;
        let _lock = self.service.state_lock()?;
        let mut connection = self.service.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut receipt = load(
            &tx,
            &self.reference.installation_id,
            &reservation.request.operation_id,
        )?
        .ok_or(IntegrationError::NotFound)?;
        if receipt.binding_id != self.id
            || receipt.target != self.reference
            || reservation.binding_id != receipt.binding_id
            || reservation.target != receipt.target
            || reservation.request != receipt.request
            || reservation.payload_sha256 != receipt.payload_sha256
            || Some(&reservation.reservation_id) != receipt.reservation_id.as_ref()
        {
            return Err(conflict());
        }
        let replay: Option<(String,i64,String)> = tx.query_row("SELECT operation_id,cursor,body FROM message_observations WHERE installation_id=?1 AND event_id=?2",
            params![self.reference.installation_id,observation.event_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if let Some((operation, cursor, body)) = replay {
            let evidence = decode_evidence(cursor, &observation.event_id, &body)?;
            if operation != receipt.request.operation_id || evidence.observation != observation {
                return Err(conflict());
            }
            if referenced_evidence(&receipt, &evidence.observation.proof) != Some(&evidence) {
                return Err(IntegrationError::Invalid(
                    "message journal evidence is absent from its receipt".into(),
                ));
            }
            return Ok(receipt);
        }
        if matches!(receipt.state, MessageState::Accepted | MessageState::Failed) {
            return Err(conflict());
        }
        match &observation.proof {
            MessageProof::Acknowledgement { .. } if receipt.acknowledgement.is_some() => {
                return Err(conflict())
            }
            MessageProof::Consumption { .. } if receipt.consumption.is_some() => {
                return Err(conflict())
            }
            MessageProof::Failure { .. }
                if receipt.acknowledgement.is_some() || receipt.consumption.is_some() =>
            {
                return Err(conflict())
            }
            _ => {}
        }
        let recorded_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|v| u64::try_from(v.as_millis()).ok())
            .ok_or_else(|| IntegrationError::Invalid("system clock unavailable".into()))?;
        tx.execute("INSERT INTO message_observations(installation_id,operation_id,event_id,body) VALUES (?1,?2,?3,'{}')",
            params![self.reference.installation_id,receipt.request.operation_id,observation.event_id])?;
        let evidence = MessageEvidence {
            cursor: tx.last_insert_rowid() as u64,
            recorded_at_unix_ms,
            observation,
        };
        tx.execute(
            "UPDATE message_observations SET body=?2 WHERE cursor=?1",
            params![evidence.cursor as i64, serde_json::to_string(&evidence)?],
        )?;
        match &evidence.observation.proof {
            MessageProof::Acknowledgement { .. } => {
                receipt.state = MessageState::Acknowledged;
                receipt.acknowledgement = Some(evidence);
            }
            MessageProof::Consumption { .. } => {
                receipt.state = MessageState::Acknowledged;
                receipt.consumption = Some(evidence);
            }
            MessageProof::Failure { .. } => {
                receipt.state = MessageState::Failed;
                receipt.failure = Some(evidence);
            }
        }
        save(&tx, &receipt)?;
        tx.commit()?;
        Ok(receipt)
    }
    /// Replay native evidence separately from the hook journal. Page cursors are
    /// durable append order, not a native causal clock. No proof is synthesized.
    pub fn message_observations(
        &self,
        operation: &str,
        after: u64,
        limit: u32,
    ) -> Result<MessagePage, IntegrationError> {
        if after > i64::MAX as u64 || !(1..=500).contains(&limit) {
            return Err(IntegrationError::Invalid(
                "invalid message observation cursor/limit".into(),
            ));
        }
        let mut connection = self.service.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let receipt = self.message_status_record(&tx, operation)?;
        let mut statement = tx.prepare("SELECT cursor,event_id,body FROM message_observations WHERE installation_id=?1 AND operation_id=?2 AND cursor>?3 ORDER BY cursor LIMIT ?4")?;
        let bodies = statement.query_map(
            params![
                self.reference.installation_id,
                operation,
                after as i64,
                limit
            ],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )?;
        let mut records = Vec::new();
        for body in bodies {
            let (cursor, event_id, body) = body?;
            let evidence = decode_evidence(cursor, &event_id, &body)?;
            if referenced_evidence(&receipt, &evidence.observation.proof) != Some(&evidence) {
                return Err(IntegrationError::Invalid(
                    "message journal evidence is absent from its receipt".into(),
                ));
            }
            records.push(evidence);
        }
        Ok(MessagePage {
            next_cursor: records.last().map_or(after, |e| e.cursor),
            records,
        })
    }
}

#[cfg(test)]
#[path = "messages_tests.rs"]
mod tests;
