//! Runnable-agent listing and argv construction.
//!
//! The per-Backend argv assembly now lives in `backends/<name>.rs`; this module
//! keeps the string-keyed entry points (`runnable_agents`, `is_runnable`,
//! `build_argv`) that the runner and public API use, delegating to
//! [`Backend`](super::backend::Backend).

use std::ffi::OsString;

use super::backend::Backend;
use super::invocation::InvocationEnvelope;

/// The runnable agent keys, in canonical order. Kept in lockstep with
/// [`Backend::ALL`] (enforced by a test below).
pub fn runnable_agents() -> &'static [&'static str] {
    &[
        "codex", "claude", "gemini", "opencode", "cursor", "pi", "aikit",
    ]
}

pub fn is_runnable(agent_key: &str) -> bool {
    Backend::from_key(agent_key).is_some()
}

/// Build the spawn argv carrying a spec-013 [`InvocationEnvelope`]. Each
/// Backend maps honored knobs onto its native flags; the runner resolves the
/// envelope (fail-closed for unsupported security knobs) before calling this.
pub(crate) fn build_argv_envelope(
    agent_key: &str,
    model: Option<&String>,
    yolo: bool,
    stream: bool,
    events_mode: bool,
    session_id: Option<&str>,
    envelope: Option<&InvocationEnvelope>,
) -> Vec<OsString> {
    let backend = Backend::from_key(agent_key).expect("unknown agent key for build_argv");
    backend.build_argv_envelope(model, yolo, stream, events_mode, session_id, envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Legacy no-envelope builder, retained as a test helper so the historical
    // argv assertions run unchanged against the envelope path (envelope = None
    // ⇒ identical argv). Production goes through `build_argv_envelope`.
    #[allow(dead_code)]
    fn build_argv(
        agent_key: &str,
        model: Option<&String>,
        yolo: bool,
        stream: bool,
        events_mode: bool,
        session_id: Option<&str>,
    ) -> Vec<OsString> {
        build_argv_envelope(
            agent_key,
            model,
            yolo,
            stream,
            events_mode,
            session_id,
            None,
        )
    }

    #[test]
    fn test_is_runnable_true_for_supported_false_for_others() {
        assert!(is_runnable("codex"));
        assert!(is_runnable("claude"));
        assert!(is_runnable("gemini"));
        assert!(is_runnable("opencode"));
        assert!(is_runnable("cursor"));
        assert!(is_runnable("pi"));
        assert!(is_runnable("aikit"));
        assert!(!is_runnable("agent")); // renamed to "cursor" (ADR 0006)
        assert!(!is_runnable("copilot"));
        assert!(!is_runnable("cursor-agent"));
        assert!(!is_runnable("unknown"));
    }

    #[test]
    fn test_is_runnable_case_sensitive() {
        assert!(is_runnable("codex"));
        assert!(!is_runnable("Codex"));
        assert!(!is_runnable("CODEX"));
    }

    #[test]
    fn test_runnable_agents_matches_backend_all() {
        let agents = runnable_agents();
        assert!(agents.contains(&"codex"));
        assert!(agents.contains(&"claude"));
        assert!(agents.contains(&"gemini"));
        assert!(agents.contains(&"opencode"));
        assert!(agents.contains(&"cursor"));
        assert!(agents.contains(&"pi"));
        assert!(agents.contains(&"aikit"));
        assert_eq!(agents.len(), 7);
        // Single source of truth: same set as Backend::ALL.
        let mut from_list: Vec<&str> = agents.to_vec();
        from_list.sort_unstable();
        let mut from_enum: Vec<&str> = crate::runner::backend::ALL
            .iter()
            .map(|b| b.key())
            .collect();
        from_enum.sort_unstable();
        assert_eq!(from_list, from_enum);
    }

    // --- codex ---

    #[test]
    fn test_codex_plain_contains_exec_and_model() {
        let argv = build_argv(
            "codex",
            Some(&"gpt-4".to_string()),
            true,
            false,
            false,
            None,
        );
        assert!(argv.contains(&OsString::from("codex")));
        assert!(argv.contains(&OsString::from("exec")));
        assert!(argv.contains(&OsString::from("-m")));
        assert!(argv.contains(&OsString::from("gpt-4")));
        assert!(argv.contains(&OsString::from("--yolo")));
        assert!(argv.contains(&OsString::from("--json")));
    }

    #[test]
    fn test_codex_plain_no_model() {
        let argv = build_argv("codex", None, false, false, false, None);
        assert!(!argv.contains(&OsString::from("-m")));
        assert!(!argv.contains(&OsString::from("--yolo")));
        assert!(argv.contains(&OsString::from("--json")));
    }

    #[test]
    fn test_empty_model_is_not_passed() {
        // An empty model string must NOT produce `-m ""` (engines reject it).
        let empty = "".to_string();
        let argv = build_argv("codex", Some(&empty), true, false, false, None);
        assert!(
            !argv.contains(&OsString::from("-m")),
            "empty model must be skipped; got {:?}",
            argv
        );
        let ws = "   ".to_string();
        let argv2 = build_argv("opencode", Some(&ws), true, false, true, None);
        assert!(
            !argv2.contains(&OsString::from("-m")),
            "whitespace model must be skipped; got {:?}",
            argv2
        );
    }

    #[test]
    fn test_opencode_events_mode_passes_dangerously_skip_permissions() {
        let argv = build_argv("opencode", None, true, false, true, None);
        assert!(
            argv.contains(&OsString::from("--dangerously-skip-permissions")),
            "opencode events mode must include --dangerously-skip-permissions; got {:?}",
            argv
        );
        assert!(
            !argv.contains(&OsString::from("--yolo")),
            "opencode does not support --yolo; got {:?}",
            argv
        );
        assert!(argv.contains(&OsString::from("--format")));
    }

    #[test]
    fn test_codex_plain_with_session_id() {
        let argv = build_argv("codex", None, false, false, false, Some("test-session-id"));
        assert!(argv.contains(&OsString::from("codex")));
        assert!(argv.contains(&OsString::from("resume")));
        assert!(argv.contains(&OsString::from("test-session-id")));
        assert!(!argv.contains(&OsString::from("exec")));
        let resume_pos = argv.iter().position(|a| a == "resume").unwrap();
        let id_pos = argv.iter().position(|a| a == "test-session-id").unwrap();
        assert_eq!(id_pos, resume_pos + 1);
    }

    #[test]
    fn test_codex_events_has_json_flag() {
        let argv = build_argv("codex", Some(&"gpt-4".to_string()), true, false, true, None);
        assert!(argv.contains(&OsString::from("--json")));
        assert!(argv.contains(&OsString::from("--yolo")));
        assert!(argv.contains(&OsString::from("-m")));
    }

    #[test]
    fn test_codex_events_with_session_id() {
        let argv = build_argv("codex", None, false, false, true, Some("test-id-123"));
        let argv_str: Vec<&str> = argv.iter().map(|s| s.to_str().unwrap()).collect();
        assert_eq!(argv_str[0], "codex");
        assert_eq!(argv_str[1], "resume");
        assert_eq!(argv_str[2], "test-id-123");
        assert!(argv.contains(&OsString::from("--json")));
        assert!(!argv.contains(&OsString::from("exec")));
    }

    // --- claude ---

    #[test]
    fn test_claude_plain_contains_prompt_and_model() {
        let argv = build_argv(
            "claude",
            Some(&"claude-3-opus".to_string()),
            false,
            true,
            false,
            None,
        );
        assert!(argv.contains(&OsString::from("claude")));
        assert!(argv.contains(&OsString::from("-p")));
        assert!(argv.contains(&OsString::from("-")));
        assert!(!argv.contains(&OsString::from("test prompt")));
        assert!(argv.contains(&OsString::from("--model")));
        assert!(argv.contains(&OsString::from("claude-3-opus")));
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("stream-json")));
    }

    #[test]
    fn test_claude_plain_text_format() {
        let argv = build_argv("claude", None, false, false, false, None);
        assert!(argv.contains(&OsString::from("text")));
        assert!(!argv.contains(&OsString::from("stream-json")));
    }

    #[test]
    fn test_claude_plain_with_session_id() {
        let argv = build_argv("claude", None, false, false, false, Some("test-session-id"));
        assert!(argv.contains(&OsString::from("--resume")));
        assert!(argv.contains(&OsString::from("test-session-id")));
    }

    #[test]
    fn test_claude_events_json_format() {
        let argv = build_argv("claude", None, false, false, true, None);
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("json")));
    }

    #[test]
    fn test_claude_events_stream_json_format() {
        let argv = build_argv("claude", None, false, true, true, None);
        assert!(argv.contains(&OsString::from("stream-json")));
        assert!(
            argv.contains(&OsString::from("--verbose")),
            "stream-json must include --verbose; got argv: {:?}",
            argv
        );
    }

    #[test]
    fn test_claude_events_json_format_no_verbose() {
        let argv = build_argv("claude", None, false, false, true, None);
        assert!(!argv.contains(&OsString::from("--verbose")));
    }

    #[test]
    fn test_claude_events_with_session_id() {
        let argv = build_argv("claude", None, false, false, true, Some("test-id-456"));
        assert!(argv.contains(&OsString::from("--resume")));
        assert!(argv.contains(&OsString::from("test-id-456")));
        let resume_pos = argv.iter().position(|a| a == "--resume").unwrap();
        assert_eq!(argv[resume_pos + 1], OsString::from("test-id-456"));
    }

    // --- gemini ---

    #[test]
    fn test_gemini_plain_contains_prompt_and_model() {
        let argv = build_argv(
            "gemini",
            Some(&"gemini-pro".to_string()),
            false,
            false,
            false,
            None,
        );
        assert!(argv.contains(&OsString::from("gemini")));
        assert!(argv.contains(&OsString::from("--prompt")));
        assert!(argv.contains(&OsString::from("-")));
        assert!(!argv.contains(&OsString::from("test prompt")));
        assert!(argv.contains(&OsString::from("--model")));
        assert!(argv.contains(&OsString::from("gemini-pro")));
    }

    #[test]
    fn test_gemini_plain_with_session_id() {
        let argv = build_argv("gemini", None, false, false, false, Some("test-session-id"));
        assert!(argv.contains(&OsString::from("--resume")));
        assert!(argv.contains(&OsString::from("test-session-id")));
    }

    #[test]
    fn test_gemini_events_stream_json_headless() {
        let argv = build_argv("gemini", None, false, false, true, None);
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("stream-json")));
        assert!(argv.contains(&OsString::from("--yolo")));
        assert!(!argv.contains(&OsString::from("--json")));
    }

    #[test]
    fn test_gemini_events_with_session_id() {
        let argv = build_argv("gemini", None, false, false, true, Some("test-id-789"));
        assert!(argv.contains(&OsString::from("--resume")));
        assert!(argv.contains(&OsString::from("test-id-789")));
        let resume_pos = argv.iter().position(|a| a == "--resume").unwrap();
        assert_eq!(argv[resume_pos + 1], OsString::from("test-id-789"));
    }

    // --- opencode ---

    #[test]
    fn test_opencode_plain_contains_prompt_and_model() {
        let argv = build_argv(
            "opencode",
            Some(&"zai-coding-plan/glm-4.7".to_string()),
            true,
            false,
            false,
            None,
        );
        assert!(argv.contains(&OsString::from("opencode")));
        assert!(argv.contains(&OsString::from("run")));
        assert!(!argv.contains(&OsString::from("test prompt")));
        assert!(argv.contains(&OsString::from("-")));
        assert!(argv.contains(&OsString::from("-m")));
        assert!(argv.contains(&OsString::from("zai-coding-plan/glm-4.7")));
        assert!(argv.contains(&OsString::from("--dangerously-skip-permissions")));
        let run_pos = argv.iter().position(|a| a == "run").unwrap();
        let dash_pos = argv.iter().position(|a| a == "-").unwrap();
        assert!(
            run_pos < dash_pos,
            "stdin marker must follow run subcommand"
        );
    }

    #[test]
    fn test_opencode_plain_no_options() {
        let argv = build_argv("opencode", None, false, false, false, None);
        assert!(!argv.contains(&OsString::from("--dangerously-skip-permissions")));
    }

    #[test]
    fn test_opencode_plain_with_session_id() {
        let argv = build_argv(
            "opencode",
            None,
            false,
            false,
            false,
            Some("test-session-id"),
        );
        assert!(argv.contains(&OsString::from("--session")));
        assert!(argv.contains(&OsString::from("test-session-id")));
    }

    #[test]
    fn test_opencode_events_uses_run_subcommand() {
        let argv = build_argv("opencode", None, false, false, true, None);
        assert!(argv.contains(&OsString::from("opencode")));
        assert!(argv.contains(&OsString::from("run")));
        assert!(!argv.contains(&OsString::from("test prompt")));
        assert_eq!(*argv.last().unwrap(), OsString::from("-"));
        assert!(argv.contains(&OsString::from("--format")));
        assert!(argv.contains(&OsString::from("json")));
        assert!(!argv.contains(&OsString::from("--json")));
        assert!(!argv.contains(&OsString::from("--prompt")));
    }

    #[test]
    fn test_opencode_events_with_model() {
        let model = "zai-coding-plan/glm-4.7".to_string();
        let argv = build_argv("opencode", Some(&model), false, false, true, None);
        assert!(argv.contains(&OsString::from("-m")));
        assert!(argv.contains(&OsString::from("zai-coding-plan/glm-4.7")));
        let m_pos = argv.iter().position(|a| a == "-m").unwrap();
        let run_pos = argv.iter().position(|a| a == "run").unwrap();
        assert!(m_pos < run_pos);
    }

    #[test]
    fn test_opencode_events_with_session_id() {
        let argv = build_argv("opencode", None, false, false, true, Some("test-id-abc"));
        assert!(argv.contains(&OsString::from("--session")));
        assert!(argv.contains(&OsString::from("test-id-abc")));
        let session_pos = argv.iter().position(|a| a == "--session").unwrap();
        assert_eq!(argv[session_pos + 1], OsString::from("test-id-abc"));
    }

    // --- cursor (was "agent") ---

    #[test]
    fn test_cursor_plain_contains_all_options() {
        let argv = build_argv(
            "cursor",
            Some(&"custom-model".to_string()),
            true,
            true,
            false,
            None,
        );
        assert!(argv.contains(&OsString::from("agent"))); // spawn binary stays `agent`
        assert!(argv.contains(&OsString::from("--print")));
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("json")));
        assert!(argv.contains(&OsString::from("--model")));
        assert!(argv.contains(&OsString::from("custom-model")));
        assert!(argv.contains(&OsString::from("--force")));
        assert!(!argv.contains(&OsString::from("test prompt")));
        assert!(!argv.contains(&OsString::from("--prompt")));
        assert!(!argv.contains(&OsString::from("--yolo")));
    }

    #[test]
    fn test_cursor_plain_with_session_id() {
        let argv = build_argv("cursor", None, false, false, false, Some("test-session-id"));
        assert!(argv.contains(&OsString::from("--resume")));
        assert!(argv.contains(&OsString::from("test-session-id")));
    }

    #[test]
    fn test_cursor_events_has_json_flag() {
        let argv = build_argv("cursor", None, false, false, true, None);
        assert!(argv.contains(&OsString::from("--print")));
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("json")));
        assert!(!argv.contains(&OsString::from("--json")));
    }

    #[test]
    fn test_cursor_events_stream_json() {
        let argv = build_argv("cursor", None, false, true, true, None);
        assert!(argv.contains(&OsString::from("stream-json")));
        assert!(!argv.contains(&OsString::from("test")));
    }

    #[test]
    fn test_cursor_events_with_session_id() {
        let argv = build_argv("cursor", None, false, false, true, Some("test-id-def"));
        assert!(argv.contains(&OsString::from("--resume")));
        assert!(argv.contains(&OsString::from("test-id-def")));
        let resume_pos = argv.iter().position(|a| a == "--resume").unwrap();
        assert_eq!(argv[resume_pos + 1], OsString::from("test-id-def"));
    }

    // ── spec 013: invocation-envelope argv mapping ───────────────────────────

    use crate::runner::{InvocationEnvelope, SandboxPolicy};
    use std::path::PathBuf;

    fn env_with(sandbox: Option<SandboxPolicy>) -> InvocationEnvelope {
        InvocationEnvelope {
            sandbox,
            ..Default::default()
        }
    }

    #[test]
    fn spec013_codex_sandbox_maps_to_native_token_and_suppresses_yolo() {
        let env = env_with(Some(SandboxPolicy::BoundedWrite));
        let argv = build_argv_envelope("codex", None, false, false, false, None, Some(&env));
        assert!(argv.contains(&OsString::from("--sandbox")));
        assert!(argv.contains(&OsString::from("workspace-write")));
        assert!(
            !argv.contains(&OsString::from("--yolo")),
            "explicit sandbox subsumes --yolo; got {:?}",
            argv
        );

        let ro = build_argv_envelope(
            "codex",
            None,
            false,
            false,
            false,
            None,
            Some(&env_with(Some(SandboxPolicy::ReadOnly))),
        );
        assert!(ro.contains(&OsString::from("read-only")));

        let un = build_argv_envelope(
            "codex",
            None,
            false,
            false,
            false,
            None,
            Some(&env_with(Some(SandboxPolicy::Unrestricted))),
        );
        assert!(un.contains(&OsString::from("danger-full-access")));
    }

    #[test]
    fn spec013_codex_inactive_envelope_is_identical_to_legacy() {
        // No knobs set ⇒ identical to the historical argv (yolo honored).
        let env = InvocationEnvelope::default();
        let with_env = build_argv_envelope("codex", None, true, false, false, None, Some(&env));
        let legacy = build_argv("codex", None, true, false, false, None);
        assert_eq!(with_env, legacy);
        assert!(with_env.contains(&OsString::from("--yolo")));
    }

    #[test]
    fn spec013_codex_envelope_emits_d2_d6_native_flags() {
        let env = InvocationEnvelope {
            working_dir: Some(PathBuf::from("/repo")),
            extra_writable_roots: vec![PathBuf::from("/shared"), PathBuf::from("/cache")],
            bare: true,
            ephemeral: true,
            skip_git_repo_check: true,
            ..Default::default()
        };
        let argv = build_argv_envelope("codex", None, false, false, false, None, Some(&env));
        assert!(argv.contains(&OsString::from("--cd")));
        assert!(argv.contains(&OsString::from("/repo")));
        assert!(argv.contains(&OsString::from("--add-dir")));
        assert!(argv.contains(&OsString::from("/shared")));
        assert!(argv.contains(&OsString::from("/cache")));
        assert!(argv.contains(&OsString::from("--ignore-user-config")));
        assert!(argv.contains(&OsString::from("--ephemeral")));
        assert!(argv.contains(&OsString::from("--skip-git-repo-check")));
    }

    #[test]
    fn spec013_claude_bare_emits_native_flag() {
        let env = InvocationEnvelope {
            bare: true,
            ..Default::default()
        };
        let argv = build_argv_envelope("claude", None, false, false, false, None, Some(&env));
        assert!(argv.contains(&OsString::from("--bare")));
        // without bare, the flag is absent
        let plain = build_argv_envelope(
            "claude",
            None,
            false,
            false,
            false,
            None,
            Some(&InvocationEnvelope::default()),
        );
        assert!(!plain.contains(&OsString::from("--bare")));
    }

    #[test]
    fn spec013_gemini_sandbox_toggle() {
        // restrictive policy ⇒ `-s`; unrestricted/none ⇒ no `-s`
        let on = build_argv_envelope(
            "gemini",
            None,
            false,
            false,
            false,
            None,
            Some(&env_with(Some(SandboxPolicy::BoundedWrite))),
        );
        assert!(on.contains(&OsString::from("-s")));

        let ro = build_argv_envelope(
            "gemini",
            None,
            false,
            false,
            false,
            None,
            Some(&env_with(Some(SandboxPolicy::ReadOnly))),
        );
        assert!(ro.contains(&OsString::from("-s")));

        let off = build_argv_envelope(
            "gemini",
            None,
            false,
            false,
            false,
            None,
            Some(&env_with(Some(SandboxPolicy::Unrestricted))),
        );
        assert!(!off.contains(&OsString::from("-s")));

        let none = build_argv_envelope(
            "gemini",
            None,
            false,
            false,
            false,
            None,
            Some(&InvocationEnvelope::default()),
        );
        assert!(!none.contains(&OsString::from("-s")));
    }

    #[test]
    fn spec013_opencode_working_dir_emits_dir_flag() {
        let env = InvocationEnvelope {
            working_dir: Some(PathBuf::from("/repo")),
            ..Default::default()
        };
        let argv = build_argv_envelope("opencode", None, false, false, false, None, Some(&env));
        assert!(argv.contains(&OsString::from("--dir")));
        assert!(argv.contains(&OsString::from("/repo")));
        // run subcommand precedes the stdin marker
        let run_pos = argv.iter().position(|a| a == "run").unwrap();
        let dash_pos = argv.iter().position(|a| a == "-").unwrap();
        assert!(run_pos < dash_pos);
    }

    #[test]
    fn spec013_cursor_envelope_adds_no_native_flags() {
        // cursor has no native spec-013 knobs; an (already-resolved) envelope
        // must not add flags. Security requests fail closed before argv.
        let env = InvocationEnvelope {
            bare: true,
            ephemeral: true,
            ..Default::default()
        };
        let with_env = build_argv_envelope("cursor", None, false, false, false, None, Some(&env));
        let legacy = build_argv("cursor", None, false, false, false, None);
        assert_eq!(with_env, legacy);
    }
}
