//! `aikit check` command implementation
//!
//! This module implements the tool checking command.

use crate::core::agent::get_agent_configs;
use crate::core::tools::check_agent_tool;
use crate::tui::output::{format_tree, TreeItem};
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

    // Check all agents
    for agent in get_agent_configs() {
        let result = check_agent_tool(&agent);
        let status = if result.is_ide_based {
            "IDE-based, no CLI check".to_string()
        } else if result.available {
            format!("✓ Found at {}", result.path.as_ref().unwrap().display())
        } else {
            "✗ Not found".to_string()
        };
        items.push(TreeItem::new(format!("{}: {}", agent.name, status)));
    }

    // Display results
    let tree = format_tree(&items);
    println!("{}", tree);

    Ok(())
}
