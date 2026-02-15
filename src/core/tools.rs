//! Tool detection and validation utilities

use crate::core::agent::AgentConfig;
use std::process::Command;

/// Check if a command-line tool is available
pub fn is_tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Check agent tool availability and configuration
pub fn check_agent_tool(agent_config: &AgentConfig) -> Result<(), Box<dyn std::error::Error>> {
    use aikit_sdk::{get_agent_status, is_runnable};

    // Non-runnable agents: no check needed
    if !is_runnable(&agent_config.key) {
        return Ok(());
    }

    // Get status for this agent
    let status_map = get_agent_status();
    let status = status_map
        .get(&agent_config.key)
        .ok_or_else(|| format!("Agent '{}' not found in status map", agent_config.key))?;

    if !status.available {
        if let Some(reason) = &status.reason {
            let error_msg = match reason {
                aikit_sdk::AgentAvailabilityReason::BinaryNotFound => {
                    format!("Agent '{}' CLI not found in PATH", agent_config.name)
                }
                aikit_sdk::AgentAvailabilityReason::VersionCheckFailed => {
                    format!(
                        "Agent '{}' CLI found but --version check failed",
                        agent_config.name
                    )
                }
                aikit_sdk::AgentAvailabilityReason::TimedOut => {
                    format!("Agent '{}' CLI probe timed out", agent_config.name)
                }
                aikit_sdk::AgentAvailabilityReason::NotRunnable => {
                    format!("Agent '{}' is not runnable", agent_config.name)
                }
            };
            return Err(error_msg.into());
        }
    }

    Ok(())
}
