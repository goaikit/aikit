//! Contextual external-session contracts. Managed runner flags keep their existing
//! meanings. A version probe and a matching scenario do not attest live settings.
use super::{IntegrationError, IntegrationService, SessionBinding, SessionRef};
use crate::runner::availability::probe_agent_version;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Interactive,
    Print,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationCapability {
    HookInstallation,
    ObservationBinding,
    DurableReplay,
    SafeDetach,
    PreToolDecision,
    CompletionDecision,
    RepeatedCompletionBlocking,
    FinalAnswerCapture,
    FailedCompletionObservation,
    SuccessfulCompletionObservation,
    MessageSubmission,
    MessageReconciliation,
    NativeSessionIdentity,
    HardHookDeadline,
}
const CAPABILITIES: [IntegrationCapability; 14] = [
    IntegrationCapability::HookInstallation,
    IntegrationCapability::ObservationBinding,
    IntegrationCapability::DurableReplay,
    IntegrationCapability::SafeDetach,
    IntegrationCapability::PreToolDecision,
    IntegrationCapability::CompletionDecision,
    IntegrationCapability::RepeatedCompletionBlocking,
    IntegrationCapability::FinalAnswerCapture,
    IntegrationCapability::FailedCompletionObservation,
    IntegrationCapability::SuccessfulCompletionObservation,
    IntegrationCapability::MessageSubmission,
    IntegrationCapability::MessageReconciliation,
    IntegrationCapability::NativeSessionIdentity,
    IntegrationCapability::HardHookDeadline,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Supported,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityAssessment {
    pub capability: IntegrationCapability,
    pub implemented: bool,
    pub support: Support,
    /// Scope and qualification limits are part of the contract, not optional prose.
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityReport {
    pub agent_key: String,
    /// Host of the probe, not the location of an arbitrary remote process.
    pub platform: String,
    pub architecture: String,
    /// Caller-selected intended session mode; not detected process identity.
    pub mode: SessionMode,
    pub version_output: Option<String>,
    pub probe_issue: Option<String>,
    pub assessments: Vec<CapabilityAssessment>,
    /// SDK accepts 1..=60 seconds; effective native limits remain unqualified.
    pub configured_timeout_seconds: (u32, u32),
    pub callback_response_reserve_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityRequirementsError {
    pub unmet: Vec<CapabilityAssessment>,
}
impl std::fmt::Display for CapabilityRequirementsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "external-session requirements are not qualified: ")?;
        for (index, item) in self.unmet.iter().enumerate() {
            if index > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{:?} ({:?})", item.capability, item.support)?;
        }
        Ok(())
    }
}
impl std::error::Error for CapabilityRequirementsError {}

impl CapabilityReport {
    /// Unknown fails a requirement just as Unsupported does. No fallback or
    /// implicit managed-agent launch is permitted by this check.
    pub fn require(
        &self,
        required: &[IntegrationCapability],
    ) -> Result<(), CapabilityRequirementsError> {
        let mut unmet = Vec::new();
        for capability in required {
            if unmet
                .iter()
                .any(|item: &CapabilityAssessment| item.capability == *capability)
            {
                continue;
            }
            let item = self
                .assessments
                .iter()
                .find(|item| item.capability == *capability)
                .cloned()
                .unwrap_or(CapabilityAssessment {
                    capability: *capability,
                    implemented: false,
                    support: Support::Unknown,
                    detail: "Capability absent from this report.".into(),
                });
            if item.support != Support::Supported {
                unmet.push(item);
            }
        }
        if unmet.is_empty() {
            Ok(())
        } else {
            Err(CapabilityRequirementsError { unmet })
        }
    }
}

impl IntegrationService {
    /// Fresh local --version probe using the existing Backend registry, command
    /// resolution and 1.5-second per-candidate timeout. Blocking: async consumers
    /// must use a blocking worker. This starts no session and changes no config.
    pub fn capabilities(
        &self,
        agent_key: &str,
        mode: SessionMode,
    ) -> Result<CapabilityReport, IntegrationError> {
        if crate::agent(agent_key).is_none() {
            return Err(IntegrationError::UnknownAgent(agent_key.into()));
        }
        let probe = probe_agent_version(agent_key).map_err(|reason| reason.to_string());
        Ok(report(
            agent_key,
            std::env::consts::OS,
            std::env::consts::ARCH,
            mode,
            probe,
        ))
    }

    /// Validate required contextual contracts before creating any binding. A
    /// report is not authorization to attach an unobserved or replaced session.
    pub fn bind_existing_requiring(
        &self,
        reference: &SessionRef,
        mode: SessionMode,
        required: &[IntegrationCapability],
    ) -> Result<SessionBinding<'_>, IntegrationError> {
        let installation = self.installed(&reference.installation_id)?;
        self.capabilities(&installation.spec.agent_key, mode)?
            .require(required)
            .map_err(IntegrationError::RequirementsUnmet)?;
        self.bind_existing(reference)
    }
}

