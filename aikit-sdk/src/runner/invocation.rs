//! Spec 013 — the common sub-agent invocation envelope.
//!
//! The caller-facing trust/IO knobs that sit above the Backend/Transport seam:
//! filesystem-trust policy ([`SandboxPolicy`]), the tool-approval axis, the
//! working root, extra writable roots, and the lifecycle toggles. Each Backend
//! declares, per knob, how it can honor a request ([`KnobSupport`]); aikit maps
//! a common request onto each Backend's native mechanism. A security knob a
//! Backend cannot honor at all fails closed ([`resolve_envelope`]) — never a
//! silent downgrade of trust (ADR 0012).
//!
//! This module owns the *contract* (the capability matrix + resolution). The
//! per-Backend argv translation of an honored knob lives in
//! `backends/<name>::argv`. aikit does not synthesize its own OS sandbox here;
//! that is out of scope for aikit-sdk (spec 005 NG-002).

use std::path::PathBuf;

use super::backend::Backend;
use super::types::{KnobSupport, RunOptions, SandboxPolicy};

/// The per-call invocation envelope: every spec-013 knob expressed as data.
/// Built from [`RunOptions`] at the subprocess boundary and mapped per-Backend
/// to native argv. Backend-agnostic; one shape for CLI and (future) `serve`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InvocationEnvelope {
    /// `--sandbox` (D1). `None` = Backend default.
    pub sandbox: Option<SandboxPolicy>,
    /// `--auto-approve` (D1 approval axis).
    pub auto_approve: bool,
    /// `-C, --cd` working root (D2). Mirrors `RunOptions::current_dir`.
    pub working_dir: Option<PathBuf>,
    /// `--add-dir` extra writable roots (D2).
    pub extra_writable_roots: Vec<PathBuf>,
    /// `--output-result` / `-o` (D3).
    pub result_file: Option<PathBuf>,
    /// `--output-schema` (D4 convenience knob).
    pub output_schema: Option<serde_json::Value>,
    /// `--bare` skip user config/hooks/MCP (D6).
    pub bare: bool,
    /// `--ephemeral` do not persist the session (D6).
    pub ephemeral: bool,
    /// `--skip-git-repo-check` (D6).
    pub skip_git_repo_check: bool,
}

impl InvocationEnvelope {
    /// Derive the envelope from a [`RunOptions`]. Pure; tested.
    pub fn from_options(opts: &RunOptions) -> Self {
        InvocationEnvelope {
            sandbox: opts.sandbox,
            auto_approve: opts.auto_approve,
            working_dir: opts.current_dir.clone(),
            extra_writable_roots: opts.extra_writable_roots.clone(),
            result_file: opts.result_file.clone(),
            output_schema: opts.output_schema.clone(),
            bare: opts.bare,
            ephemeral: opts.ephemeral,
            skip_git_repo_check: opts.skip_git_repo_check,
        }
    }

    /// True if the envelope carries any spec-013 knob the Backend must map.
    pub fn is_active(&self) -> bool {
        self.sandbox.is_some()
            || self.auto_approve
            || self.working_dir.is_some()
            || !self.extra_writable_roots.is_empty()
            || self.result_file.is_some()
            || self.output_schema.is_some()
            || self.bare
            || self.ephemeral
            || self.skip_git_repo_check
    }
}

/// A requested security knob the Backend cannot honor. Returned by
/// [`resolve_envelope`] so the caller can fail closed (spec 013 exit code 3)
/// rather than run at a wider trust level than asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedKnob {
    pub backend: Backend,
    pub knob: &'static str,
    pub detail: String,
}

impl std::fmt::Display for UnsupportedKnob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "agent '{}' cannot honor --{}: {}",
            self.backend.key(),
            self.knob,
            self.detail
        )
    }
}

impl std::error::Error for UnsupportedKnob {}

