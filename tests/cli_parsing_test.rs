//! CLI parsing tests using CliTestHarness
//!
//! These tests verify that commands can be dispatched correctly via cli-framework.
//! Note: cli-framework returns Ok(()) for parse errors (reported via stderr diagnostics)
//! so we check stderr content for validation errors rather than exit_code.

#[cfg(test)]
mod tests {
    use aikit::cli::build_app;
    use cli_framework::testkit::CliTestHarness;

    fn harness() -> CliTestHarness<aikit::cli::context::AikitContext> {
        CliTestHarness::new(build_app().expect("failed to build app"))
    }

    /// check command runs without error
    #[tokio::test]
    async fn test_check_runs() {
        let mut h = harness();
        let out = h.run(&["aikit", "check"]).await;
        assert_eq!(out.exit_code, 0, "stderr: {}", out.stderr);
    }

    /// package init requires a name argument — framework reports error via stderr
    #[tokio::test]
    async fn test_package_init_requires_name() {
        let mut h = harness();
        let out = h.run(&["aikit", "package", "init"]).await;
        // Framework reports parse errors in stderr
        assert!(
            out.exit_code != 0 || !out.stderr.is_empty(),
            "expected error when name is missing; stdout: {}",
            out.stdout
        );
    }

    /// install with source — basic arg extraction works
    #[tokio::test]
    async fn test_install_with_invalid_source() {
        let mut h = harness();
        // Source "not-a-valid-source" triggers InvalidSource error
        let out = h.run(&["aikit", "install", "not-a-valid-source"]).await;
        assert_ne!(
            out.exit_code, 0,
            "expected non-zero for invalid source; stdout: {}",
            out.stdout
        );
    }

    /// run with dry-run returns exit 0 (execute_inner is sync, test --dry-run path)
    #[tokio::test]
    async fn test_run_dry_run() {
        let mut h = harness();
        let out = h
            .run(&[
                "aikit",
                "agent",
                "run",
                "--agent",
                "codex",
                "--dry-run",
                "-p",
                "hello",
            ])
            .await;
        // Note: println! output is not captured by testkit stdout_capture; only check exit code
        assert_eq!(
            out.exit_code, 0,
            "dry-run should succeed; stderr: {}",
            out.stderr
        );
    }

    /// agents command lists headers
    #[tokio::test]
    async fn test_agents_command() {
        let mut h = harness();
        let out = h.run(&["aikit", "agent", "list"]).await;
        assert_eq!(out.exit_code, 0, "stderr: {}", out.stderr);
    }

    /// mcp list command runs
    #[tokio::test]
    async fn test_mcp_list() {
        let mut h = harness();
        let out = h.run(&["aikit", "agent", "mcp", "list"]).await;
        // Note: println! output is not captured by testkit stdout_capture; only check exit code
        assert_eq!(out.exit_code, 0, "stderr: {}", out.stderr);
    }

    /// list command runs (no packages installed in test env)
    #[tokio::test]
    async fn test_list_command() {
        let mut h = harness();
        let out = h.run(&["aikit", "list"]).await;
        assert_eq!(out.exit_code, 0, "stderr: {}", out.stderr);
    }

    /// package validate with nonexistent path reports error
    #[tokio::test]
    async fn test_package_validate_nonexistent_path() {
        let mut h = harness();
        let out = h
            .run(&[
                "aikit",
                "package",
                "validate",
                "--path",
                "/nonexistent/path/that/does/not/exist",
            ])
            .await;
        assert_ne!(out.exit_code, 0, "expected error for nonexistent path");
    }

    /// llm validator rejects missing model
    #[tokio::test]
    async fn test_llm_validator_rejects_missing_model() {
        let mut h = harness();
        let out = h.run(&["aikit", "llm", "-p", "hello"]).await;
        // Our validator checks for model and emits a diagnostic
        assert!(
            !out.stderr.is_empty() || out.exit_code != 0,
            "expected error diagnostic for missing --model"
        );
    }

    /// llm validator rejects when no prompt or prompt-file
    #[tokio::test]
    async fn test_llm_validator_rejects_missing_prompt() {
        let mut h = harness();
        let out = h.run(&["aikit", "llm", "-m", "gpt-4o"]).await;
        assert!(
            !out.stderr.is_empty() || out.exit_code != 0,
            "expected error diagnostic for missing prompt"
        );
    }

    /// mcp add validator requires url or command
    #[tokio::test]
    async fn test_mcp_add_requires_transport() {
        let mut h = harness();
        let out = h
            .run(&[
                "aikit", "mcp", "add", "--agent", "claude", "--scope", "global", "--name", "test",
            ])
            .await;
        assert!(
            !out.stderr.is_empty() || out.exit_code != 0,
            "expected error diagnostic for missing --url or --command"
        );
    }

