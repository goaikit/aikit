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
use super::types::{KnobSupport, RunOptions, SandboxPolicy, SkillIsolation};

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
    /// Restrict the skill surface to one materialized skill (spec 016 D3).
    /// Carried on the envelope so every backend's argv builder sees the
    /// payload (pi needs `--skill <path>`) without an `ArgvCtx` change.
    pub skill_isolation: Option<SkillIsolation>,
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
            skill_isolation: opts.skill_isolation.clone(),
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
            || self.skill_isolation.is_some()
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

    /// `--bare` support (D6). codex (`--ignore-user-config`), claude (`--bare`),
    /// and pi (`--no-context-files`/`--no-extensions`/`--no-skills`/
    /// `--no-prompt-templates` skip user-config discovery); no native
    /// equivalent elsewhere.
    pub fn bare_support(self) -> KnobSupport {
        match self {
            Backend::Codex | Backend::Claude | Backend::Pi => KnobSupport::SupportedOsEnforced,
            Backend::Gemini | Backend::OpenCode | Backend::Cursor | Backend::Aikit => {
                KnobSupport::Unsupported
            }
        }
    }

    /// `--ephemeral` support (D6). codex (`--ephemeral`) and pi
    /// (`--no-session`); no native equivalent elsewhere.
    pub fn ephemeral_support(self) -> KnobSupport {
        match self {
            Backend::Codex | Backend::Pi => KnobSupport::SupportedOsEnforced,
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

    /// `skill_isolation` support (spec 016 D3) — a *fidelity* knob, never
    /// fail-closed (D4). Mechanisms are per-backend and empirically verified
    /// (spec 016 Appendices A/B):
    ///
    /// - claude: `--setting-sources project` (36→19 skills, plugins gone,
    ///   skill under test retained, auth intact). NOT `--bare`, which drops
    ///   the skill under test and breaks OAuth auth.
    /// - codex: scratch `CODEX_HOME` containing a copied `auth.json`. NOT
    ///   `--ignore-user-config`, which is a measured no-op for skills.
    /// - pi: `--no-skills` **paired with** `--skill <path>` — both or
    ///   neither; `--no-skills` alone removes the skill under test.
    /// - gemini/cursor: no per-run mechanism exists (`gemini skills
    ///   enable/disable` is stateful; cursor-agent exposes only `--sandbox`).
    /// - opencode: no mechanism *and* no skills path in the deploy catalog.
    /// - aikit: in-process; `AgentConfig.skills_dirs` is an explicit
    ///   caller-supplied list, so isolation is emulated by pointing it at the
    ///   scratch skills dir only.
    ///
    /// Exhaustive match, no `_` arm: adding a Backend must force an explicit
    /// isolation decision (do not copy `ephemeral_support`'s `_` fallback).
    pub fn skill_isolation_support(self) -> KnobSupport {
        match self {
            Backend::Claude => KnobSupport::SupportedAppLevel,
            Backend::Codex => KnobSupport::SupportedAppLevel,
            Backend::Pi => KnobSupport::SupportedAppLevel,
            Backend::Gemini => KnobSupport::Unsupported,
            Backend::Cursor => KnobSupport::Unsupported,
            Backend::OpenCode => KnobSupport::Unsupported,
            Backend::Aikit => KnobSupport::Emulated,
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
        ("skill-isolation", backend.skill_isolation_support()),
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

/// Env vars to apply to the spawned agent Command for the envelope's sandbox
/// knobs (spec 013 D1/D2). gemini configures its OS sandbox via env
/// (`SEATBELT_PROFILE` / `SANDBOX_MOUNTS`) rather than argv; other backends
/// return empty (their sandbox knobs are argv-mapped or unsupported).
pub fn sandbox_env_for(backend: Backend, env: &InvocationEnvelope) -> Vec<(String, String)> {
    if !matches!(backend, Backend::Gemini) {
        return Vec::new();
    }
    let mut out = Vec::new();
    // read-only ⇒ strict profile (read+write restrictions); bounded-write uses
    // gemini's default permissive-open (writes confined to the workspace);
    // unrestricted leaves the sandbox off (no `-s`).
    if matches!(env.sandbox, Some(SandboxPolicy::ReadOnly)) {
        out.push(("SEATBELT_PROFILE".to_string(), "strict-open".to_string()));
    }
    if !env.extra_writable_roots.is_empty() {
        let mounts: Vec<String> = env
            .extra_writable_roots
            .iter()
            .map(|r| format!("{}:{}:rw", r.display(), r.display()))
            .collect();
        out.push(("SANDBOX_MOUNTS".to_string(), mounts.join(",")));
    }
    out
}

/// Env vars to apply to the spawned agent Command for the envelope's
/// `skill_isolation` knob (spec 016 D3). codex's mechanism is a home-directory
/// swap rather than an argv flag: point `CODEX_HOME` at the caller-allocated
/// scratch home (which holds only a copied `auth.json` — never log it, never
/// retain it). Every other backend maps isolation via argv (or in-process
/// config) and returns empty.
pub fn isolation_env_for(backend: Backend, env: &InvocationEnvelope) -> Vec<(String, String)> {
    if let (Backend::Codex, Some(iso)) = (backend, env.skill_isolation.as_ref()) {
        if let Some(home) = &iso.codex_home {
            return vec![("CODEX_HOME".to_string(), home.display().to_string())];
        }
    }
    Vec::new()
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
            let _ = b.skill_isolation_support();
            for &p in SandboxPolicy::ALL {
                let _ = b.sandbox_support(p);
            }
        }
    }

    fn test_isolation() -> SkillIsolation {
        SkillIsolation {
            workspace_root: PathBuf::from("/scratch/ws"),
            skill_path: PathBuf::from("/scratch/ws/.claude/skills/my-skill"),
            skill_name: "my-skill".to_string(),
            codex_home: None,
        }
    }

    /// The spec-016 D3 matrix, locked value-by-value so a change is a
    /// conscious decision (and exhaustive over ALL so a new Backend that
    /// forgets a row fails here too).
    #[test]
    fn skill_isolation_support_matrix() {
        for &b in ALL {
            let expected = match b {
                Backend::Claude | Backend::Codex | Backend::Pi => KnobSupport::SupportedAppLevel,
                Backend::Gemini | Backend::Cursor | Backend::OpenCode => KnobSupport::Unsupported,
                Backend::Aikit => KnobSupport::Emulated,
            };
            assert_eq!(
                b.skill_isolation_support(),
                expected,
                "skill_isolation_support for {b:?}"
            );
        }
    }

    /// spec 016 D4: isolation is a fidelity knob, not a trust boundary —
    /// resolve_envelope must NOT reject it on any backend, including the ones
    /// that cannot honor it (they run anyway and report degraded fidelity).
    #[test]
    fn resolve_skill_isolation_never_fails_closed() {
        let env = InvocationEnvelope {
            skill_isolation: Some(test_isolation()),
            ..Default::default()
        };
        for &b in ALL {
            assert!(
                resolve_envelope(b, &env).is_ok(),
                "skill_isolation must never fail closed, but did for {b:?}"
            );
        }
    }

    #[test]
    fn isolation_env_codex_home_only() {
        // codex with an allocated scratch home → CODEX_HOME points at it.
        let with_home = InvocationEnvelope {
            skill_isolation: Some(SkillIsolation {
                codex_home: Some(PathBuf::from("/scratch/codex-home")),
                ..test_isolation()
            }),
            ..Default::default()
        };
        assert_eq!(
            isolation_env_for(Backend::Codex, &with_home),
            vec![("CODEX_HOME".to_string(), "/scratch/codex-home".to_string())]
        );
        // codex without a scratch home → no override (degraded, reported by
        // the caller — never a half-configured env).
        let without_home = InvocationEnvelope {
            skill_isolation: Some(test_isolation()),
            ..Default::default()
        };
        assert!(isolation_env_for(Backend::Codex, &without_home).is_empty());
        // every other backend never gets CODEX_HOME.
        for &b in ALL {
            if b != Backend::Codex {
                assert!(
                    isolation_env_for(b, &with_home).is_empty(),
                    "no isolation env expected for {b:?}"
                );
            }
        }
    }

    #[test]
    fn format_capabilities_includes_skill_isolation_row() {
        let s = format_capabilities(Backend::Claude);
        assert!(
            s.lines()
                .any(|l| l.trim_start().starts_with("skill-isolation")
                    && l.contains("supported (app-level)")),
            "claude skill-isolation row missing/wrong:\n{s}"
        );
        let s = format_capabilities(Backend::Aikit);
        assert!(
            s.lines()
                .any(|l| l.trim_start().starts_with("skill-isolation") && l.contains("emulated")),
            "aikit skill-isolation row missing/wrong:\n{s}"
        );
    }

    #[test]
    fn envelope_carries_skill_isolation_and_activates() {
        let opts = RunOptions::default().with_skill_isolation(test_isolation());
        let env = InvocationEnvelope::from_options(&opts);
        assert_eq!(env.skill_isolation, Some(test_isolation()));
        assert!(
            env.is_active(),
            "skill_isolation alone must activate the envelope, or backends never see it"
        );
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
    fn lifecycle_knobs_matrix() {
        // bare: codex + claude + pi
        assert_eq!(
            Backend::Codex.bare_support(),
            KnobSupport::SupportedOsEnforced
        );
        assert_eq!(
            Backend::Claude.bare_support(),
            KnobSupport::SupportedOsEnforced
        );
        assert_eq!(Backend::Pi.bare_support(), KnobSupport::SupportedOsEnforced);
        // ephemeral: codex + pi
        assert_eq!(
            Backend::Codex.ephemeral_support(),
            KnobSupport::SupportedOsEnforced
        );
        assert_eq!(
            Backend::Pi.ephemeral_support(),
            KnobSupport::SupportedOsEnforced
        );
        // skip-git-repo-check: codex only
        assert_eq!(
            Backend::Codex.skip_git_repo_check_support(),
            KnobSupport::SupportedOsEnforced
        );
        for &b in ALL {
            // skip-git-repo-check is codex-only
            if !matches!(b, Backend::Codex) {
                assert_eq!(
                    b.skip_git_repo_check_support(),
                    KnobSupport::Unsupported,
                    "skip-git-repo-check for {b:?}"
                );
            }
            // ephemeral is unsupported except codex + pi
            if !matches!(b, Backend::Codex | Backend::Pi) {
                assert_eq!(
                    b.ephemeral_support(),
                    KnobSupport::Unsupported,
                    "ephemeral for {b:?}"
                );
            }
            // bare is unsupported except codex + claude + pi
            if !matches!(b, Backend::Codex | Backend::Claude | Backend::Pi) {
                assert_eq!(b.bare_support(), KnobSupport::Unsupported, "bare for {b:?}");
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
    fn resolve_pi_with_sandbox_fails_closed() {
        // Pi has no native sandbox (Unsupported); any --sandbox policy fails
        // closed rather than silently downgrading trust.
        let env = InvocationEnvelope {
            sandbox: Some(SandboxPolicy::BoundedWrite),
            ..Default::default()
        };
        let err = resolve_envelope(Backend::Pi, &env).unwrap_err();
        assert_eq!(err.backend, Backend::Pi);
        assert_eq!(err.knob, "sandbox");
    }

    #[test]
    fn resolve_pi_extra_roots_fails_closed() {
        let env = InvocationEnvelope {
            extra_writable_roots: vec![PathBuf::from("/tmp/x")],
            ..Default::default()
        };
        let err = resolve_envelope(Backend::Pi, &env).unwrap_err();
        assert_eq!(err.backend, Backend::Pi);
        assert_eq!(err.knob, "add-dir");
    }

    #[test]
    fn format_capabilities_pi_shows_native_lifecycle_unsupported_sandbox() {
        let s = format_capabilities(Backend::Pi);
        assert!(s.contains("backend: pi"), "{}", s);
        // sandbox is unsupported for every policy
        assert!(s.contains("unsupported"), "{}", s);
        // bare + ephemeral are now natively supported
        assert!(
            s.lines()
                .any(|l| l.trim_start().starts_with("bare") && l.contains("supported")),
            "bare line must be supported:\n{s}"
        );
        assert!(
            s.lines()
                .any(|l| l.trim_start().starts_with("ephemeral") && l.contains("supported")),
            "ephemeral line must be supported:\n{s}"
        );
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

    // ── gemini sandbox env (follow-up 2) ───────────────────────────────────────

    #[test]
    fn sandbox_env_only_gemini_emits() {
        let env = InvocationEnvelope {
            sandbox: Some(SandboxPolicy::ReadOnly),
            ..Default::default()
        };
        assert!(sandbox_env_for(Backend::Codex, &env).is_empty());
        assert!(sandbox_env_for(Backend::Claude, &env).is_empty());
        assert!(!sandbox_env_for(Backend::Gemini, &env).is_empty());
    }

    #[test]
    fn gemini_sandbox_env_read_only_sets_strict_profile() {
        let env = InvocationEnvelope {
            sandbox: Some(SandboxPolicy::ReadOnly),
            ..Default::default()
        };
        assert_eq!(
            sandbox_env_for(Backend::Gemini, &env),
            vec![("SEATBELT_PROFILE".to_string(), "strict-open".to_string())]
        );
    }

    #[test]
    fn gemini_sandbox_env_bounded_write_no_profile_override() {
        // bounded-write uses gemini's default profile (writes confined to the
        // workspace); unrestricted leaves the sandbox off.
        let bw = InvocationEnvelope {
            sandbox: Some(SandboxPolicy::BoundedWrite),
            ..Default::default()
        };
        assert!(sandbox_env_for(Backend::Gemini, &bw).is_empty());
        let un = InvocationEnvelope {
            sandbox: Some(SandboxPolicy::Unrestricted),
            ..Default::default()
        };
        assert!(sandbox_env_for(Backend::Gemini, &un).is_empty());
    }

    #[test]
    fn gemini_sandbox_env_mounts_extra_roots() {
        let env = InvocationEnvelope {
            extra_writable_roots: vec![PathBuf::from("/a"), PathBuf::from("/b")],
            ..Default::default()
        };
        let e = sandbox_env_for(Backend::Gemini, &env);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].0, "SANDBOX_MOUNTS");
        assert!(e[0].1.contains("/a:/a:rw"), "{}", e[0].1);
        assert!(e[0].1.contains("/b:/b:rw"), "{}", e[0].1);
    }
}
