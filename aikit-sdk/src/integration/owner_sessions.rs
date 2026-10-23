//! Scoped owner inspection and recorded detachment. No provider process control.
use super::{IntegrationError, IntegrationService, SessionRef, SessionStatus};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordedBinding {
    pub id: String,
    pub reference: SessionRef,
    pub status: SessionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordedBindingsPage {
    pub records: Vec<RecordedBinding>,
    /// Last returned ID, not a claim that another page exists.
    pub next_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordedDetachment {
    pub binding_id: String,
    pub reference: SessionRef,
    /// Current installation admission floor, not a native termination timestamp.
    pub admission_after_cursor: u64,
}

pub(super) fn scope(
    application_id: &str,
    agent_key: &str,
    workspace: &Path,
) -> Result<String, IntegrationError> {
    if crate::agent(agent_key).is_none() {
        return Err(IntegrationError::UnknownAgent(agent_key.into()));
    }
    Ok(super::install::installation_key(
        agent_key,
        application_id,
        &fs::canonicalize(workspace)?,
    ))
}

impl IntegrationService {
    /// Read a bounded snapshot of non-detached handles in one canonical owner
    /// scope. Recorded status never establishes native liveness/readiness.
    pub fn attached_bindings(
        &self,
        application_id: &str,
        agent_key: &str,
        workspace: &Path,
        after_id: Option<&str>,
        limit: u32,
    ) -> Result<RecordedBindingsPage, IntegrationError> {
        if limit == 0
            || limit > 500
            || after_id.is_some_and(|id| id.len() > 256 || id.contains('\0'))
        {
            return Err(IntegrationError::Invalid(
                "binding page requires a valid cursor and limit 1-500".into(),
            ));
        }
        let installation = scope(application_id, agent_key, workspace)?;
        let _guard = self.state_lock()?;
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
        // All rows share this installation. Validate its files once, rather
        // than repeating filesystem hashes for every historical handle.
        let configured = matches!(
            self.status_inner(&tx, &installation)?,
            super::InstallationStatus::Configured { .. }
        );
        let mut query = tx.prepare("SELECT id,reference FROM session_bindings WHERE detached=0 AND json_extract(reference,'$.installation_id')=?1 AND id>?2 ORDER BY id LIMIT ?3")?;
        let mut records = Vec::new();
        for row in query.query_map(params![installation, after_id.unwrap_or(""), limit], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (id, body) = row?;
            let reference = serde_json::from_str(&body)?;
            let status = if configured {
                super::sessions::reference_status(&tx, &reference)?
            } else {
                SessionStatus::Stale
            };
            records.push(RecordedBinding {
                id,
                reference,
                status,
            });
        }
        Ok(RecordedBindingsPage {
            next_id: records.last().map(|r| r.id.clone()),
            records,
        })
    }

    /// Close only the selected recorded handle. Atomically retain a journal
    /// admission floor so a new handle cannot revive an earlier observation.
    /// Existing other handles are untouched; no native command is sent.
    pub fn detach_recorded_binding(
        &self,
        application_id: &str,
        agent_key: &str,
        workspace: &Path,
        binding_id: &str,
    ) -> Result<RecordedDetachment, IntegrationError> {
        self.detach_binding_for_installation(
            binding_id,
            &scope(application_id, agent_key, workspace)?,
        )
    }

    pub(super) fn detach_binding_for_installation(
        &self,
        binding_id: &str,
        installation: &str,
    ) -> Result<RecordedDetachment, IntegrationError> {
        let _guard = self.state_lock()?;
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (body, detached): (String, bool) = tx
            .query_row(
                "SELECT reference,detached FROM session_bindings WHERE id=?1",
                [binding_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(IntegrationError::NotFound)?;
        let reference: SessionRef = serde_json::from_str(&body)?;
        if reference.installation_id != installation {
            return Err(IntegrationError::Conflict(
                "binding belongs to another installation".into(),
            ));
        }
        let previous: Option<i64> = tx
            .query_row(
                "SELECT cursor FROM binding_admission_floors WHERE installation_id=?1",
                [installation],
                |r| r.get(0),
            )
            .optional()?;
        if previous.is_some_and(|cursor| cursor < 0) {
            return Err(IntegrationError::Invalid("negative admission floor".into()));
        }
        let cursor = if let (true, Some(cursor)) = (detached, previous) {
            cursor
        } else {
            let maximum: i64 = tx.query_row(
                "SELECT coalesce(max(sequence),0) FROM hook_invocations",
                [],
                |r| r.get(0),
            )?;
            maximum.max(previous.unwrap_or(0))
        };
        let receipt_cursor = u64::try_from(cursor)
            .map_err(|_| IntegrationError::Invalid("negative journal cursor".into()))?;
        tx.execute(
            "UPDATE session_bindings SET detached=1 WHERE id=?1",
            [binding_id],
        )?;
        tx.execute("INSERT INTO binding_admission_floors VALUES (?1,?2) ON CONFLICT(installation_id) DO UPDATE SET cursor=max(cursor,excluded.cursor)", params![installation,cursor])?;
        tx.commit()?;
        Ok(RecordedDetachment {
            binding_id: binding_id.into(),
            reference,
            admission_after_cursor: receipt_cursor,
        })
    }
}
