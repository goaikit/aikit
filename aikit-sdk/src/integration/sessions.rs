//! Bind application observation to a recorded native session start. These handles
//! own no native process or transport. They never spawn, signal or terminate one.
use super::{
    HookEvent, HookPage, HookRequest, InstallationStatus, IntegrationError, IntegrationService,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

/// A journal-qualified identity, not proof of native process identity. A later
/// SessionStart (including resume/compaction) invalidates the reference. Native
/// Pi wire v2 adds an extension-issued invocation scope. Other adapters lack it.
/// This is not process authentication or an existing-session delivery guarantee.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionRef {
    pub installation_id: String,
    pub installation_revision: String,
    pub native_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_id: Option<String>,
    pub start_cursor: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Observed,
    Ended,
    Stale,
    Detached,
}

/// Durable application binding. Dropping a handle only releases the Rust borrow;
/// reopen it with `binding(id)`. Explicit detach revokes all handles for its ID.
/// There are no subscriptions or native resources to release on Drop.
pub struct SessionBinding<'a> {
    service: &'a IntegrationService,
    id: String,
    reference: SessionRef,
}

impl IntegrationService {
    pub(super) fn installation_revision(&self, id: &str) -> Result<String, IntegrationError> {
        revision(&self.connection()?, id)?.ok_or(IntegrationError::NotFound)
    }

    /// Resolve an already observed native session. Absence of SessionStart is not
    /// repaired by inventing a process generation or launching/resuming an agent.
    pub fn observed_session(
        &self,
        installation_id: &str,
        native_session_id: &str,
    ) -> Result<SessionRef, IntegrationError> {
        if native_session_id.trim().is_empty()
            || native_session_id.len() > 256
            || native_session_id.contains('\0')
        {
            return Err(IntegrationError::Invalid(
                "invalid native session ID".into(),
            ));
        }
        let installation = self.installed(installation_id)?;
        if !installation
            .spec
            .events
            .contains(&HookEvent::SessionStarted)
        {
            return Err(IntegrationError::Unsupported(
                "session binding requires SessionStarted observations".into(),
            ));
        }
        let connection = self.connection()?;
        let installation_revision =
            revision(&connection, installation_id)?.ok_or(IntegrationError::NotFound)?;
        let mut reference = SessionRef {
            installation_id: installation_id.into(),
            installation_revision,
            native_session_id: native_session_id.into(),
            invocation_id: None,
            start_cursor: 0,
        };
        reference.start_cursor =
            latest_start(&connection, &reference)?.ok_or(IntegrationError::NotFound)?;
        reference.invocation_id = start_invocation(&connection, reference.start_cursor)?;
        Ok(reference)
    }

    /// Resolve this particular invocation, not merely the latest reuse of its
    /// native session ID. Consumers should use this when admitting a hook.
    pub fn session_for_hook(&self, request: &HookRequest) -> Result<SessionRef, IntegrationError> {
        let reference = self.observed_session(&request.installation_id, &request.session_id)?;
        if reference.invocation_id != request.invocation_id {
            return Err(IntegrationError::StaleSession);
        }
        Ok(reference)
    }

    /// Idempotently bind this installation's application to a current observation.
    /// Does not grant workspace admission or prove native control capabilities.
    pub fn bind_existing(
        &self,
        reference: &SessionRef,
    ) -> Result<SessionBinding<'_>, IntegrationError> {
        if !matches!(
            self.installation_status(&reference.installation_id)?,
            InstallationStatus::Configured { .. }
        ) {
            return Err(IntegrationError::StaleSession);
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match reference_status(&tx, reference)? {
            SessionStatus::Observed => {}
            SessionStatus::Ended => return Err(IntegrationError::SessionEnded),
            _ => return Err(IntegrationError::StaleSession),
        }
        let body = serde_json::to_string(reference)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM session_bindings WHERE reference=?1 AND detached=0",
                [&body],
                |r| r.get(0),
            )
            .optional()?;
        let id = existing.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        tx.execute(
            "INSERT OR IGNORE INTO session_bindings(id,reference,detached) VALUES (?1,?2,0)",
            params![id, body],
        )?;
        tx.commit()?;
        Ok(SessionBinding {
            service: self,
            id,
            reference: reference.clone(),
        })
    }

    /// Reopen a persisted handle for status, replay or explicit detach. Reopening
    /// does not reactivate a detached binding or silently bind a replacement.
    pub fn binding(&self, id: &str) -> Result<SessionBinding<'_>, IntegrationError> {
        let body: String = self
            .connection()?
            .query_row(
                "SELECT reference FROM session_bindings WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(IntegrationError::NotFound)?;
        Ok(SessionBinding {
            service: self,
            id: id.into(),
            reference: serde_json::from_str(&body)?,
        })
    }
}