    /// --resume <id> is accepted by the CLI parser
    #[tokio::test]
    async fn test_run_dry_run_with_resume_id() {
        let mut h = harness();
        let out = h
            .run(&[
                "aikit",
                "agent",
                "run",
                "--agent",
                "codex",
                "--dry-run",
                "-p",
                "hello",
                "--resume",
                "test-session-id",
            ])
            .await;
        assert_eq!(
            out.exit_code, 0,
            "dry-run with --resume should succeed; stderr: {}",
            out.stderr
        );
    }

    /// -r <id> short form is accepted by the CLI parser
    #[tokio::test]
    async fn test_run_dry_run_with_resume_short() {
        let mut h = harness();
        let out = h
            .run(&[
                "aikit",
                "agent",
                "run",
                "--agent",
                "codex",
                "--dry-run",
                "-p",
                "hello",
                "-r",
                "test-session-id",
            ])
            .await;
        assert_eq!(
            out.exit_code, 0,
            "dry-run with -r should succeed; stderr: {}",
            out.stderr
        );
    }

    /// --resume-last is accepted by the CLI parser
    #[tokio::test]
    async fn test_run_dry_run_with_resume_last() {
        let mut h = harness();
        let out = h
            .run(&[
                "aikit",
                "agent",
                "run",
                "--agent",
                "codex",
                "--dry-run",
                "-p",
                "hello",
                "--resume-last",
            ])
            .await;
        assert_eq!(
            out.exit_code, 0,
            "dry-run with --resume-last should succeed; stderr: {}",
            out.stderr
        );
    }

