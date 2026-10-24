//! Provider resolution for optional physical caller checks. No capability grant.
use super::{IntegrationError, IntegrationService, RecordedSessionGuard, SessionBinding};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Independently owner-selected executable identity, never accepted from native
/// request data. The initial supported implementation is Linux Claude's existing
/// inbox peer/ancestor relationship; other platforms/providers fail explicitly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeCallerPolicy {
    LinuxExecutable { sha256: String },
}

impl NativeCallerPolicy {
    pub fn read(reader: impl std::io::Read) -> Result<Self, IntegrationError> {
        use std::io::Read;
        let mut bytes = Vec::new();
        reader.take(4097).read_to_end(&mut bytes)?;
        if bytes.len() > 4096 {
            return Err(IntegrationError::Invalid(
                "native caller policy exceeds 4096 bytes".into(),
            ));
        }
        let policy: Self = serde_json::from_slice(&bytes)?;
        policy.validate()?;
        Ok(policy)
    }
    pub fn validate(&self) -> Result<(), IntegrationError> {
        let Self::LinuxExecutable { sha256 } = self;
        if sha256.len() != 64
            || !sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(IntegrationError::Invalid(
                "native caller policy requires an independently selected lowercase SHA-256".into(),
            ));
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub(super) fn inspect(
        &self,
        agent: &str,
    ) -> Result<super::LinuxNativeCaller, IntegrationError> {
        self.validate()?;
        if agent != "claude" {
            return Err(unsupported());
        }
        let Self::LinuxExecutable { sha256 } = self;
        let address = std::env::var("CLAUDE_CODE_MESSAGING_SOCKET").map_err(|_| {
            IntegrationError::Invalid("native caller address is unavailable".into())
        })?;
        super::inspect_linux_native_caller(&address, sha256)
    }

    #[cfg(target_os = "linux")]
    pub(super) fn inspect_binding(
        &self,
        binding: &SessionBinding<'_>,
    ) -> Result<super::RecordedLinuxNativeCaller, IntegrationError> {
        self.validate()?;
        if binding
            .service
            .installed(&binding.reference().installation_id)?
            .spec
            .agent_key
            != "claude"
        {
            return Err(unsupported());
        }
        let Self::LinuxExecutable { sha256 } = self;
        binding.reinspect_recorded_caller(sha256)
    }
}

fn unsupported() -> IntegrationError {
    IntegrationError::Unsupported(
        "physical native caller policy is not implemented for this provider/platform".into(),
    )
}

impl SessionBinding<'_> {
    /// Check a native callback without granting recorded mutation/idle admission.
    /// Useful before retaining SDK checkpoints. No policy cannot bypass an
    /// association; historical owner lookup should not call this method.
    pub fn check_native_caller(
        &self,
        policy: Option<&NativeCallerPolicy>,
    ) -> Result<(), IntegrationError> {
        let Some(policy) = policy else {
            let associated: bool = self.service.connection()?.query_row(
                "SELECT EXISTS(SELECT 1 FROM linux_native_callers WHERE binding_id=?1)",
                [self.id()],
                |row| row.get(0),
            )?;
            return if associated {
                Err(IntegrationError::Invalid(
                    "native caller policy is required for this associated binding".into(),
                ))
            } else {
                Ok(())
            };
        };
        policy.validate()?;
        #[cfg(target_os = "linux")]
        {
            self.check_inspected_caller(&policy.inspect_binding(self)?)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(unsupported())
        }
    }

    /// Provider-neutral admission at the actual native callback boundary. With
    /// no policy, ordinary admission still refuses any process-associated row.
    /// With policy, an existing exact association is mandatory; never auto-bind.
    pub fn admit_recorded_for_native_caller(
        &self,
        projected: u64,
        wait: Duration,
        policy: Option<&NativeCallerPolicy>,
    ) -> Result<RecordedSessionGuard, IntegrationError> {
        if wait > Duration::from_millis(250) {
            return Err(IntegrationError::Invalid(
                "recorded admission lock wait exceeds 250 ms".into(),
            ));
        }
        let Some(policy) = policy else {
            return self.admit_recorded_with_wait(projected, wait);
        };
        policy.validate()?;
        #[cfg(target_os = "linux")]
        {
            let inspected = policy.inspect_binding(self)?;
            self.admit_inspected_caller_wait(projected, &inspected, wait)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (projected, wait);
            Err(unsupported())
        }
    }
}

impl IntegrationService {
    /// An additive checked hook entry point. Existing entry points retain their
    /// semantics. A policy never grants readiness or mandatory-handler authority.
    pub async fn handle_hook_with_native_caller(
        &self,
        installation: &str,
        input: &[u8],
        handler: &dyn super::HookHandler,
        policy: Option<&NativeCallerPolicy>,
    ) -> super::HookResponse {
        self.dispatch_hook_with_caller_policy(installation, input, handler, policy)
            .await
            .unwrap_or_else(|error| super::HookResponse::failure(&error))
    }
}
