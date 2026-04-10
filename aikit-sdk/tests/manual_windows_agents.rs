/// Manual integration tests for Windows agent CLI resolution.
///
/// These tests are marked `#[ignore]` and require a real Windows environment with
/// Cursor agent installed. Run them with:
///
/// ```
/// cargo test --test manual_windows_agents -- --ignored
/// ```
///
/// Or to run a specific test:
///
/// ```
/// cargo test --test manual_windows_agents test_cursor_agent_available -- --ignored
/// ```
#[cfg(windows)]
mod windows_agent_tests {
    /// Tests that the Cursor agent is detected as available on Windows when `agent.cmd` is on PATH.
    ///
    /// Requires: Cursor installed with `agent.cmd` available on PATH.
    #[test]
    #[ignore = "requires Cursor agent installed on Windows PATH"]
    fn test_cursor_agent_available() {
        let statuses = aikit_sdk::get_agent_status();
        let cursor_status = statuses
            .get("cursor")
            .expect("cursor agent key should exist");
        assert!(
            cursor_status.available,
            "Cursor agent should be detected as available when agent.cmd is on PATH. Status: {:?}",
            cursor_status
        );
    }

    /// Tests that AIKIT_CURSOR_AGENT environment variable override is respected
    /// when checking agent availability.
    ///
    /// Requires: AIKIT_CURSOR_AGENT set to a valid path to agent.cmd.
    #[test]
    #[ignore = "requires AIKIT_CURSOR_AGENT env var pointing to real agent.cmd"]
    fn test_cursor_agent_env_override_availability() {
        let _override = std::env::var("AIKIT_CURSOR_AGENT")
            .expect("AIKIT_CURSOR_AGENT must be set to run this test");

        let statuses = aikit_sdk::get_agent_status();
        let cursor_status = statuses
            .get("cursor")
            .expect("cursor agent key should exist");
        assert!(
            cursor_status.available,
            "Cursor agent should be detected as available via AIKIT_CURSOR_AGENT override. Status: {:?}",
            cursor_status
        );
    }
}

// Compile-time placeholder for non-Windows platforms
#[cfg(not(windows))]
#[test]
fn not_applicable_on_unix() {
    // Windows-specific manual tests are not applicable on Unix.
    // This test exists so the file compiles on all platforms.
}
