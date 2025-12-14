//! Tool detection and checking
//!
//! This module handles detection of installed tools and AI agent CLIs.

use crate::core::agent::AgentConfig;
use std::path::PathBuf;

/// Result of a tool availability check
#[derive(Debug, Clone)]
pub struct ToolCheckResult {
    /// Tool name
    pub tool_name: String,
    /// Whether tool is available
    pub available: bool,
    /// Path where tool was found
    pub path: Option<PathBuf>,
    /// Whether tool is IDE-based (skip check)
    pub is_ide_based: bool,
    /// Status message for display
    pub message: String,
}

/// Check if a tool is available on PATH
pub fn check_tool_on_path(tool_name: &str) -> Option<PathBuf> {
    which::which(tool_name).ok()
}

/// Check for Claude CLI at special location
///
/// Claude CLI may be installed at ~/.claude/local/claude even if not on PATH
pub fn check_claude_local() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let claude_path = PathBuf::from(home).join(".claude/local/claude");
    if claude_path.exists() {
        Some(claude_path)
    } else {
        None
    }
}

/// Check if a tool is available (general check)
pub fn is_tool_available(tool_name: &str) -> bool {
    check_tool_on_path(tool_name).is_some()
}

/// Check agent tool availability
pub fn check_agent_tool(agent: &AgentConfig) -> ToolCheckResult {
    if !agent.requires_cli {
        return ToolCheckResult {
            tool_name: agent.key.clone(),
            available: false,
            path: None,
            is_ide_based: true,
            message: "IDE-based, no CLI check".to_string(),
        };
    }

    // Special case for Claude
    let path = if agent.key == "claude" {
        check_claude_local().or_else(|| check_tool_on_path(&agent.key))
    } else {
        check_tool_on_path(&agent.key)
    };

    let available = path.is_some();
    let message = if available {
        format!("Found at {}", path.as_ref().unwrap().display())
    } else {
        "Not found on PATH".to_string()
    };

    ToolCheckResult {
        tool_name: agent.key.clone(),
        available,
        path,
        is_ide_based: false,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::agent::AgentConfig;

    #[test]
    fn test_check_ide_based_agent() {
        let agent = AgentConfig {
            key: "copilot".to_string(),
            name: "GitHub Copilot".to_string(),
            folder: ".github".to_string(),
            install_url: None,
            requires_cli: false,
            output_format: crate::core::agent::OutputFormat::AgentMd,
            output_dir: ".github/agents".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        };

        let result = check_agent_tool(&agent);
        assert!(result.is_ide_based);
        assert!(!result.available);
    }
}