fn report(
    agent_key: &str,
    platform: &str,
    architecture: &str,
    mode: SessionMode,
    probe: Result<String, String>,
) -> CapabilityReport {
    let (version_output, probe_issue) = match probe {
        Ok(value) => (Some(value), None),
        Err(issue) => (None, Some(issue)),
    };
    let qualified = agent_key == "claude"
        && platform == "windows"
        && architecture == "x86_64"
        && mode == SessionMode::Print
        && version_output.as_deref() == Some("2.1.269 (Claude Code)");
    let cursor_dispatch_failure = agent_key == "cursor"
        && platform == "windows"
        && architecture == "x86_64"
        && mode == SessionMode::Print
        && version_output.as_deref() == Some("2026.09.02-c22c1a3");
    let pi_boundary_gap = agent_key == "pi"
        && platform == "windows"
        && architecture == "x86_64"
        && mode == SessionMode::Print
        && version_output.as_deref() == Some("1.0.4");
    let assessments = CAPABILITIES.into_iter().map(|capability| {
        use IntegrationCapability::*;
        let (implemented, support, detail) = if agent_key == "cursor" {
            match capability {
                HookInstallation | ObservationBinding | DurableReplay | SafeDetach => (true, Support::Supported, "SDK owned single-workspace configuration, observation binding, journal and detach contracts only. Cursor prompt/tool and session hooks are implemented; native execution and effective settings need qualification."),
                PreToolDecision if cursor_dispatch_failure => (true, Support::Unknown, "Recorded Windows print qualification failure: Cursor's generated PowerShell wrapper failed to parse before invoking the SDK. A matched no-hook Write succeeded; installed hooks failed closed with no SDK observation. Native dispatch requires repair and requalification; other workspaces and modes are not inferred."),
                PreToolDecision | HardHookDeadline => (true, Support::Unknown, "Cursor prompt/tool decisions use native responses and failClosed configuration. Installed-version execution, shell transport and deadlines remain unqualified."),
                _ => (false, Support::Unsupported, "Cursor completion continuation, final-answer capture, accepted completion, native identity and messaging are not implemented by this adapter. Stop follow-up is not an enforced completion proposal."),
            }
        } else if agent_key == "codex" {
            match capability {
                HookInstallation | ObservationBinding | DurableReplay | SafeDetach => (true, Support::Supported, "SDK owned project hooks, observation binding, journal and detach only. Codex must trust the project and exact hook definitions through its native review flow. SDK does not change trust or managed policy."),
                PreToolDecision | CompletionDecision | RepeatedCompletionBlocking | FinalAnswerCapture | HardHookDeadline => (true, Support::Unknown, "Codex decision/final-message translation exists; native execution, trust, effective settings, competing Stop hooks, tool coverage and deadlines are unqualified. PostToolUse is an outcome-unknown observation, not success."),
                _ => (false, Support::Unsupported, "Codex failed/accepted completion observations, native invocation identity and existing-session messaging are not implemented. Interrupt and SessionEnd do not prove successful or failed completion."),
            }
        } else if agent_key == "pi" {
            match capability {
                RepeatedCompletionBlocking if pi_boundary_gap => (true, Support::Unknown, "Recorded Windows Pi 1.0.4 print-mode counterexample: a later extension observed the SDK Block and continue:true, returned continue:false, and Pi settled after one model turn without an Allow. Native continuation decisions are not monotonic; effective extension ordering and enforced blocking remain unqualified."),
                SuccessfulCompletionObservation if pi_boundary_gap => (false, Support::Unsupported, "Recorded Windows Pi 1.0.4 print-mode counterexample: a later extension aborted after the SDK Allow. Both normal and aborted agent_settled events contained only type, with no outcome/proposal ID and no available abort signal. Allow followed by settlement cannot prove accepted completion. A native correlated final-outcome contract is required."),
                HookInstallation | ObservationBinding | DurableReplay | SafeDetach => (true, Support::Supported, "SDK owns one generated Pi extension file, with fingerprint-checked plans/removal, observation bindings and replay. Loading and native project trust are not attested."),
                PreToolDecision | CompletionDecision | RepeatedCompletionBlocking | FinalAnswerCapture | FailedCompletionObservation | HardHookDeadline => (true, Support::Unknown, "Pi process bridge and lifecycle translation exist. Native execution/version, extension ordering, settled-outcome correlation, subagent attribution and deadline behavior remain unqualified. Later boundary handlers or non-runnable context can defeat continuation."),
                NativeSessionIdentity => (true, Support::Unknown, "Pi wire v2 carries a fresh extension-issued invocation ID at each session_start. Shared bindings and journal writes reject old observed invocations, including callbacks replaced during policy. This is not native process authentication, liveness, or proof against a previously unseen delayed start; broader native qualification remains open."),
                _ => (false, Support::Unsupported, "Pi accepted-completion observation and existing-session message delivery/reconciliation are not implemented. A settlement notification alone is not proof that application policy was accepted."),
            }
        } else if agent_key != "claude" {
            (false, Support::Unsupported, "External hook adapter is not implemented for this catalog key; managed runner capabilities do not substitute.")
        } else {
            match capability {
                HookInstallation => (true, Support::Supported, "SDK owned configuration plan/apply/remove contract only. Configured settings do not attest effective native hooks; Windows requires a native executable handler."),
                ObservationBinding => (true, Support::Supported, "SDK binding requires current installation revision and recorded SessionStart. Native process identity and liveness are not established."),
                DurableReplay => (true, Support::Supported, "SDK journal uses immutable local cursors. Observations and prepared decisions are distinct; tool contents are omitted. No native causal ordering is asserted."),
                SafeDetach => (true, Support::Supported, "SDK detach revokes the application binding and sends no native command. It preserves hooks/history. Interactive process-identity qualification remains open."),
                CompletionDecision | FinalAnswerCapture if qualified => (true, Support::Supported, "Bounded Windows Claude Code 2.1.269 print-mode scenario: ten blocked Stops, one Allow, final answer. Fixture disabled the continuation cap. This does not attest effective settings for the caller."),
                FailedCompletionObservation if qualified => (true, Support::Supported, "Bounded Windows Claude Code 2.1.269 print-mode invalid-model scenario emitted StopFailure with no Final Answer. Application projection may require durable replay."),
                PreToolDecision if qualified => (true, Support::Unknown, "One native Write denial prevented file creation on this exact context. Edit, shell, MCP, subagent and effective-settings enforcement remain unqualified; the complete pre-tool requirement is not met."),
                PreToolDecision => (true, Support::Unknown, "Native pre-tool blocking and all edit paths need live qualification for this context; a decoder fixture or allowed Write is insufficient."),
                RepeatedCompletionBlocking => (true, Support::Unknown, "Effective continuation limits and competing settings are not attested. A bounded scenario with an explicit unlimited setting does not prove unconditional enforcement."),
                CompletionDecision | FinalAnswerCapture | FailedCompletionObservation => (true, Support::Unknown, "Adapter exists, but this version/platform/mode has no matching native qualification scenario."),
                SuccessfulCompletionObservation => (false, Support::Unsupported, "No accepted-completion observation is implemented for user-started sessions. Stop Allow and SessionEnd are insufficient."),
                MessageSubmission | MessageReconciliation => (false, Support::Unsupported, "Existing-session native message transport and receipt reconciliation are not implemented. Managed session transports are a different topology."),
                NativeSessionIdentity => (false, Support::Unsupported, "No native invocation nonce or equivalent process identity is implemented; delayed hooks may reuse session IDs."),
                HardHookDeadline => (true, Support::Unknown, "Callback timeout reserves 250 ms for response handling; blocking workers and operating-system I/O do not establish a universal hard deadline."),
            }
        };
        CapabilityAssessment { capability, implemented, support, detail: detail.into() }
    }).collect();
    CapabilityReport {
        agent_key: agent_key.into(),
        platform: platform.into(),
        architecture: architecture.into(),
        mode,
        version_output,
        probe_issue,
        assessments,
        configured_timeout_seconds: (1, 60),
        callback_response_reserve_ms: 250,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_dispatch_failure_is_scoped_without_inventing_an_enforcement_success() {
        for (platform, arch, mode, version, has_failure) in [
            (
                "windows",
                "x86_64",
                SessionMode::Print,
                "2026.09.02-c22c1a3",
                true,
            ),
            (
                "windows",
                "x86_64",
                SessionMode::Interactive,
                "2026.09.02-c22c1a3",
                false,
            ),
            (
                "linux",
                "x86_64",
                SessionMode::Print,
                "2026.09.02-c22c1a3",
                false,
            ),
            (
                "windows",
                "aarch64",
                SessionMode::Print,
                "2026.09.02-c22c1a3",
                false,
            ),
            (
                "windows",
                "x86_64",
                SessionMode::Print,
                "another-version",
                false,
            ),
        ] {
            let report = report("cursor", platform, arch, mode, Ok(version.into()));
            let error = report
                .require(&[IntegrationCapability::PreToolDecision])
                .unwrap_err();
            assert_eq!(error.unmet[0].support, Support::Unknown);
            assert!(error.unmet[0].implemented);
            assert_eq!(
                error.unmet[0].detail.contains("failed to parse"),
                has_failure
            );
            report
                .require(&[IntegrationCapability::HookInstallation])
                .unwrap();
        }
    }
    #[test]
    fn exact_native_evidence_never_promotes_completion_messaging_or_unlimited_enforcement() {
        let report = report(
            "claude",
            "windows",
            "x86_64",
            SessionMode::Print,
            Ok("2.1.269 (Claude Code)".into()),
        );
        report
            .require(&[
                IntegrationCapability::FinalAnswerCapture,
                IntegrationCapability::CompletionDecision,
            ])
            .unwrap();
        let error = report
            .require(&[
                IntegrationCapability::RepeatedCompletionBlocking,
                IntegrationCapability::SuccessfulCompletionObservation,
                IntegrationCapability::MessageSubmission,
                IntegrationCapability::PreToolDecision,
            ])
            .unwrap_err();
        assert_eq!(error.unmet.len(), 4);
        assert_eq!(error.unmet[0].support, Support::Unknown);
        assert_eq!(error.unmet[1].support, Support::Unsupported);
        assert_eq!(error.unmet[3].support, Support::Unknown);
    }
    #[test]
    fn version_mode_platform_and_missing_probe_do_not_inherit_native_qualification() {
        let arm = report(
            "claude",
            "windows",
            "aarch64",
            SessionMode::Print,
            Ok("2.1.269 (Claude Code)".into()),
        );
        assert_eq!(
            arm.require(&[IntegrationCapability::FinalAnswerCapture])
                .unwrap_err()
                .unmet[0]
                .support,
            Support::Unknown
        );
        for (platform, mode, probe) in [
            (
                "linux",
                SessionMode::Print,
                Ok("2.1.269 (Claude Code)".into()),
            ),
            (
                "windows",
                SessionMode::Interactive,
                Ok("2.1.269 (Claude Code)".into()),
            ),
            (
                "windows",
                SessionMode::Print,
                Ok("2.1.270 (Claude Code)".into()),
            ),
            (
                "windows",
                SessionMode::Print,
                Err("binary_not_found".into()),
            ),
        ] {
            let report = report("claude", platform, "x86_64", mode, probe);
            let error = report
                .require(&[IntegrationCapability::FinalAnswerCapture])
                .unwrap_err();
            assert_eq!(error.unmet[0].support, Support::Unknown);
            report
                .require(&[IntegrationCapability::DurableReplay])
                .unwrap();
        }
    }
    #[test]
    fn missing_external_adapters_do_not_borrow_managed_capabilities() {
        let report = report(
            "gemini",
            "windows",
            "x86_64",
            SessionMode::Print,
            Ok("2.1.269 (Claude Code)".into()),
        );
        assert!(report
            .assessments
            .iter()
            .all(|value| !value.implemented && value.support == Support::Unsupported));
    }
    #[test]
    fn absent_and_duplicate_requirements_fail_explicitly() {
        let mut report = report(
            "claude",
            "windows",
            "x86_64",
            SessionMode::Unknown,
            Err("timed_out".into()),
        );
        report.assessments.clear();
        let error = report
            .require(&[
                IntegrationCapability::SafeDetach,
                IntegrationCapability::SafeDetach,
            ])
            .unwrap_err();
        assert_eq!(error.unmet.len(), 1);
        assert_eq!(error.unmet[0].support, Support::Unknown);
    }

    #[test]
    fn pi_boundary_counterexamples_are_scoped_and_never_promote_completion() {
        use IntegrationCapability::{RepeatedCompletionBlocking, SuccessfulCompletionObservation};
        for (platform, architecture, mode, version, matched) in [
            ("windows", "x86_64", SessionMode::Print, "1.0.4", true),
            (
                "windows",
                "x86_64",
                SessionMode::Interactive,
                "1.0.4",
                false,
            ),
            ("linux", "x86_64", SessionMode::Print, "1.0.4", false),
            ("windows", "aarch64", SessionMode::Print, "1.0.4", false),
            ("windows", "x86_64", SessionMode::Print, "1.0.5", false),
        ] {
            let report = report("pi", platform, architecture, mode, Ok(version.into()));
            let error = report
                .require(&[RepeatedCompletionBlocking, SuccessfulCompletionObservation])
                .unwrap_err();
            assert_eq!(error.unmet[0].support, Support::Unknown);
            assert_eq!(error.unmet[1].support, Support::Unsupported);
            for assessment in error.unmet {
                assert_eq!(assessment.detail.contains("counterexample"), matched);
            }
        }
    }
}
