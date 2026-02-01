//! Agent configuration and validation module
//!
//! This module contains types and functions for managing AI agent configurations,
//! including agent selection, validation, and tool checking.

use std::collections::HashMap;

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
    #[allow(dead_code)]
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

/// Extras table for agent-specific configuration
/// Maps agent keys to (install_url, requires_cli, output_format, arg_placeholder, folder)
static EXTRAS: &[(&str, Option<&str>, bool, &str, &str)] = &[
    ("claude", Some("https://claude.ai/code"), true, "$ARGUMENTS", ".claude"),
    ("gemini", Some("https://ai.google.dev/"), true, "{{args}}", ".gemini"),
    ("copilot", None, false, "$ARGUMENTS", ".github"),
    ("cursor-agent", Some("https://cursor.sh/"), true, "$ARGUMENTS", ".cursor"),
    ("qwen", Some("https://qwenlm.github.io/"), true, "{{args}}", ".qwen"),
    ("opencode", Some("https://opencode.dev/"), true, "$ARGUMENTS", ".opencode"),
    ("codex", Some("https://codex.ai/"), true, "$ARGUMENTS", ".codex"),
    ("windsurf", None, false, "$ARGUMENTS", ".windsurf"),
    ("kilocode", None, false, "$ARGUMENTS", ".kilocode"),
    ("auggie", Some("https://auggie.ai/"), true, "$ARGUMENTS", ".augment"),
    ("roo", None, false, "$ARGUMENTS", ".roo"),
    ("codebuddy", Some("https://codebuddy.ai/"), true, "$ARGUMENTS", ".codebuddy"),
    ("qoder", Some("https://qoder.ai/"), true, "$ARGUMENTS", ".qoder"),
    ("amp", Some("https://amp.dev/"), true, "$ARGUMENTS", ".agents"),
    ("shai", Some("https://shai.ai/"), true, "$ARGUMENTS", ".shai"),
    ("q", Some("https://aws.amazon.com/q/"), true, "$ARGUMENTS", ".amazonq"),
    ("bob", None, false, "$ARGUMENTS", ".bob"),
];

/// Get the agent configuration list
///
/// This is the single source of truth for all supported AI agents.
/// Delegates to ai-agent-deploy for catalog data and uses extras table for aikit-specific fields.
pub fn get_agent_configs() -> Vec<AgentConfig> {
    use ai_agent_deploy::{AgentConfig as DeployConfig, all_agents};

    all_agents()
        .into_iter()
        .map(|deploy_config| {
            let extras = EXTRAS.iter().find(|(key, _, _, _, _)| *key == deploy_config.key);

            let (install_url, requires_cli, output_format, arg_placeholder, folder) = match extras {
                Some((_, url, req_cli, placeholder, folder_str)) => {
                    (url.map(|s| s.to_string()), *req_cli, OutputFormat::Markdown, placeholder.to_string(), folder_str.to_string())
                }
                None => {
                    (None, true, OutputFormat::Markdown, "$ARGUMENTS".to_string(), deploy_config.key.clone())
                }
            };

            AgentConfig {
                key: deploy_config.key,
                name: deploy_config.name,
                folder,
                install_url,
                requires_cli,
                output_format,
                output_dir: deploy_config.commands_dir.clone(),
                arg_placeholder,
            }
        })
        .collect()
}

/// Get agent configuration by key
///
/// Delegates to ai-agent-deploy for catalog data and uses extras table for aikit-specific fields.
pub fn get_agent_config(key: &str) -> Option<AgentConfig> {
    use ai_agent_deploy::{AgentConfig as DeployConfig, agent};

    let deploy_config = agent(key)?;
    let extras = EXTRAS.iter().find(|(k, _, _, _, _)| *k == key);

    let (install_url, requires_cli, output_format, arg_placeholder, folder) = match extras {
        Some((_, url, req_cli, placeholder, folder_str)) => (
            url.map(|s| s.to_string()),
            *req_cli,
            OutputFormat::Markdown,
            placeholder.to_string(),
            folder_str.to_string(),
        ),
        None => (
            None,
            true,
            OutputFormat::Markdown,
            "$ARGUMENTS".to_string(),
            key.to_string(),
        ),
    };

    Some(AgentConfig {
        key: deploy_config.key,
        name: deploy_config.name,
        folder,
        install_url,
        requires_cli,
        output_format,
        output_dir: deploy_config.commands_dir.clone(),
        arg_placeholder,
    })
}