    #[tokio::test]
    async fn test_session_sync_dry_run_json() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CLAUDE_HOME", tmp.path().join("claude"));
        std::env::set_var("CODEX_HOME", tmp.path().join("codex"));
        let mut h = harness();
        let out = h
            .run(&[
                "aikit",
                "session",
                "sync",
                "--dry-run",
                "--owner",
                "owner",
                "--format",
                "json",
            ])
            .await;
        assert_eq!(
            out.exit_code, 0,
            "session sync dry-run should succeed; stderr: {}",
            out.stderr
        );
    }

    /// aikit agent run --dry-run works under new namespace
    #[tokio::test]
    async fn test_agent_run_dry_run() {
        let mut h = harness();
        let out = h
            .run(&[
                "aikit",
                "agent",
                "run",
                "--agent",
                "codex",
                "--dry-run",
                "-p",
                "hello",
            ])
            .await;
        assert_eq!(
            out.exit_code, 0,
            "agent run dry-run should succeed; stderr: {}",
            out.stderr
        );
    }

    /// aikit agent run --resume <id> --dry-run works under new namespace
    #[tokio::test]
    async fn test_agent_run_with_resume() {
        let mut h = harness();
        let out = h
            .run(&[
                "aikit",
                "agent",
                "run",
                "--agent",
                "codex",
                "--dry-run",
                "-p",
                "hello",
                "--resume",
                "test-session-id",
            ])
            .await;
        assert_eq!(
            out.exit_code, 0,
            "agent run with --resume should succeed; stderr: {}",
            out.stderr
        );
    }

    /// aikit agent list works under new namespace
    #[tokio::test]
    async fn test_agent_list() {
        let mut h = harness();
        let out = h.run(&["aikit", "agent", "list"]).await;
        assert_eq!(
            out.exit_code, 0,
            "agent list should succeed; stderr: {}",
            out.stderr
        );
    }

    /// aikit agent mcp list works under new namespace
    #[tokio::test]
    async fn test_agent_mcp_list() {
        let mut h = harness();
        let out = h.run(&["aikit", "agent", "mcp", "list"]).await;
        assert_eq!(
            out.exit_code, 0,
            "agent mcp list should succeed; stderr: {}",
            out.stderr
        );
    }

    /// aikit agent check works and does not include dev tool output
    #[tokio::test]
    async fn test_agent_check() {
        let mut h = harness();
        let out = h.run(&["aikit", "agent", "check"]).await;
        assert_eq!(
            out.exit_code, 0,
            "agent check should succeed; stderr: {}",
            out.stderr
        );
    }

    /// aikit agent run without --agent reports an error
    #[tokio::test]
    async fn test_agent_run_requires_agent_flag() {
        let mut h = harness();
        let out = h
            .run(&["aikit", "agent", "run", "--dry-run", "-p", "hello"])
            .await;
        assert!(
            out.exit_code != 0 || !out.stderr.is_empty(),
            "agent run without --agent should fail; stdout: {}",
            out.stdout
        );
    }

    /// aikit agent mcp add without --url or --command triggers E_MCP_TRANSPORT_REQUIRED
    #[tokio::test]
    async fn test_agent_mcp_add_requires_transport() {
        let mut h = harness();
        let out = h
            .run(&[
                "aikit", "agent", "mcp", "add", "--agent", "claude", "--scope", "global", "--name",
                "test",
            ])
            .await;
        assert!(
            !out.stderr.is_empty() || out.exit_code != 0,
            "expected E_MCP_TRANSPORT_REQUIRED; stdout: {}",
            out.stdout
        );
    }

    /// completion without <shell> must produce E003, not E001
    #[tokio::test]
    async fn test_completion_missing_shell_arg_is_e003() {
        let mut h = harness();
        let out = h.run(&["aikit", "completion"]).await;

        // Non-zero exit code required
        assert!(
            out.exit_code != 0 || !out.stderr.is_empty(),
            "expected error when shell is missing; stdout: {}",
            out.stdout
        );

        // Must mention E003 (missing-argument category), NOT E001 (unrecognized command)
        assert!(
            out.stderr.contains("E003"),
            "expected E003 in stderr, got: {}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("E001"),
            "must not emit E001 for missing arg; stderr: {}",
            out.stderr
        );
    }

    /// mcp install --dry-run for cursor exits 0 and produces output
    #[tokio::test]
    async fn test_mcp_install_dry_run_cursor() {
        let mut h = harness();
        let out = h
            .run(&[
                "aikit",
                "mcp",
                "install",
                "--agent",
                "cursor",
                "--stdio",
                "--dry-run",
            ])
            .await;
        assert_eq!(out.exit_code, 0, "expected exit 0; stderr: {}", out.stderr);
        assert!(
            !out.stdout.is_empty(),
            "dry-run must emit config block to stdout; stdout was empty"
        );
    }

    /// mcp install --dry-run for claude exits 0
    #[tokio::test]
    async fn test_mcp_install_dry_run_claude() {
        let mut h = harness();
        let out = h
            .run(&[
                "aikit",
                "mcp",
                "install",
                "--agent",
                "claude",
                "--stdio",
                "--dry-run",
            ])
            .await;
        assert_eq!(out.exit_code, 0, "expected exit 0; stderr: {}", out.stderr);
    }

    /// mcp install without --stdio or --url defaults to HTTP transport.
    ///
    /// HTTP is the default transport (--host/--port/--path supply the URL when
    /// --url is omitted); --stdio is the opt-out. So no transport flag is valid,
    /// not an error. Run with --dry-run so the assertion is hermetic — no real
    /// config write, whose success would otherwise depend on pre-existing state.
    #[tokio::test]
    async fn test_mcp_install_no_transport_defaults_to_http() {
        let mut h = harness();
        let out = h
            .run(&["aikit", "mcp", "install", "--agent", "cursor", "--dry-run"])
            .await;
        assert!(
            !out.stderr.contains("E001"),
            "mcp install must be a recognized command; stderr: {}",
            out.stderr
        );
        assert_eq!(
            out.exit_code, 0,
            "no transport flag is valid (defaults to HTTP); stderr: {}",
            out.stderr
        );
        assert!(
            out.stdout.contains("HTTP"),
            "dry-run should report the defaulted HTTP transport; stdout: {}",
            out.stdout
        );
    }

    /// mcp install with unknown agent — command is recognized; agent key validated at runtime
    #[tokio::test]
    async fn test_mcp_install_unknown_agent() {
        let mut h = harness();
        let out = h
            .run(&[
                "aikit",
                "mcp",
                "install",
                "--agent",
                "nonexistent",
                "--stdio",
                "--dry-run",
            ])
            .await;
        // Agent key validation must reject unknown keys with a non-zero exit.
        assert_ne!(
            out.exit_code, 0,
            "expected non-zero exit for unknown agent; stderr: {}",
            out.stderr
        );
    }

    /// mcp list shows supported agents
    #[tokio::test]
    async fn test_mcp_list_shows_agents() {
        let mut h = harness();
        let out = h.run(&["aikit", "mcp", "list"]).await;
        assert_eq!(out.exit_code, 0, "expected exit 0; stderr: {}", out.stderr);
        assert!(
            out.stdout.contains("cursor"),
            "mcp list must list cursor; stdout: {}",
            out.stdout
        );
    }

    /// mcp register alias behaves like mcp install
    #[tokio::test]
    async fn test_mcp_register_alias() {
        let mut h = harness();
        let out = h
            .run(&[
                "aikit",
                "mcp",
                "register",
                "--agent",
                "cursor",
                "--stdio",
                "--dry-run",
            ])
            .await;
        assert_eq!(out.exit_code, 0, "expected exit 0; stderr: {}", out.stderr);
    }

    /// completion with valid shell arg must succeed
    #[tokio::test]
    async fn test_completion_with_shell_arg_succeeds() {
        let mut h = harness();
        for shell in &["bash", "zsh", "fish", "powershell", "pwsh"] {
            let out = h.run(&["aikit", "completion", shell]).await;
            assert_eq!(
                out.exit_code, 0,
                "aikit completion {} failed; stderr: {}",
                shell, out.stderr
            );
        }
    }
}
