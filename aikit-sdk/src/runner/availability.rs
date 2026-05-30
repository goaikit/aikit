use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::argv::{is_runnable, runnable_agents};
use super::types::{AgentAvailabilityReason, AgentStatus, ChildTimeoutExt};

/// Timeout for agent availability probing in milliseconds.
pub(super) const PROBE_TIMEOUT_MS: u64 = 1500;

/// Gets the binary candidates for an agent key.
pub(super) fn get_binary_candidates(agent_key: &str) -> &'static [&'static str] {
    match agent_key {
        "codex" => &["codex"],
        "claude" => &["claude"],
        "gemini" => &["gemini"],
        "opencode" => &["opencode", "opencode-desktop"],
        "agent" => &["agent"],
        _ => &[],
    }
}

/// Probes a binary with a --version check under timeout.
///
/// Returns Ok(true) if binary responds successfully to --version,
/// Ok(false) if binary exists but --version fails,
/// Err if binary not found or timeout occurs.
pub(super) fn probe_binary_with_timeout(binary: &str) -> Result<bool, AgentAvailabilityReason> {
    let resolved_binary = crate::command_resolve::resolve_command(binary);
    let mut cmd = Command::new(resolved_binary);
    cmd.arg("--version");
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    let mut child = cmd
        .spawn()
        .map_err(|_| AgentAvailabilityReason::BinaryNotFound)?;

    let timeout = Duration::from_millis(PROBE_TIMEOUT_MS);

    match child.wait_timeout(timeout) {
        Ok(Some(status)) => Ok(status.success()),
        Ok(None) => {
            let _ = child.kill();
            Err(AgentAvailabilityReason::TimedOut)
        }
        Err(_) => Err(AgentAvailabilityReason::BinaryNotFound),
    }
}

/// Checks if an agent is available (installed and responds to --version).
///
/// Returns false for non-runnable agents.
/// For runnable agents, probes each binary candidate and returns true
/// if any responds successfully to --version.
pub fn is_agent_available(agent_key: &str) -> bool {
    if !is_runnable(agent_key) {
        return false;
    }
    if agent_key == "aikit" {
        return true;
    }

    let candidates = get_binary_candidates(agent_key);
    for binary in candidates {
        if probe_binary_with_timeout(binary).unwrap_or(false) {
            return true;
        }
    }

    false
}

/// Gets the list of installed and available runnable agents.
///
/// Returns sorted list of agent keys that are runnable and available.
pub fn get_installed_agents() -> Vec<String> {
    let mut agents: Vec<String> = runnable_agents()
        .iter()
        .filter(|&&key: &&&str| is_agent_available(key))
        .map(|s: &&str| s.to_string())
        .collect();
    agents.sort();
    agents
}

/// Gets the status for all runnable agents.
///
/// Returns BTreeMap for stable ordering. Includes all runnable agents
/// with their availability status and reason if unavailable.
pub fn get_agent_status() -> BTreeMap<String, AgentStatus> {
    let mut status = BTreeMap::new();

    for &agent_key in runnable_agents() {
        if agent_key == "aikit" {
            status.insert(agent_key.to_string(), AgentStatus::available());
            continue;
        }

        if !is_runnable(agent_key) {
            status.insert(
                agent_key.to_string(),
                AgentStatus::unavailable(AgentAvailabilityReason::NotRunnable),
            );
            continue;
        }

        let candidates = get_binary_candidates(agent_key);
        let mut available = false;
        let mut last_error = AgentAvailabilityReason::BinaryNotFound;

        for binary in candidates {
            match probe_binary_with_timeout(binary) {
                Ok(true) => {
                    available = true;
                    break;
                }
                Ok(false) => {
                    last_error = AgentAvailabilityReason::VersionCheckFailed;
                }
                Err(e) => {
                    last_error = e;
                }
            }
        }

        if available {
            status.insert(agent_key.to_string(), AgentStatus::available());
        } else {
            status.insert(agent_key.to_string(), AgentStatus::unavailable(last_error));
        }
    }

    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::argv::runnable_agents;

    #[test]
    fn test_is_agent_available_false_for_non_runnable() {
        assert!(!is_agent_available("copilot"));
        assert!(!is_agent_available("cursor-agent"));
        assert!(!is_agent_available("unknown"));
    }

    #[test]
    fn test_get_agent_status_keys_match_runnable_agents() {
        let status = get_agent_status();
        let runnable_set: std::collections::HashSet<_> =
            runnable_agents().iter().copied().collect();
        let status_keys: std::collections::HashSet<_> = status.keys().map(|s| s.as_str()).collect();
        assert_eq!(runnable_set, status_keys);
    }

    #[test]
    fn test_get_installed_agents_is_subset_of_runnable_agents() {
        let installed = get_installed_agents();
        let runnable_set: std::collections::HashSet<_> =
            runnable_agents().iter().copied().collect();
        for agent in &installed {
            assert!(runnable_set.contains(agent.as_str()));
        }
    }

    #[test]
    fn test_get_installed_agents_sorted() {
        let installed = get_installed_agents();
        let mut sorted_installed = installed.clone();
        sorted_installed.sort();
        assert_eq!(installed, sorted_installed);
    }

    #[test]
    fn test_unavailable_statuses_have_reason() {
        let status = get_agent_status();
        for (agent_key, agent_status) in &status {
            if !agent_status.available {
                assert!(
                    agent_status.reason.is_some(),
                    "Agent {} is unavailable but has no reason",
                    agent_key
                );
            }
        }
    }

    #[test]
    fn test_binary_candidates_mapping() {
        assert_eq!(get_binary_candidates("codex"), &["codex"] as &[&str]);
        assert_eq!(get_binary_candidates("claude"), &["claude"]);
        assert_eq!(get_binary_candidates("gemini"), &["gemini"]);
        assert_eq!(
            get_binary_candidates("opencode"),
            &["opencode", "opencode-desktop"]
        );
        assert_eq!(get_binary_candidates("agent"), &["agent"]);
        assert!(get_binary_candidates("unknown").is_empty());
    }

    #[test]
    fn test_agent_status_available() {
        let status = AgentStatus::available();
        assert!(status.available);
        assert!(status.reason.is_none());
    }

    #[test]
    fn test_agent_status_unavailable() {
        let status = AgentStatus::unavailable(AgentAvailabilityReason::BinaryNotFound);
        assert!(!status.available);
        assert_eq!(status.reason, Some(AgentAvailabilityReason::BinaryNotFound));
    }

    #[test]
    fn test_agent_availability_reason_display() {
        assert_eq!(
            format!("{}", AgentAvailabilityReason::NotRunnable),
            "not_runnable"
        );
        assert_eq!(
            format!("{}", AgentAvailabilityReason::BinaryNotFound),
            "binary_not_found"
        );
        assert_eq!(
            format!("{}", AgentAvailabilityReason::VersionCheckFailed),
            "version_check_failed"
        );
        assert_eq!(
            format!("{}", AgentAvailabilityReason::TimedOut),
            "timed_out"
        );
    }
}