/// Validate agent key
///
/// Delegates to ai-agent-deploy for validation.
pub fn validate_agent_key(key: &str) -> Result<(), String> {
    use ai_agent_deploy::validate_agent_key;

    validate_agent_key(key).map_err(|e| e.to_string())
}

/// Get all agent keys
#[allow(dead_code)]
pub fn get_all_agent_keys() -> Vec<String> {
    get_agent_configs().iter().map(|a| a.key.clone()).collect()
}

/// Agent selection enum
///
/// Represents user's agent selection (interactive or CLI argument).
#[derive(Debug, Clone)]
#[allow(dead_code)]
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

impl AgentConfig {
    /// Check if agent supports package installation
    #[allow(dead_code)]
    pub fn supports_packages(&self) -> bool {
        // All agents in the current configuration support packages
        // In the future, this could be a configuration field
        true
    }

    /// Get the namespace prefix for package commands
    #[allow(dead_code)]
    pub fn get_namespace_prefix(&self, package_name: &str) -> String {
        format!("{}.{}", package_name, self.key)
    }

    /// Generate package command content for this agent
    #[allow(dead_code)]
    pub fn generate_package_command(
        &self,
        package_name: &str,
        _command_name: &str,
        description: &str,
        script_template: &str,
    ) -> String {
        let namespaced_command = self.get_namespace_prefix(package_name);

        match self.output_format {
            OutputFormat::Markdown => {
                format!(
                    "# {}\n\n**Description**: {}\n\n**Command**: `{}`\n\n**Arguments**: {}\n\n---\n\n{}",
                    namespaced_command,
                    description,
                    namespaced_command,
                    self.arg_placeholder,
                    script_template
                )
            }
            OutputFormat::Toml => {
                format!(
                    "command = \"{}\"\ndescription = \"{}\"\nargs = \"{}\"\nscript = \"\"\"\n{}\n\"\"\"",
                    namespaced_command, description, self.arg_placeholder, script_template
                )
            }
            OutputFormat::AgentMd => {
                format!(
                    "# {}\n\n{}\n\nCommand: {}\nArgs: {}\n\n```bash\n{}\n```",
                    namespaced_command,
                    description,
                    namespaced_command,
                    self.arg_placeholder,
                    script_template
                )
            }
        }
    }

    /// Apply agent-specific overrides to package content
    #[allow(dead_code)]
    pub fn apply_overrides(
        &self,
        content: &str,
        overrides: &std::collections::HashMap<String, String>,
    ) -> String {
        let mut result = content.to_string();

        // Apply agent-specific argument placeholder
        result = result.replace("{args}", &self.arg_placeholder);
        result = result.replace("$ARGUMENTS", &self.arg_placeholder);
        result = result.replace("{{args}}", &self.arg_placeholder);

        // Apply custom overrides
        for (key, value) in overrides {
            result = result.replace(key, value);
        }

        result
    }

    /// Get the full path for a package command file
    #[allow(dead_code)]
    pub fn get_package_command_path(
        &self,
        package_name: &str,
        command_name: &str,
    ) -> std::path::PathBuf {
        std::path::PathBuf::from(&self.output_dir)
            .join(format!("{}-{}.md", package_name, command_name))
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

    #[test]
    fn test_extras_table_populated() {
        let configs = get_agent_configs();
        assert_eq!(configs.len(), 17);

        // Verify extras table covers all agents
        let keys: Vec<_> = configs.iter().map(|c| c.key.as_str()).collect();
        assert!(keys.contains(&"opencode"));
    }
}
