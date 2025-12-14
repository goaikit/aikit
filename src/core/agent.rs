//! Agent configuration and validation module
//!
//! This module contains types and functions for managing AI agent configurations,
//! including agent selection, validation, and tool checking.

/// Script variant (bash or PowerShell)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScriptVariant {
    /// Bash script (.sh)
    Sh,
    /// PowerShell script (.ps1)
    Ps,
}

impl ScriptVariant {
    /// Get the default script variant for the current platform
    pub fn default_for_platform() -> Self {
        if cfg!(windows) {
            Self::Ps
        } else {
            Self::Sh
        }
    }

    /// Get the file extension for this script variant
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Sh => "sh",
            Self::Ps => "ps1",
        }
    }
}

/// Output format for command files
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Markdown format (.md)
    Markdown,
    /// TOML format (.toml)
    Toml,
    /// Agent-specific markdown format (agent.md for Copilot)
    AgentMd,
}

/// Agent configuration
///
/// Represents an AI agent configuration with all metadata needed for
/// initialization and tool checking.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Executable name (e.g., "claude", "gemini")
    pub key: String,
    /// Display name (e.g., "Claude", "Google Gemini")
    pub name: String,
    /// Project directory (e.g., ".claude", ".gemini")
    pub folder: String,
    /// Optional installation URL
    pub install_url: Option<String>,
    /// Whether agent requires CLI tool check
    pub requires_cli: bool,
    /// Command file format
    pub output_format: OutputFormat,
    /// Output directory for command files
    pub output_dir: String,
    /// Argument placeholder format ("$ARGUMENTS" or "{{args}}")
    pub arg_placeholder: String,
}

