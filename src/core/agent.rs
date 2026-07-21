//! Agent configuration and validation module
//!
//! This module contains types and functions for managing AI agent configurations,
//! including agent selection, validation, and tool checking.
//!
//! ADR 0015: this module carries no agent data of its own. It is a thin
//! translation layer over aikit-sdk's canonical deploy-layout registry
//! (`aikit_sdk::{AgentConfig, all_agents, agent}`) plus the runnable-backend
//! derivation (`aikit_sdk::requires_cli`). The former `EXTRAS` table here
//! (install_url/requires_cli/arg_placeholder/folder overrides, keyed
//! separately from the SDK catalog) has been folded into that canonical
//! registry and deleted.

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
///
/// Mirrors `aikit_sdk::DeployOutputFormat` one-to-one (see `From` impl
/// below); kept as a distinct local type so this crate's public API is
/// insulated from the SDK's internal naming.
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

impl From<aikit_sdk::DeployOutputFormat> for OutputFormat {
    fn from(value: aikit_sdk::DeployOutputFormat) -> Self {
        match value {
            aikit_sdk::DeployOutputFormat::Markdown => OutputFormat::Markdown,
            aikit_sdk::DeployOutputFormat::Toml => OutputFormat::Toml,
            aikit_sdk::DeployOutputFormat::AgentMd => OutputFormat::AgentMd,
        }
    }
}

/// Agent configuration
///
/// Represents an AI agent configuration with all metadata needed for
/// initialization and tool checking. Every field here is sourced from
/// aikit-sdk's canonical deploy-layout registry (ADR 0015); `requires_cli` is
/// the one exception — it is never stored anywhere, and is derived fresh
/// from `runner::Backend` membership via `aikit_sdk::requires_cli`.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Canonical catalog key (e.g., "claude", "gemini", "cursor")
    pub key: String,
    /// Display name (e.g., "Claude", "Google Gemini")
    pub name: String,
    /// Project directory (e.g., ".claude", ".gemini") — the parent of `output_dir`
    pub folder: String,
    /// Optional installation URL
    pub install_url: Option<String>,
    /// Whether agent requires CLI tool check (derived from Backend membership)
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

/// Derives an agent's base config folder (e.g. `.claude`) from its commands
/// directory (e.g. `.claude/commands`).
///
/// Every agent in the canonical catalog places its commands directory one
/// level under its base folder, so `folder` does not need to be a separately
/// stored field — it is always the parent of `commands_dir`. (Deriving it
/// this way also fixes a latent bug: agents absent from the old `EXTRAS`
/// table, e.g. `newton`, previously got the raw catalog key as their
/// `folder` instead of a real path.)
fn folder_from_commands_dir(commands_dir: &str) -> String {
    std::path::Path::new(commands_dir)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| commands_dir.to_string())
}

fn from_deploy_config(deploy_config: aikit_sdk::AgentConfig) -> AgentConfig {
    let requires_cli = aikit_sdk::requires_cli(&deploy_config.key);
    AgentConfig {
        folder: folder_from_commands_dir(&deploy_config.commands_dir),
        key: deploy_config.key,
        name: deploy_config.name,
        install_url: deploy_config.install_url,
        requires_cli,
        output_format: deploy_config.output_format.into(),
        output_dir: deploy_config.commands_dir,
        skills_dir: deploy_config.skills_dir,
        agents_dir: deploy_config.agents_dir,
        arg_placeholder: deploy_config.arg_placeholder,
    }
}

/// Get the agent configuration list
///
/// This is the single source of truth for all supported AI agents: it
/// delegates entirely to aikit-sdk's canonical deploy-layout registry (ADR
/// 0015). `requires_cli` is derived per agent from `runner::Backend`
/// membership, not stored.
pub fn get_agent_configs() -> Vec<AgentConfig> {
    aikit_sdk::all_agents()
        .into_iter()
        .map(from_deploy_config)
        .collect()
}

/// Get agent configuration by key
///
/// Delegates to aikit-sdk's canonical deploy-layout registry (ADR 0015).
pub fn get_agent_config(key: &str) -> Option<AgentConfig> {
    aikit_sdk::agent(key).map(from_deploy_config)
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

        // Every agent now resolves through the canonical SDK registry.
        let keys: Vec<_> = configs.iter().map(|c| c.key.as_str()).collect();
        assert!(keys.contains(&"opencode"));
        assert!(keys.contains(&"newton"));
    }

    // ---- ADR 0015: single canonical registry (deploy-layout ⟂ Backend) ----

    #[test]
    fn test_single_canonical_key_cursor_not_cursor_agent() {
        // The historical `cursor` vs `cursor-agent` split is gone: only the
        // canonical `cursor` key resolves, in both this crate and the SDK.
        assert!(get_agent_config("cursor-agent").is_none());
        let cursor = get_agent_config("cursor").expect("cursor must resolve");
        assert_eq!(cursor.key, "cursor");
    }

    #[test]
    fn test_agent_config_key_matches_lookup_key_for_every_agent() {
        // Regression guard for the old bug where `.key` was derived from
        // `name.to_lowercase()` (e.g. "claude code") instead of the
        // canonical catalog key, silently breaking every `.key`-keyed lookup
        // for multi-word agent names.
        for config in get_agent_configs() {
            let by_key = get_agent_config(&config.key);
            assert!(
                by_key.is_some(),
                "agent config key '{}' must round-trip through get_agent_config",
                config.key
            );
            assert_eq!(by_key.unwrap().key, config.key);
        }
    }

    #[test]
    fn test_divergent_values_now_single_and_correct() {
        // gemini: EXTRAS said requires_cli=true (Gemini CLI is genuinely
        // required); the deleted models/config.rs table said false. Now
        // derived from Backend membership instead of stored at all.
        let gemini = get_agent_config("gemini").unwrap();
        assert!(gemini.requires_cli);

        // cursor: EXTRAS said arg_placeholder "$ARGUMENTS" (matches Cursor's
        // actual command-file substitution syntax); the deleted
        // models/config.rs table said "{args}".
        let cursor = get_agent_config("cursor").unwrap();
        assert_eq!(cursor.arg_placeholder, "$ARGUMENTS");

        // copilot: output dir now comes solely from the SDK catalog
        // (".github/agents"), not the deleted models/config.rs table's stale
        // ".github/copilot-instructions".
        let copilot = get_agent_config("copilot").unwrap();
        assert_eq!(copilot.output_dir, ".github/agents");
        assert_eq!(copilot.folder, ".github");
    }

    #[test]
    fn test_requires_cli_derived_for_runnable_deploy_only_and_aikit() {
        // Runnable external agent (a Backend): requires its CLI.
        assert!(get_agent_config("claude").unwrap().requires_cli);
        // Deploy-only agent (never a Backend, e.g. Copilot): no CLI to require.
        assert!(!get_agent_config("copilot").unwrap().requires_cli);
        // `aikit` itself is not in this deploy-layout catalog (it has no
        // deploy layout — it's the in-process runnable backend), so it is
        // correctly absent here; its requires_cli=false is asserted directly
        // against the SDK in aikit-sdk's own test suite.
        assert!(get_agent_config("aikit").is_none());
        assert!(!aikit_sdk::requires_cli("aikit"));
    }
}
