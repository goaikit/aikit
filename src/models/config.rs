//! Configuration Data Structures
//!
//! This module defines configuration structures for AIKIT's template package system,
//! including global settings and user preferences.
//!
//! ADR 0015: agent configuration is no longer duplicated here. The former
//! `default_agents()` table and its own `AgentConfig`/`OutputFormat` types
//! (a third, independently-drifted copy of agent metadata, distinct from
//! both `aikit_sdk::AgentConfig` and `crate::core::agent::AgentConfig`) have
//! been deleted; the canonical deploy-layout registry lives in aikit-sdk
//! (`aikit_sdk::{AgentConfig, all_agents, agent}`), fronted for this crate by
//! `crate::core::agent`.

#![allow(dead_code)]

use crate::models::registry::RegistryConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Global AIKIT configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AikConfig {
    /// Configuration version
    pub version: String,
    /// Package installation directory (default: ".aikit")
    pub install_dir: String,
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

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.version.is_empty() {
            errors.push("version must be specified".to_string());
        }

        if let Err(registry_errors) = self.registry.validate() {
            errors.extend(registry_errors);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
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
    let local_config = ConfigPaths::local_config();
    let config = if local_config.exists() {
        AikConfig::load(&local_config)?
    } else {
        let global_config = ConfigPaths::global_config();
        if global_config.exists() {
            AikConfig::load(&global_config)?
        } else {
            AikConfig::default()
        }
    };

    config.validate().map_err(|errors| {
        let msg = format!(
            "Configuration validation failed:\n{}",
            errors
                .iter()
                .map(|e| format!("  - {}", e))
                .collect::<Vec<_>>()
                .join("\n")
        );
        Box::<dyn std::error::Error>::from(msg)
    })?;

    Ok(config)
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