impl Backend {
    /// Per-policy filesystem-trust support (spec 013 D1/D5). Queried for the
    /// *specific* requested policy — one Backend may differ across the three
    /// values. codex/gemini enforce at the OS layer; claude/opencode/aikit only
    /// cooperatively; cursor not at all.
    pub fn sandbox_support(self, policy: SandboxPolicy) -> KnobSupport {
        let _ = policy; // support is uniform per Backend today; param reserved for per-value futures.
        match self {
            Backend::Codex | Backend::Gemini => KnobSupport::SupportedOsEnforced,
            Backend::Claude | Backend::OpenCode | Backend::Aikit => KnobSupport::SupportedAppLevel,
            Backend::Cursor | Backend::Pi => KnobSupport::Unsupported,
        }
    }

    /// `--auto-approve` support (D1 approval axis). Independent of the sandbox
    /// axis on claude/opencode/gemini/aikit; coupled into `--sandbox` on codex.
    pub fn auto_approve_support(self) -> KnobSupport {
        match self {
            Backend::Codex
            | Backend::Claude
            | Backend::Gemini
            | Backend::OpenCode
            | Backend::Aikit => KnobSupport::SupportedAppLevel,
            Backend::Cursor | Backend::Pi => KnobSupport::Unsupported,
        }
    }

    /// `-C, --cd` working-root support (D2). The child cwd is OS-enforced
    /// (`Command::current_dir`) for every Backend, including in-process aikit.
    pub fn working_dir_support(self) -> KnobSupport {
        KnobSupport::SupportedOsEnforced
    }

    /// `--add-dir` extra-writable-roots support (D2). codex (`--add-dir`) and
    /// gemini (`SANDBOX_MOUNTS`) enforce via their sandbox; aikit cooperatively;
    /// claude/opencode/cursor have no native mechanism.
    pub fn extra_writable_roots_support(self) -> KnobSupport {
        match self {
            Backend::Codex | Backend::Gemini => KnobSupport::SupportedOsEnforced,
            Backend::Aikit => KnobSupport::SupportedAppLevel,
            Backend::Claude | Backend::OpenCode | Backend::Cursor | Backend::Pi => {
                KnobSupport::Unsupported
            }
        }
    }

    /// `--output-schema` support (D4 convenience knob). codex and claude have a
    /// native flag; elsewhere aikit validates the final message itself.
    pub fn output_schema_support(self) -> KnobSupport {
        match self {
            Backend::Codex | Backend::Claude => KnobSupport::SupportedOsEnforced,
            Backend::Gemini
            | Backend::OpenCode
            | Backend::Cursor
            | Backend::Aikit
            | Backend::Pi => KnobSupport::Emulated,
        }
    }

    /// `--bare` support (D6). codex (`--ignore-user-config`) and claude
    /// (`--bare`); no native equivalent elsewhere.
    pub fn bare_support(self) -> KnobSupport {
        match self {
            Backend::Codex | Backend::Claude => KnobSupport::SupportedOsEnforced,
            Backend::Gemini
            | Backend::OpenCode
            | Backend::Cursor
            | Backend::Aikit
            | Backend::Pi => KnobSupport::Unsupported,
        }
    }

    /// `--ephemeral` support (D6). codex only today.
    pub fn ephemeral_support(self) -> KnobSupport {
        match self {
            Backend::Codex => KnobSupport::SupportedOsEnforced,
            _ => KnobSupport::Unsupported,
        }
    }

    /// `--skip-git-repo-check` support (D6). codex only today.
    pub fn skip_git_repo_check_support(self) -> KnobSupport {
        match self {
            Backend::Codex => KnobSupport::SupportedOsEnforced,
            _ => KnobSupport::Unsupported,
        }
    }
}

/// Pre-flight resolution (spec 013 D5). Security knobs (`--sandbox`,
/// `--add-dir`) that the Backend cannot honor at all fail closed; convenience
/// knobs never fail. Returns the knob that blocked resolution, if any.
///
/// `SupportedAppLevel` is *not* a failure — it is honored at cooperative
/// fidelity and reported honestly; the caller decides whether that suffices.
pub fn resolve_envelope(backend: Backend, env: &InvocationEnvelope) -> Result<(), UnsupportedKnob> {
    if let Some(policy) = env.sandbox {
        let support = backend.sandbox_support(policy);
        if !support.is_available() {
            return Err(UnsupportedKnob {
                backend,
                knob: "sandbox",
                detail: format!(
                    "policy '{}' is unsupported (no native sandbox mechanism)",
                    policy
                ),
            });
        }
    }
    if !env.extra_writable_roots.is_empty()
        && !backend.extra_writable_roots_support().is_available()
    {
        return Err(UnsupportedKnob {
            backend,
            knob: "add-dir",
            detail: "extra writable roots are unsupported (no native mechanism)".to_string(),
        });
    }
    Ok(())
}

