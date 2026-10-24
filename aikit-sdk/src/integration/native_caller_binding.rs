//! Immutable physical caller association over the existing recorded binding.
use super::{
    inspect_linux_native_caller, IntegrationError, LinuxNativeCaller, RecordedSessionGuard,
    SessionBinding, SessionRef, SessionStatus,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{fs, os::unix::fs::MetadataExt, time::Duration};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecordedLinuxNativeCaller {
    pub schema_version: u32,
    pub binding_id: String,
    pub reference: SessionRef,
    pub process: LinuxNativeCaller,
    /// Current kernel boot/PID namespace, not a credential or module identity.
    pub boot_id: String,
    pub pid_namespace: u64,
}

fn invalid() -> IntegrationError {
    IntegrationError::Invalid("recorded Linux native caller scope is invalid".into())
}
fn conflict() -> IntegrationError {
    IntegrationError::Conflict("original Linux native caller association changed".into())
}
fn boot_scope() -> Result<(String, u64), IntegrationError> {
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id").map_err(|_| invalid())?;
    let boot = boot.trim();
    if uuid::Uuid::parse_str(boot).is_err() {
        return Err(invalid());
    }
    let namespace = fs::metadata("/proc/self/ns/pid")
        .map_err(|_| invalid())?
        .ino();
    if namespace == 0 {
        return Err(invalid());
    }
    Ok((boot.into(), namespace))
}

impl SessionBinding<'_> {
    fn inspect_caller(
        &self,
        address: &str,
        expected_hash: &str,
    ) -> Result<RecordedLinuxNativeCaller, IntegrationError> {
        let scope = boot_scope()?;
        let process = inspect_linux_native_caller(address, expected_hash)?;
        if boot_scope()? != scope {
            return Err(conflict());
        }
        Ok(RecordedLinuxNativeCaller {
            schema_version: 1,
            binding_id: self.id().into(),
            reference: self.reference().clone(),
            process,
            boot_id: scope.0,
            pid_namespace: scope.1,
        })
    }

    fn caller_record(
        &self,
        connection: &Connection,
    ) -> Result<Option<RecordedLinuxNativeCaller>, IntegrationError> {
        let value: Option<String> = connection
            .query_row(
                "SELECT body FROM linux_native_callers WHERE binding_id=?1",
                [self.id()],
                |row| row.get(0),
            )
            .optional()?;
        let record: Option<RecordedLinuxNativeCaller> =
            value.as_deref().map(serde_json::from_str).transpose()?;
        if let Some(record) = &record {
            if record.schema_version != 1
                || record.binding_id != self.id()
                || &record.reference != self.reference()
                || uuid::Uuid::parse_str(&record.boot_id).is_err()
                || record.pid_namespace == 0
                || record.process.pid == 0
                || record.process.start_ticks == 0
                || record.process.executable_sha256.len() != 64
                || !record
                    .process
                    .executable_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(invalid());
            }
        }
        Ok(record)
    }

    /// Read retained process association, including after detach/replacement.
    /// Historical facts neither prove liveness nor reactivate the binding.
    pub fn recorded_linux_native_caller(
        &self,
    ) -> Result<Option<RecordedLinuxNativeCaller>, IntegrationError> {
        self.caller_record(&self.service.connection()?)
    }

    /// Inspect this actual caller and bind its current Linux process facts once.
    /// The journal's original reference must still be observed/configured. Exact
    /// repeats are idempotent; changed process, boot, namespace or target conflict.
    /// This enables required physical checks for recorded mutation admission;
    /// it does not attest the reference's native session/module claims or policy.
    /// Native process/proc/executable I/O precedes the short journal transaction.
    pub fn bind_linux_native_caller(
        &self,
        address: &str,
        expected_hash: &str,
    ) -> Result<RecordedLinuxNativeCaller, IntegrationError> {
        let inspected = self.inspect_caller(address, expected_hash)?;
        self.bind_inspected_caller(inspected)
    }

    fn bind_inspected_caller(
        &self,
        inspected: RecordedLinuxNativeCaller,
    ) -> Result<RecordedLinuxNativeCaller, IntegrationError> {
        let _lock = self.service.state_lock()?;
        let mut connection = self.service.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match self.status_inner(&tx)? {
            SessionStatus::Observed => {}
            SessionStatus::Detached => return Err(IntegrationError::Detached),
            SessionStatus::Ended => return Err(IntegrationError::SessionEnded),
            SessionStatus::Stale => return Err(IntegrationError::StaleSession),
        }
        if let Some(existing) = self.caller_record(&tx)? {
            if existing != inspected {
                return Err(conflict());
            }
        } else {
            if inspected.binding_id != self.id() || &inspected.reference != self.reference() {
                return Err(invalid());
            }
            tx.execute(
                "INSERT INTO linux_native_callers(binding_id,body) VALUES (?1,?2)",
                params![self.id(), serde_json::to_string(&inspected)?],
            )?;
        }
        tx.commit()?;
        Ok(inspected)
    }

    /// Inspect the actual current caller, then acquire the existing recorded
    /// admission guard and compare its immutable association under that guard.
    /// Replacement, revocation, unprojected observations and changed process
    /// refuse admission. No record is created or reset on lookup failure.
    /// Hold the returned guard only through a short application commit. It pins
    /// recorded identity, not native process lifetime or an idle/transport lease.
    pub fn admit_recorded_with_linux_native_caller(
        &self,
        projected_through: u64,
        address: &str,
        expected_hash: &str,
    ) -> Result<RecordedSessionGuard, IntegrationError> {
        let inspected = self.inspect_caller(address, expected_hash)?;
        self.admit_inspected_caller(projected_through, &inspected)
    }

    /// Same physical checks with the existing at-most-250-ms journal-lock budget.
    /// No native request or application mutation is retried while waiting.
    pub fn admit_recorded_with_linux_native_caller_wait(
        &self,
        projected_through: u64,
        address: &str,
        expected_hash: &str,
        lock_wait_budget: Duration,
    ) -> Result<RecordedSessionGuard, IntegrationError> {
        if lock_wait_budget > Duration::from_millis(250) {
            return Err(invalid());
        }
        let inspected = self.inspect_caller(address, expected_hash)?;
        self.admit_inspected_caller_wait(projected_through, &inspected, lock_wait_budget)
    }

    fn admit_inspected_caller(
        &self,
        projected_through: u64,
        inspected: &RecordedLinuxNativeCaller,
    ) -> Result<RecordedSessionGuard, IntegrationError> {
        self.admit_inspected_caller_wait(projected_through, inspected, Duration::ZERO)
    }

    fn admit_inspected_caller_wait(
        &self,
        projected_through: u64,
        inspected: &RecordedLinuxNativeCaller,
        lock_wait_budget: Duration,
    ) -> Result<RecordedSessionGuard, IntegrationError> {
        let guard = self.admit_recorded_inner(projected_through, lock_wait_budget, true)?;
        let recorded = self
            .caller_record(&guard.connection)?
            .ok_or(IntegrationError::NotFound)?;
        if &recorded != inspected {
            return Err(conflict());
        }
        Ok(guard)
    }
}

#[cfg(test)]
#[path = "native_caller_binding_tests.rs"]
mod tests;
