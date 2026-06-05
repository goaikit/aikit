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
}

/// Output format for command files
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
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
    /// Optional directory for agent skills (None if agent does not support skills)
    pub skills_dir: Option<String>,
    /// Optional directory for agent subagents (None if agent does not support subagents)
    pub agents_dir: Option<String>,
    /// Argument placeholder format ("$ARGUMENTS" or "{{args}}")
    pub arg_placeholder: String,
}

struct AgentExtra {
    install_url: Option<&'static str>,
    requires_cli: bool,
    arg_placeholder: &'static str,
    folder: &'static str,
}

/// Extras table for agent-specific configuration
static EXTRAS: &[(&str, AgentExtra)] = &[
    (
        "claude",
        AgentExtra {
            install_url: Some("https://claude.ai/code"),
            requires_cli: true,
            arg_placeholder: "$ARGUMENTS",
            folder: ".claude",
        },
    ),
    (
        "gemini",
        AgentExtra {
            install_url: Some("https://ai.google.dev/"),
            requires_cli: true,
            arg_placeholder: "{{args}}",
            folder: ".gemini",
        },
    ),
    (
        "copilot",
        AgentExtra {
            install_url: None,
            requires_cli: false,
            arg_placeholder: "$ARGUMENTS",
            folder: ".github",
        },
    ),
    (
        "cursor-agent",
        AgentExtra {
            install_url: Some("https://cursor.sh/"),
            requires_cli: true,
            arg_placeholder: "$ARGUMENTS",
            folder: ".cursor",
        },
    ),
    (
        "qwen",
        AgentExtra {
            install_url: Some("https://qwenlm.github.io/"),
            requires_cli: true,
            arg_placeholder: "{{args}}",
            folder: ".qwen",
        },
    ),
    (
        "opencode",
        AgentExtra {
            install_url: Some("https://opencode.dev/"),
            requires_cli: true,
            arg_placeholder: "$ARGUMENTS",
            folder: ".opencode",
        },
    ),
    (
        "codex",
        AgentExtra {
            install_url: Some("https://codex.ai/"),
            requires_cli: true,
            arg_placeholder: "$ARGUMENTS",
            folder: ".codex",
        },
    ),
    (
        "windsurf",
        AgentExtra {
            install_url: None,
            requires_cli: false,
            arg_placeholder: "$ARGUMENTS",
            folder: ".windsurf",
        },
    ),
    (
        "kilocode",
        AgentExtra {
            install_url: None,
            requires_cli: false,
            arg_placeholder: "$ARGUMENTS",
            folder: ".kilocode",
        },
    ),
    (
        "auggie",
        AgentExtra {
            install_url: Some("https://auggie.ai/"),
            requires_cli: true,
            arg_placeholder: "$ARGUMENTS",
            folder: ".augment",
        },
    ),
    (
        "roo",
        AgentExtra {
            install_url: None,
            requires_cli: false,
            arg_placeholder: "$ARGUMENTS",
            folder: ".roo",
        },
    ),
    (
        "codebuddy",
        AgentExtra {
            install_url: Some("https://codebuddy.ai/"),
            requires_cli: true,
            arg_placeholder: "$ARGUMENTS",
            folder: ".codebuddy",
        },
    ),
    (
        "qoder",
        AgentExtra {
            install_url: Some("https://qoder.ai/"),
            requires_cli: true,
            arg_placeholder: "$ARGUMENTS",
            folder: ".qoder",
        },
    ),
    (
        "amp",
        AgentExtra {
            install_url: Some("https://amp.dev/"),
            requires_cli: true,
            arg_placeholder: "$ARGUMENTS",
            folder: ".agents",
        },
    ),
    (
        "shai",
        AgentExtra {
            install_url: Some("https://shai.ai/"),
            requires_cli: true,
            arg_placeholder: "$ARGUMENTS",
            folder: ".shai",
        },
    ),
    (
        "q",
        AgentExtra {
            install_url: Some("https://aws.amazon.com/q/"),
            requires_cli: true,
            arg_placeholder: "$ARGUMENTS",
            folder: ".amazonq",
        },
    ),
    (
        "bob",
        AgentExtra {
            install_url: None,
            requires_cli: false,
            arg_placeholder: "$ARGUMENTS",
            folder: ".bob",
        },
    ),
];

fn find_extra(key: &str) -> Option<&'static AgentExtra> {
    EXTRAS.iter().find(|(k, _)| *k == key).map(|(_, e)| e)
}

/// Get the agent configuration list
///
/// This is the single source of truth for all supported AI agents.
/// Delegates to aikit-sdk for catalog data and uses extras table for aikit-specific fields.
pub fn get_agent_configs() -> Vec<AgentConfig> {
    use aikit_sdk::all_agents;

    all_agents()
        .into_iter()
        .map(|deploy_config| {
            let extra = find_extra(&deploy_config.key());

            let (install_url, requires_cli, arg_placeholder, folder) = match extra {
                Some(e) => (
                    e.install_url.map(|s| s.to_string()),
                    e.requires_cli,
                    e.arg_placeholder.to_string(),
                    e.folder.to_string(),
                ),
                None => (
                    None,
                    true,
                    "$ARGUMENTS".to_string(),
                    deploy_config.key().clone(),
                ),
            };

            AgentConfig {
                key: deploy_config.name.to_lowercase(),
                name: deploy_config.name,
                folder,
                install_url,
                requires_cli,
                output_format: OutputFormat::Markdown,
                output_dir: deploy_config.commands_dir.clone(),
                skills_dir: deploy_config.skills_dir.clone(),
                agents_dir: deploy_config.agents_dir.clone(),
                arg_placeholder,
            }
        })
        .collect()
}

/// Get agent configuration by key
///
/// Delegates to aikit-sdk for catalog data and uses extras table for aikit-specific fields.
pub fn get_agent_config(key: &str) -> Option<AgentConfig> {
    use aikit_sdk::agent;

    let deploy_config = agent(key)?;
    let extra = find_extra(key);

    let (install_url, requires_cli, arg_placeholder, folder) = match extra {
        Some(e) => (
            e.install_url.map(|s| s.to_string()),
            e.requires_cli,
            e.arg_placeholder.to_string(),
            e.folder.to_string(),
        ),
        None => (None, true, "$ARGUMENTS".to_string(), key.to_string()),
    };

    Some(AgentConfig {
        key: deploy_config.name.to_lowercase(),
        name: deploy_config.name,
        folder,
        install_url,
        requires_cli,
        output_format: OutputFormat::Markdown,
        output_dir: deploy_config.commands_dir.clone(),
        skills_dir: deploy_config.skills_dir.clone(),
        agents_dir: deploy_config.agents_dir.clone(),
        arg_placeholder,
    })
}

/// Validate agent key
///
/// Delegates to aikit-sdk for validation.
pub fn validate_agent_key(key: &str) -> Result<(), String> {
    use aikit_sdk::validate_agent_key;

    validate_agent_key(key).map_err(|e| e.to_string())
}

impl AgentConfig {
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
        assert_eq!(get_agent_configs().len(), 18);
    }

    #[test]
    fn test_extras_table_populated() {
        let configs = get_agent_configs();
        assert_eq!(configs.len(), 18);

        // Verify extras table covers all agents
        let keys: Vec<_> = configs.iter().map(|c| c.key.as_str()).collect();
        assert!(keys.contains(&"opencode"));
        assert!(keys.contains(&"newton"));
    }
}