/// Human-readable label for a [`KnobSupport`] value, for `--capabilities`.
pub fn knob_support_label(s: KnobSupport) -> &'static str {
    match s {
        KnobSupport::SupportedOsEnforced => "supported (os-enforced)",
        KnobSupport::SupportedAppLevel => "supported (app-level)",
        KnobSupport::Emulated => "emulated",
        KnobSupport::Unsupported => "unsupported",
    }
}

/// Render a Backend's resolved spec-013 capability matrix (one knob per line),
/// for `aikit agent run --capabilities`. Pure; unit-tested.
pub fn format_capabilities(backend: Backend) -> String {
    let mut out = String::new();
    out.push_str(&format!("backend: {}\n", backend.key()));
    out.push_str("sandbox:\n");
    for &p in SandboxPolicy::ALL {
        out.push_str(&format!(
            "  {:14} {}\n",
            p.as_kebab_str(),
            knob_support_label(backend.sandbox_support(p))
        ));
    }
    for (knob, support) in [
        ("auto-approve", backend.auto_approve_support()),
        ("working-dir", backend.working_dir_support()),
        (
            "extra-writable-roots",
            backend.extra_writable_roots_support(),
        ),
        ("output-schema", backend.output_schema_support()),
        ("bare", backend.bare_support()),
        ("ephemeral", backend.ephemeral_support()),
        ("skip-git-repo-check", backend.skip_git_repo_check_support()),
    ] {
        out.push_str(&format!("{:21} {}\n", knob, knob_support_label(support)));
    }
    out
}

