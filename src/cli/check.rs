//! `aikit check` command implementation
//!
//! This module implements the tool checking command.

use crate::core::agent::get_agent_configs;
use crate::tui::output::{format_tree, TreeItem};
use aikit_sdk::{get_agent_status, AgentAvailabilityReason};
use anyhow::Result;
use clap::Args;

/// Check installed tools and AI agent CLIs
#[derive(Args, Debug)]
pub struct CheckArgs {
    // No arguments for check command
}

/// Execute the check command
pub fn execute(_args: CheckArgs) -> Result<()> {
    let mut items = Vec::new();

    // Check Git
    let git_available = crate::core::tools::is_tool_available("git");
    items.push(TreeItem::new(format!(
        "git: {}",
        if git_available {
            "✓ Found"
        } else {
            "✗ Not found"
        }
    )));

    // Check VS Code
    let code_available = crate::core::tools::is_tool_available("code");
    let code_insiders_available = crate::core::tools::is_tool_available("code-insiders");
    items.push(TreeItem::new(format!(
        "VS Code: {}",
        if code_available || code_insiders_available {
            "✓ Found"
        } else {
            "✗ Not found"
        }
    )));

    // Get runnable agent status (deterministic ordering from BTreeMap)
    let agent_status = get_agent_status();
    let agent_configs = get_agent_configs();

    // Display status for all runnable agents
    for (agent_key, status) in &agent_status {
        let agent_config = agent_configs.iter().find(|a| a.key == *agent_key);

        let agent_name = agent_config.map(|a| a.name.as_str()).unwrap_or(agent_key);

        let status_text = if status.available {
            "✓ Available".to_string()
        } else {
            let reason = status
                .reason
                .as_ref()
                .map(format_reason_for_user)
                .unwrap_or_else(|| "Not available".to_string());
            format!("✗ {}", reason)
        };

        items.push(TreeItem::new(format!("{}: {}", agent_name, status_text)));
    }

    // Display results
    let tree = format_tree(&items);
    println!("{}", tree);

    Ok(())
}

/// Convert AgentAvailabilityReason to user-facing message
fn format_reason_for_user(reason: &AgentAvailabilityReason) -> String {
    match reason {
        AgentAvailabilityReason::BinaryNotFound => "CLI not found in PATH".to_string(),
        AgentAvailabilityReason::VersionCheckFailed => "CLI found but --version failed".to_string(),
        AgentAvailabilityReason::TimedOut => "CLI probe timed out".to_string(),
        AgentAvailabilityReason::NotRunnable => "Not runnable".to_string(),
    }
}