/// Get the agent configuration list
///
/// This is the single source of truth for all supported AI agents.
pub fn get_agent_configs() -> Vec<AgentConfig> {
    vec![
        AgentConfig {
            key: "claude".to_string(),
            name: "Claude Code".to_string(),
            folder: ".claude".to_string(),
            install_url: Some("https://claude.ai/code".to_string()),
            requires_cli: true,
            output_format: OutputFormat::Markdown,
            output_dir: ".claude/commands".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
        AgentConfig {
            key: "gemini".to_string(),
            name: "Google Gemini".to_string(),
            folder: ".gemini".to_string(),
            install_url: Some("https://ai.google.dev/".to_string()),
            requires_cli: true,
            output_format: OutputFormat::Toml,
            output_dir: ".gemini/commands".to_string(),
            arg_placeholder: "{{args}}".to_string(),
        },
        AgentConfig {
            key: "copilot".to_string(),
            name: "GitHub Copilot".to_string(),
            folder: ".github".to_string(),
            install_url: None,
            requires_cli: false,
            output_format: OutputFormat::AgentMd,
            output_dir: ".github/agents".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
        AgentConfig {
            key: "cursor-agent".to_string(),
            name: "Cursor".to_string(),
            folder: ".cursor".to_string(),
            install_url: Some("https://cursor.sh/".to_string()),
            requires_cli: true,
            output_format: OutputFormat::Markdown,
            output_dir: ".cursor/commands".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
        AgentConfig {
            key: "qwen".to_string(),
            name: "Qwen Code".to_string(),
            folder: ".qwen".to_string(),
            install_url: Some("https://qwenlm.github.io/".to_string()),
            requires_cli: true,
            output_format: OutputFormat::Toml,
            output_dir: ".qwen/commands".to_string(),
            arg_placeholder: "{{args}}".to_string(),
        },
        AgentConfig {
            key: "opencode".to_string(),
            name: "opencode".to_string(),
            folder: ".opencode".to_string(),
            install_url: Some("https://opencode.dev/".to_string()),
            requires_cli: true,
            output_format: OutputFormat::Markdown,
            output_dir: ".opencode/command".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
        AgentConfig {
            key: "codex".to_string(),
            name: "Codex CLI".to_string(),
            folder: ".codex".to_string(),
            install_url: Some("https://codex.ai/".to_string()),
            requires_cli: true,
            output_format: OutputFormat::Markdown,
            output_dir: ".codex/prompts".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
        AgentConfig {
            key: "windsurf".to_string(),
            name: "Windsurf".to_string(),
            folder: ".windsurf".to_string(),
            install_url: None,
            requires_cli: false,
            output_format: OutputFormat::Markdown,
            output_dir: ".windsurf/workflows".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
        AgentConfig {
            key: "kilocode".to_string(),
            name: "Kilo Code".to_string(),
            folder: ".kilocode".to_string(),
            install_url: None,
            requires_cli: false,
            output_format: OutputFormat::Markdown,
            output_dir: ".kilocode/workflows".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
        AgentConfig {
            key: "auggie".to_string(),
            name: "Auggie CLI".to_string(),
            folder: ".augment".to_string(),
            install_url: Some("https://auggie.ai/".to_string()),
            requires_cli: true,
            output_format: OutputFormat::Markdown,
            output_dir: ".augment/commands".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
        AgentConfig {
            key: "roo".to_string(),
            name: "Roo Code".to_string(),
            folder: ".roo".to_string(),
            install_url: None,
            requires_cli: false,
            output_format: OutputFormat::Markdown,
            output_dir: ".roo/commands".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
        AgentConfig {
            key: "codebuddy".to_string(),
            name: "CodeBuddy CLI".to_string(),
            folder: ".codebuddy".to_string(),
            install_url: Some("https://codebuddy.ai/".to_string()),
            requires_cli: true,
            output_format: OutputFormat::Markdown,
            output_dir: ".codebuddy/commands".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
        AgentConfig {
            key: "qoder".to_string(),
            name: "Qoder CLI".to_string(),
            folder: ".qoder".to_string(),
            install_url: Some("https://qoder.ai/".to_string()),
            requires_cli: true,
            output_format: OutputFormat::Markdown,
            output_dir: ".qoder/commands".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
        AgentConfig {
            key: "amp".to_string(),
            name: "Amp".to_string(),
            folder: ".agents".to_string(),
            install_url: Some("https://amp.dev/".to_string()),
            requires_cli: true,
            output_format: OutputFormat::Markdown,
            output_dir: ".agents/commands".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
        AgentConfig {
            key: "shai".to_string(),
            name: "SHAI".to_string(),
            folder: ".shai".to_string(),
            install_url: Some("https://shai.ai/".to_string()),
            requires_cli: true,
            output_format: OutputFormat::Markdown,
            output_dir: ".shai/commands".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
        AgentConfig {
            key: "q".to_string(),
            name: "Amazon Q Developer".to_string(),
            folder: ".amazonq".to_string(),
            install_url: Some("https://aws.amazon.com/q/".to_string()),
            requires_cli: true,
            output_format: OutputFormat::Markdown,
            output_dir: ".amazonq/prompts".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
        AgentConfig {
            key: "bob".to_string(),
            name: "IBM Bob".to_string(),
            folder: ".bob".to_string(),
            install_url: None,
            requires_cli: false,
            output_format: OutputFormat::Markdown,
            output_dir: ".bob/commands".to_string(),
            arg_placeholder: "$ARGUMENTS".to_string(),
        },
    ]
}

/// Get agent configuration by key
pub fn get_agent_config(key: &str) -> Option<AgentConfig> {
    get_agent_configs()
        .into_iter()
        .find(|agent| agent.key == key)
}

/// Validate agent key
pub fn validate_agent_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("Agent key cannot be empty".to_string());
    }

    if !key
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "Agent key '{}' contains invalid characters. Only alphanumeric, hyphen, and underscore are allowed.",
            key
        ));
    }

    if get_agent_config(key).is_none() {
        return Err(format!(
            "Unknown agent key '{}'. Available agents: {}",
            key,
            get_agent_configs()
                .iter()
                .map(|a| a.key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    Ok(())
}

/// Get all agent keys
pub fn get_all_agent_keys() -> Vec<String> {
    get_agent_configs().iter().map(|a| a.key.clone()).collect()
}

/// Agent selection enum
///
/// Represents user's agent selection (interactive or CLI argument).
#[derive(Debug, Clone)]
pub enum AgentSelection {
    /// Agent key selected
    Selected(String),
    /// Trigger interactive selection
    Interactive,
    /// Use default (copilot)
    Default,
}

impl AgentSelection {
    /// Resolve to a concrete agent key
    pub fn resolve(&self) -> String {
        match self {
            Self::Selected(key) => key.clone(),
            Self::Default => "copilot".to_string(),
            Self::Interactive => {
                // This will be handled by TUI in a later phase
                "copilot".to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_variant_default() {
        let variant = ScriptVariant::default_for_platform();
        assert!(matches!(variant, ScriptVariant::Sh | ScriptVariant::Ps));
    }

    #[test]
    fn test_validate_agent_key() {
        assert!(validate_agent_key("claude").is_ok());
        assert!(validate_agent_key("invalid").is_err());
        assert!(validate_agent_key("").is_err());
    }

    #[test]
    fn test_get_agent_config() {
        assert!(get_agent_config("claude").is_some());
        assert!(get_agent_config("invalid").is_none());
    }

    #[test]
    fn test_all_17_agents_present() {
        assert_eq!(get_agent_configs().len(), 17);
    }
}