/// Map a run's outcome to the spec-013 D6 exit code. A security-knob fail-closed
/// pre-flight (`InvocationUnsupported`) → 3; a timeout → 124; otherwise the
/// backend's own exit code passes through (0 = success; 130/137/143 are
/// SIGINT/SIGKILL/SIGTERM from the process group; a runtime sandbox violation
/// surfaces as the backend's own non-zero code). `None` status → 1.
pub fn exit_code_for(status_code: Option<i32>, run_error: Option<&super::types::RunError>) -> i32 {
    use super::types::RunError;
    match run_error {
        Some(RunError::InvocationUnsupported(_)) => return 3,
        Some(RunError::TimedOut { .. }) => return 124,
        _ => {}
    }
    status_code.unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::backend::ALL;

    #[test]
    fn sandbox_policy_round_trips() {
        for &p in SandboxPolicy::ALL {
            let s = serde_json::to_string(&p).unwrap();
            let back: SandboxPolicy = serde_json::from_str(&s).unwrap();
            assert_eq!(back, p, "serde round trip for {p:?}");
        }
        assert_eq!(SandboxPolicy::ReadOnly.as_kebab_str(), "read-only");
        assert_eq!(SandboxPolicy::BoundedWrite.as_kebab_str(), "bounded-write");
        assert_eq!(SandboxPolicy::Unrestricted.as_kebab_str(), "unrestricted");
    }

    #[test]
    fn sandbox_policy_parses_canonical_and_legacy_aliases() {
        // canonical
        assert!("read-only".parse::<SandboxPolicy>().unwrap() == SandboxPolicy::ReadOnly);
        assert!("bounded-write".parse::<SandboxPolicy>().unwrap() == SandboxPolicy::BoundedWrite);
        assert!("unrestricted".parse::<SandboxPolicy>().unwrap() == SandboxPolicy::Unrestricted);
        // legacy / native aliases accepted for migration friendliness
        assert!("readonly".parse::<SandboxPolicy>().unwrap() == SandboxPolicy::ReadOnly);
        assert!("workspace-write".parse::<SandboxPolicy>().unwrap() == SandboxPolicy::BoundedWrite);
        assert!(
            "danger-full-access".parse::<SandboxPolicy>().unwrap() == SandboxPolicy::Unrestricted
        );
        assert!("full-access".parse::<SandboxPolicy>().unwrap() == SandboxPolicy::Unrestricted);
        // unknown
        assert!("bogus".parse::<SandboxPolicy>().is_err());
        assert!("".parse::<SandboxPolicy>().is_err());
    }

    #[test]
    fn knob_support_is_available_excludes_unsupported() {
        assert!(KnobSupport::SupportedOsEnforced.is_available());
        assert!(KnobSupport::SupportedAppLevel.is_available());
        assert!(KnobSupport::Emulated.is_available());
        assert!(!KnobSupport::Unsupported.is_available());
    }

    /// The load-bearing spec-013 matrix: sandbox fidelity per Backend, per
    /// policy value. Locked so a future change is a conscious decision.
    #[test]
    fn sandbox_support_matrix() {
        for &policy in SandboxPolicy::ALL {
            assert_eq!(
                Backend::Codex.sandbox_support(policy),
                KnobSupport::SupportedOsEnforced
            );
            assert_eq!(
                Backend::Gemini.sandbox_support(policy),
                KnobSupport::SupportedOsEnforced
            );
            assert_eq!(
                Backend::Claude.sandbox_support(policy),
                KnobSupport::SupportedAppLevel
            );
            assert_eq!(
                Backend::OpenCode.sandbox_support(policy),
                KnobSupport::SupportedAppLevel
            );
            assert_eq!(
                Backend::Aikit.sandbox_support(policy),
                KnobSupport::SupportedAppLevel
            );
            assert_eq!(
                Backend::Cursor.sandbox_support(policy),
                KnobSupport::Unsupported
            );
        }
    }

    #[test]
    fn every_backend_declares_every_knob() {
        // Exhaustive: no panic paths, every Backend answers every knob query.
        for &b in ALL {
            let _ = b.auto_approve_support();
            let _ = b.working_dir_support();
            let _ = b.extra_writable_roots_support();
            let _ = b.output_schema_support();
            let _ = b.bare_support();
            let _ = b.ephemeral_support();
            let _ = b.skip_git_repo_check_support();
            for &p in SandboxPolicy::ALL {
                let _ = b.sandbox_support(p);
            }
        }
    }

    #[test]
    fn working_dir_supported_by_all() {
        for &b in ALL {
            assert_eq!(b.working_dir_support(), KnobSupport::SupportedOsEnforced);
        }
    }

    #[test]
    fn extra_roots_only_codex_gemini_aikit() {
        for &b in ALL {
            let s = b.extra_writable_roots_support();
            match b {
                Backend::Codex | Backend::Gemini => assert_eq!(s, KnobSupport::SupportedOsEnforced),
                Backend::Aikit => assert_eq!(s, KnobSupport::SupportedAppLevel),
                Backend::Claude | Backend::OpenCode | Backend::Cursor | Backend::Pi => {
                    assert_eq!(s, KnobSupport::Unsupported)
                }
            }
        }
    }

    #[test]
    fn output_schema_native_for_codex_claude_emulated_elsewhere() {
        for &b in ALL {
            let s = b.output_schema_support();
            match b {
                Backend::Codex | Backend::Claude => {
                    assert_eq!(s, KnobSupport::SupportedOsEnforced)
                }
                Backend::Gemini
                | Backend::OpenCode
                | Backend::Cursor
                | Backend::Aikit
                | Backend::Pi => {
                    assert_eq!(s, KnobSupport::Emulated)
                }
            }
        }
    }

    #[test]
    fn lifecycle_knobs_codex_only_or_codex_claude() {
        for &b in ALL {
            // bare: codex + claude
            assert_eq!(
                Backend::Codex.bare_support(),
                KnobSupport::SupportedOsEnforced
            );
            assert_eq!(
                Backend::Claude.bare_support(),
                KnobSupport::SupportedOsEnforced
            );
            // ephemeral / skip-git-repo-check: codex only
            assert_eq!(
                Backend::Codex.ephemeral_support(),
                KnobSupport::SupportedOsEnforced
            );
            assert_eq!(
                Backend::Codex.skip_git_repo_check_support(),
                KnobSupport::SupportedOsEnforced
            );
            // every non-codex lifecycle is unsupported except claude bare
            if !matches!(b, Backend::Codex) {
                assert_eq!(b.ephemeral_support(), KnobSupport::Unsupported);
                assert_eq!(b.skip_git_repo_check_support(), KnobSupport::Unsupported);
                if !matches!(b, Backend::Claude) {
                    assert_eq!(b.bare_support(), KnobSupport::Unsupported);
                }
            }
        }
    }

    // ── fail-closed resolution (D5) ───────────────────────────────────────────

    #[test]
    fn resolve_cursor_with_sandbox_fails_closed() {
        let env = InvocationEnvelope {
            sandbox: Some(SandboxPolicy::ReadOnly),
            ..Default::default()
        };
        let err = resolve_envelope(Backend::Cursor, &env).unwrap_err();
        assert_eq!(err.backend, Backend::Cursor);
        assert_eq!(err.knob, "sandbox");
    }

    #[test]
    fn resolve_claude_with_sandbox_passes_app_level() {
        // AppLevel is honored (cooperatively), not a failure.
        let env = InvocationEnvelope {
            sandbox: Some(SandboxPolicy::ReadOnly),
            ..Default::default()
        };
        assert!(resolve_envelope(Backend::Claude, &env).is_ok());
    }

    #[test]
    fn resolve_codex_with_sandbox_passes_os_enforced() {
        let env = InvocationEnvelope {
            sandbox: Some(SandboxPolicy::BoundedWrite),
            ..Default::default()
        };
        assert!(resolve_envelope(Backend::Codex, &env).is_ok());
    }

    #[test]
    fn resolve_extra_roots_fails_closed_on_claude() {
        let env = InvocationEnvelope {
            extra_writable_roots: vec![PathBuf::from("/tmp/x")],
            ..Default::default()
        };
        let err = resolve_envelope(Backend::Claude, &env).unwrap_err();
        assert_eq!(err.knob, "add-dir");
        // codex/gemini/aikit honor it
        assert!(resolve_envelope(Backend::Codex, &env).is_ok());
        assert!(resolve_envelope(Backend::Gemini, &env).is_ok());
        assert!(resolve_envelope(Backend::Aikit, &env).is_ok());
    }

    #[test]
    fn resolve_empty_envelope_always_ok() {
        let env = InvocationEnvelope::default();
        for &b in ALL {
            assert!(
                resolve_envelope(b, &env).is_ok(),
                "empty envelope for {b:?}"
            );
        }
    }

    #[test]
    fn resolve_convenience_knobs_never_fail() {
        // output_schema/bare/ephemeral are convenience knobs — never fail closed.
        let env = InvocationEnvelope {
            output_schema: Some(serde_json::json!({"type": "object"})),
            bare: true,
            ephemeral: true,
            skip_git_repo_check: true,
            ..Default::default()
        };
        for &b in ALL {
            assert!(
                resolve_envelope(b, &env).is_ok(),
                "convenience knobs for {b:?}"
            );
        }
    }

    #[test]
    fn unsupported_knob_display_names_backend_and_knob() {
        let err = resolve_envelope(
            Backend::Cursor,
            &InvocationEnvelope {
                sandbox: Some(SandboxPolicy::ReadOnly),
                ..Default::default()
            },
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("cursor"), "msg: {msg}");
        assert!(msg.contains("--sandbox"), "msg: {msg}");
    }

    // ── InvocationEnvelope ─────────────────────────────────────────────────────

    #[test]
    fn envelope_from_options_maps_fields() {
        let opts = RunOptions::default()
            .with_sandbox(SandboxPolicy::BoundedWrite)
            .with_auto_approve(true)
            .with_current_dir(PathBuf::from("/repo"))
            .with_extra_writable_root(PathBuf::from("/shared"))
            .with_result_file(PathBuf::from("/r.txt"))
            .with_output_schema(serde_json::json!({"type": "string"}))
            .with_bare(true)
            .with_ephemeral(true)
            .with_skip_git_repo_check(true);
        let env = InvocationEnvelope::from_options(&opts);
        assert_eq!(env.sandbox, Some(SandboxPolicy::BoundedWrite));
        assert!(env.auto_approve);
        assert_eq!(
            env.working_dir.as_deref(),
            Some(std::path::Path::new("/repo"))
        );
        assert_eq!(env.extra_writable_roots, vec![PathBuf::from("/shared")]);
        assert_eq!(
            env.result_file.as_deref(),
            Some(std::path::Path::new("/r.txt"))
        );
        assert!(env.output_schema.is_some());
        assert!(env.bare && env.ephemeral && env.skip_git_repo_check);
        assert!(env.is_active());
    }

    #[test]
    fn empty_envelope_is_inactive() {
        assert!(!InvocationEnvelope::default().is_active());
        // a single convenience knob activates it
        assert!(InvocationEnvelope {
            bare: true,
            ..Default::default()
        }
        .is_active());
    }

    // ── --capabilities + exit-code helpers (slice 2) ───────────────────────────

    #[test]
    fn knob_support_label_all_variants() {
        assert_eq!(
            knob_support_label(KnobSupport::SupportedOsEnforced),
            "supported (os-enforced)"
        );
        assert_eq!(
            knob_support_label(KnobSupport::SupportedAppLevel),
            "supported (app-level)"
        );
        assert_eq!(knob_support_label(KnobSupport::Emulated), "emulated");
        assert_eq!(knob_support_label(KnobSupport::Unsupported), "unsupported");
    }

    #[test]
    fn format_capabilities_codex_matrix() {
        let s = format_capabilities(Backend::Codex);
        assert!(s.contains("backend: codex"), "{}", s);
        assert!(s.contains("sandbox:"), "{}", s);
        assert!(s.contains("read-only"), "{}", s);
        assert!(s.contains("bounded-write"), "{}", s);
        assert!(s.contains("unrestricted"), "{}", s);
        // codex sandbox is OS-enforced for every policy
        assert!(s.contains("supported (os-enforced)"), "{}", s);
        // every knob row is present
        for knob in [
            "auto-approve",
            "working-dir",
            "extra-writable-roots",
            "output-schema",
            "bare",
            "ephemeral",
            "skip-git-repo-check",
        ] {
            assert!(s.contains(knob), "missing knob {knob}:\n{s}");
        }
    }

    #[test]
    fn format_capabilities_cursor_marks_unsupported() {
        let s = format_capabilities(Backend::Cursor);
        assert!(s.contains("backend: cursor"), "{}", s);
        assert!(s.contains("unsupported"), "{}", s);
    }

    #[test]
    fn exit_code_for_maps_spec013_d6_table() {
        use crate::runner::types::RunError;
        use std::time::Duration;

        assert_eq!(exit_code_for(Some(0), None), 0); // success
        assert_eq!(exit_code_for(None, None), 1); // missing status => 1
        assert_eq!(exit_code_for(Some(130), None), 130); // SIGINT
        assert_eq!(exit_code_for(Some(143), None), 143); // SIGTERM
        assert_eq!(exit_code_for(Some(137), None), 137); // SIGKILL
        assert_eq!(exit_code_for(Some(7), None), 7); // backend non-zero pass-through

        // fail-closed pre-flight => 3 (overrides status)
        let unsupported = RunError::InvocationUnsupported(UnsupportedKnob {
            backend: Backend::Cursor,
            knob: "sandbox",
            detail: "x".into(),
        });
        assert_eq!(exit_code_for(Some(0), Some(&unsupported)), 3);

        // timeout => 124
        let timed_out = RunError::TimedOut {
            timeout: Duration::from_secs(10),
            stdout: vec![],
            stderr: vec![],
        };
        assert_eq!(exit_code_for(None, Some(&timed_out)), 124);
    }
}