impl SessionBinding<'_> {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn reference(&self) -> &SessionRef {
        &self.reference
    }

    pub fn status(&self) -> Result<SessionStatus, IntegrationError> {
        let connection = self.service.connection()?;
        let detached: bool = connection.query_row(
            "SELECT detached FROM session_bindings WHERE id=?1",
            [&self.id],
            |r| r.get(0),
        )?;
        if detached {
            return Ok(SessionStatus::Detached);
        }
        if !matches!(
            self.service
                .installation_status(&self.reference.installation_id)?,
            InstallationStatus::Configured { .. }
        ) {
            return Ok(SessionStatus::Stale);
        }
        reference_status(&connection, &self.reference)
    }

    /// Reuse the installation journal and its immutable cursors. `limit` bounds
    /// scanned records; a page may be empty but advance over another session.
    /// Ended sessions remain readable; stale/detached handles do not follow a
    /// replacement. Historical installation records remain available separately.
    pub fn events(&self, after: u64, limit: u32) -> Result<HookPage, IntegrationError> {
        self.ensure_readable()?;
        let after = after.max(self.reference.start_cursor.saturating_sub(1));
        let mut page = self
            .service
            .events(&self.reference.installation_id, after, limit)?;
        page.records.retain(|record| {
            record.request.session_id == self.reference.native_session_id
                && record.installation_revision.as_deref()
                    == Some(&self.reference.installation_revision)
                && record.request.invocation_id == self.reference.invocation_id
        });
        // Recheck after reading so replacement/detach during the read cannot
        // produce a successful page containing the replacement's observations.
        self.ensure_readable()?;
        Ok(page)
    }

    fn ensure_readable(&self) -> Result<(), IntegrationError> {
        match self.status()? {
            SessionStatus::Observed | SessionStatus::Ended => Ok(()),
            SessionStatus::Detached => Err(IntegrationError::Detached),
            SessionStatus::Stale => Err(IntegrationError::StaleSession),
        }
    }

    /// Revoke only this application handle. No provider command is sent; installed
    /// hooks and historical records remain unchanged. Repeating detach is safe.
    pub fn detach(&self) -> Result<(), IntegrationError> {
        self.service.connection()?.execute(
            "UPDATE session_bindings SET detached=1 WHERE id=?1",
            [&self.id],
        )?;
        Ok(())
    }
}

fn revision(connection: &Connection, id: &str) -> Result<Option<String>, IntegrationError> {
    Ok(connection
        .query_row(
            "SELECT revision FROM installation_revisions WHERE installation_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?)
}

fn latest_start(
    connection: &Connection,
    reference: &SessionRef,
) -> Result<Option<u64>, IntegrationError> {
    let cursor: Option<i64> = connection.query_row("SELECT max(sequence) FROM hook_invocations WHERE installation_id=?1 AND installation_revision=?2 AND session_id=?3 AND decision IS NULL AND json_extract(body,'$.event')='session_started'", params![reference.installation_id, reference.installation_revision, reference.native_session_id], |r| r.get(0))?;
    Ok(cursor.map(|c| c as u64))
}

fn reference_status(
    connection: &Connection,
    reference: &SessionRef,
) -> Result<SessionStatus, IntegrationError> {
    if reference.start_cursor == 0
        || reference.start_cursor > i64::MAX as u64
        || revision(connection, &reference.installation_id)?.as_deref()
            != Some(&reference.installation_revision)
        || latest_start(connection, reference)? != Some(reference.start_cursor)
        || start_invocation(connection, reference.start_cursor)? != reference.invocation_id
    {
        return Ok(SessionStatus::Stale);
    }
    let ended: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM hook_invocations WHERE installation_id=?1 AND installation_revision=?2 AND session_id=?3 AND sequence>?4 AND decision IS NULL AND json_extract(body,'$.event')='session_ended' AND json_extract(body,'$.invocation_id') IS ?5)", params![reference.installation_id,reference.installation_revision,reference.native_session_id,reference.start_cursor as i64,reference.invocation_id], |r| r.get(0))?;
    Ok(if ended {
        SessionStatus::Ended
    } else {
        SessionStatus::Observed
    })
}

fn start_invocation(
    connection: &Connection,
    cursor: u64,
) -> Result<Option<String>, IntegrationError> {
    Ok(connection.query_row(
        "SELECT json_extract(body,'$.invocation_id') FROM hook_invocations WHERE sequence=?1",
        [cursor as i64],
        |r| r.get(0),
    )?)
}

/// Called in the same write transaction as journal append. No transaction spans
/// application code. Legacy adapters with no invocation evidence keep their
/// explicitly unqualified identity semantics.
pub(super) fn validate_invocation(
    connection: &Connection,
    request: &HookRequest,
    revision: &str,
) -> Result<(), IntegrationError> {
    let Some(invocation) = &request.invocation_id else {
        return Ok(());
    };
    if request.event == HookEvent::SessionStarted {
        let reused: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM hook_invocations WHERE installation_id=?1 AND json_extract(body,'$.invocation_id')=?2 AND json_extract(body,'$.event')='session_started')",
            params![request.installation_id, invocation], |r| r.get(0),
        )?;
        if reused {
            return Err(IntegrationError::StaleSession);
        }
        return Ok(());
    }
    let reference = SessionRef {
        installation_id: request.installation_id.clone(),
        installation_revision: revision.into(),
        native_session_id: request.session_id.clone(),
        invocation_id: Some(invocation.clone()),
        start_cursor: 0,
    };
    let cursor = latest_start(connection, &reference)?.ok_or(IntegrationError::StaleSession)?;
    let reference = SessionRef {
        start_cursor: cursor,
        ..reference
    };
    match reference_status(connection, &reference)? {
        SessionStatus::Observed => Ok(()),
        SessionStatus::Ended => Err(IntegrationError::SessionEnded),
        _ => Err(IntegrationError::StaleSession),
    }
}
