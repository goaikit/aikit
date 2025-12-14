//! Package registry management
//!
//! This module handles local and remote package registries,
//! including package discovery, installation tracking, and caching.

use crate::models::package::PackageRegistryEntry;
use crate::models::registry::{LocalRegistry, RegistryConfig, RemoteRegistry};
use std::path::PathBuf;

/// Registry manager for coordinating local and remote registries
pub struct RegistryManager {
    config: RegistryConfig,
    local: LocalRegistry,
    remotes: Vec<RemoteRegistry>,
}

impl RegistryManager {
    /// Create a new registry manager
    pub fn new(config: RegistryConfig) -> Self {
        Self {
            config,
            local: LocalRegistry::new(),
            remotes: Vec::new(),
        }
    }

    /// Load local registry from disk
    pub fn load_local(&mut self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            self.local = toml::from_str(&content)?;
        }
        Ok(())
    }

    /// Save local registry to disk
    pub fn save_local(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        let content = toml::to_string_pretty(&self.local)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Search for packages across all registries
    pub async fn search(
        &mut self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<PackageRegistryEntry>, Box<dyn std::error::Error>> {
        let mut results = Vec::new();

        // Search each remote registry
        for remote in &self.remotes {
            let remote_results = remote.search(query, limit);
            results.extend(remote_results.into_iter().cloned());
        }

        // Remove duplicates and sort by relevance
        results.sort_by(|a, b| a.name.cmp(&b.name));
        results.dedup_by(|a, b| a.name == b.name);

        // Limit results
        results.truncate(limit);

        Ok(results)
    }

    /// Refresh a remote registry
    async fn refresh_remote(
        &self,
        remote: &mut RemoteRegistry,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Implement remote registry refresh
        // - Query GitHub API for repositories
        // - Parse package.toml from repositories
        // - Update cached registry entries

        remote.last_updated = Some(chrono::Utc::now());
        Ok(())
    }

    /// Get local registry reference
    pub fn local(&self) -> &LocalRegistry {
        &self.local
    }

    /// Get mutable local registry reference
    pub fn local_mut(&mut self) -> &mut LocalRegistry {
        &mut self.local
    }
}

impl Default for RegistryManager {
    fn default() -> Self {
        Self::new(RegistryConfig::default())
    }
}
