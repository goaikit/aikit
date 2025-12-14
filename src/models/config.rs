//! Configuration Data Structures
//!
//! This module defines configuration structures for AIKIT's universal package system,
//! including global settings, agent configurations, and user preferences.

use crate::models::registry::RegistryConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Global AIKIT configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AikConfig {
    /// Configuration version
    pub version: String,
    /// Package installation directory (default: ".aikit")
    pub install_dir: String,
    /// Agent configurations
    pub agents: HashMap<String, AgentConfig>,
    /// Registry configuration
    pub registry: RegistryConfig,
    /// User preferences
    pub preferences: UserPreferences,
}

impl Default for AikConfig {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            install_dir: ".aikit".to_string(),
            agents: Self::default_agents(),
            registry: RegistryConfig::default(),
            preferences: UserPreferences::default(),
        }
    }
}

impl AikConfig {
    /// Load configuration from file
    pub fn load(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: AikConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get default agent configurations
    fn default_agents() -> HashMap<String, AgentConfig> {
        let mut agents = HashMap::new();

        // Claude Code
        agents.insert(
            "claude".to_string(),
            AgentConfig {
                name: "Claude Code".to_string(),
                key: "claude".to_string(),
                folder: ".claude".to_string(),
                install_url: Some("https://claude.ai/code".to_string()),
                requires_cli: true,
                output_format: OutputFormat::Markdown,
                output_dir: ".claude/commands".to_string(),
                arg_placeholder: "$ARGUMENTS".to_string(),
                extensions: vec![".md".to_string()],
            },
        );

        // Cursor
        agents.insert(
            "cursor".to_string(),
            AgentConfig {
                name: "Cursor".to_string(),
                key: "cursor".to_string(),
                folder: ".cursor".to_string(),
                install_url: None,
                requires_cli: false,
                output_format: OutputFormat::Markdown,
                output_dir: ".cursor/commands".to_string(),
                arg_placeholder: "{args}".to_string(),
                extensions: vec![".md".to_string()],
            },
        );

        // GitHub Copilot
        agents.insert(
            "copilot".to_string(),
            AgentConfig {
                name: "GitHub Copilot".to_string(),
                key: "copilot".to_string(),
                folder: ".github".to_string(),
                install_url: None,
                requires_cli: false,
                output_format: OutputFormat::Markdown,
                output_dir: ".github/copilot-instructions".to_string(),
                arg_placeholder: "{args}".to_string(),
                extensions: vec![".md".to_string()],
            },
        );

        // Gemini (Google AI)
        agents.insert(
            "gemini".to_string(),
            AgentConfig {
                name: "Gemini".to_string(),
                key: "gemini".to_string(),
                folder: ".gemini".to_string(),
                install_url: None,
                requires_cli: false,
                output_format: OutputFormat::Markdown,
                output_dir: ".gemini/prompts".to_string(),
                arg_placeholder: "{args}".to_string(),
                extensions: vec![".md".to_string()],
            },
        );

        // Continue (Codex)
        agents.insert(
            "continue".to_string(),
            AgentConfig {
                name: "Continue".to_string(),
                key: "continue".to_string(),
                folder: ".continue".to_string(),
                install_url: None,
                requires_cli: false,
                output_format: OutputFormat::Markdown,
                output_dir: ".continue/config".to_string(),
                arg_placeholder: "{args}".to_string(),
                extensions: vec![".json".to_string(), ".md".to_string()],
            },
        );

        agents
    }
}

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Display name
    pub name: String,
    /// Internal key (lowercase, no spaces)
    pub key: String,
    /// Configuration folder name
    pub folder: String,
    /// Installation URL (optional)
    pub install_url: Option<String>,
    /// Whether this agent requires CLI installation
    pub requires_cli: bool,
    /// Output format for generated content
    pub output_format: OutputFormat,
    /// Output directory for commands/prompts
    pub output_dir: String,
    /// Placeholder for command arguments
    pub arg_placeholder: String,
    /// Supported file extensions
    pub extensions: Vec<String>,
}

impl AgentConfig {
    /// Get the full output path for a command
    pub fn get_command_path(&self, command_name: &str) -> PathBuf {
        PathBuf::from(&self.output_dir).join(format!("{}.md", command_name))
    }

    /// Check if agent supports a file extension
    pub fn supports_extension(&self, extension: &str) -> bool {
        self.extensions.iter().any(|ext| ext == extension)
    }
}

/// Output format for generated content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputFormat {
    /// Markdown format
    Markdown,
    /// JSON format
    Json,
    /// Plain text
    Plain,
}

/// User preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    /// Auto-add .aikit to .gitignore
    pub auto_gitignore: bool,
    /// Default agent for package installation
    pub default_agent: Option<String>,
    /// Verbose output
    pub verbose: bool,
    /// Confirm before overwriting files
    pub confirm_overwrite: bool,
    /// Timeout for network operations (seconds)
    pub network_timeout: u64,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            auto_gitignore: true,
            default_agent: None,
            verbose: false,
            confirm_overwrite: true,
            network_timeout: 30,
        }
    }
}

/// Package build configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Output directory for built packages
    pub output_dir: String,
    /// Include source files in build
    pub include_sources: bool,
    /// Compress artifacts
    pub compress: bool,
    /// Build for specific agents only
    pub target_agents: Option<Vec<String>>,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            output_dir: "dist".to_string(),
            include_sources: true,
            compress: true,
            target_agents: None,
        }
    }
}

/// Development environment configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevConfig {
    /// Enable debug logging
    pub debug: bool,
    /// Log file path
    pub log_file: Option<String>,
    /// Test data directory
    pub test_data_dir: String,
    /// Mock external services
    pub mock_services: bool,
}

impl Default for DevConfig {
    fn default() -> Self {
        Self {
            debug: false,
            log_file: None,
            test_data_dir: "tests/data".to_string(),
            mock_services: false,
        }
    }
}

/// Configuration file locations
pub struct ConfigPaths;

impl ConfigPaths {
    /// Global configuration file
    pub fn global_config() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".aikit")
            .join("config.toml")
    }

    /// Local project configuration file
    pub fn local_config() -> PathBuf {
        PathBuf::from(".aikit").join("config.toml")
    }

    /// Package registry file
    pub fn registry() -> PathBuf {
        PathBuf::from(".aikit").join("registry.toml")
    }

    /// Installed packages database
    pub fn installed_packages() -> PathBuf {
        PathBuf::from(".aikit").join("installed.toml")
    }
}

/// Load configuration with fallback hierarchy:
/// 1. Local project config (.aikit/config.toml)
/// 2. Global user config (~/.aikit/config.toml)
/// 3. Default configuration
pub fn load_config() -> Result<AikConfig, Box<dyn std::error::Error>> {
    // Try local config first
    let local_config = ConfigPaths::local_config();
    if local_config.exists() {
        return AikConfig::load(&local_config);
    }

    // Try global config
    let global_config = ConfigPaths::global_config();
    if global_config.exists() {
        return AikConfig::load(&global_config);
    }

    // Fall back to defaults
    Ok(AikConfig::default())
}

/// Save configuration to appropriate location
pub fn save_config(config: &AikConfig) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = ConfigPaths::local_config();

    // Ensure directory exists
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    config.save(&config_path)
}
