//! Package Registry Data Structures
//!
//! This module defines data structures for managing package registries,
//! including local installation registries and remote package discovery.

use crate::models::package::{InstalledPackage, PackageRegistryEntry};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Local package registry that tracks installed packages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRegistry {
    /// Map of package name -> installed package info
    pub packages: HashMap<String, InstalledPackage>,
    /// Registry format version
    pub version: String,
}

impl LocalRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            packages: HashMap::new(),
            version: "1.0".to_string(),
        }
    }

    /// Add or update an installed package
    pub fn add_package(&mut self, package: InstalledPackage) {
        self.packages.insert(package.package.name.clone(), package);
    }

    /// Remove a package from the registry
    pub fn remove_package(&mut self, package_name: &str) -> Option<InstalledPackage> {
        self.packages.remove(package_name)
    }

    /// Get installed package by name
    pub fn get_package(&self, package_name: &str) -> Option<&InstalledPackage> {
        self.packages.get(package_name)
    }

    /// Check if a package is installed
    pub fn is_installed(&self, package_name: &str) -> bool {
        self.packages.contains_key(package_name)
    }

    /// Get all installed packages
    pub fn list_packages(&self) -> Vec<&InstalledPackage> {
        self.packages.values().collect()
    }

    /// Get packages by author
    pub fn packages_by_author(&self, author: &str) -> Vec<&InstalledPackage> {
        self.packages
            .values()
            .filter(|pkg| pkg.package.authors.iter().any(|a| a.contains(author)))
            .collect()
    }

    /// Load registry from filesystem
    pub fn load_from_file(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            Ok(toml::from_str(&content)?)
        } else {
            Ok(Self::new())
        }
    }

    /// Save registry to filesystem
    pub fn save_to_file(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        let content = toml::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }
}

impl Default for LocalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Remote package registry for search and discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteRegistry {
    /// Base URL of the registry
    pub base_url: String,
    /// Registry name
    pub name: String,
    /// Cached package entries
    pub packages: HashMap<String, PackageRegistryEntry>,
    /// Last update timestamp
    pub last_updated: Option<chrono::DateTime<chrono::Utc>>,
}

impl RemoteRegistry {
    /// Create a new remote registry
    pub fn new(base_url: String, name: String) -> Self {
        Self {
            base_url,
            name,
            packages: HashMap::new(),
            last_updated: None,
        }
    }

    /// Add or update a package entry
    pub fn add_entry(&mut self, entry: PackageRegistryEntry) {
        self.packages.insert(entry.name.clone(), entry);
        self.last_updated = Some(chrono::Utc::now());
    }

    /// Get package entry by name
    pub fn get_entry(&self, package_name: &str) -> Option<&PackageRegistryEntry> {
        self.packages.get(package_name)
    }

    /// Search packages by name, description, or tags
    pub fn search(&self, query: &str, limit: usize) -> Vec<&PackageRegistryEntry> {
        let query_lower = query.to_lowercase();

        let mut matches: Vec<_> = self
            .packages
            .values()
            .filter(|entry| {
                entry.name.to_lowercase().contains(&query_lower)
                    || entry.description.to_lowercase().contains(&query_lower)
            })
            .collect();

        // Sort by relevance (name matches first, then description matches)
        matches.sort_by(|a, b| {
            let a_name_match = a.name.to_lowercase().contains(&query_lower);
            let b_name_match = b.name.to_lowercase().contains(&query_lower);

            match (a_name_match, b_name_match) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });

        matches.into_iter().take(limit).collect()
    }

    /// Get all packages sorted by name
    pub fn list_all(&self) -> Vec<&PackageRegistryEntry> {
        let mut entries: Vec<_> = self.packages.values().collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }

    /// Check if registry needs refresh (older than specified duration)
    pub fn needs_refresh(&self, max_age_minutes: i64) -> bool {
        match self.last_updated {
            Some(updated) => {
                let age = chrono::Utc::now().signed_duration_since(updated);
                age.num_minutes() > max_age_minutes
            }
            None => true, // Never updated
        }
    }
}

/// Search result with ranking information
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Package entry
    pub entry: PackageRegistryEntry,
    /// Search relevance score (higher is better)
    pub relevance_score: f32,
    /// Why this result matched
    pub match_reason: String,
}

impl SearchResult {
    /// Create a search result
    pub fn new(entry: PackageRegistryEntry, relevance_score: f32, match_reason: String) -> Self {
        Self {
            entry,
            relevance_score,
            match_reason,
        }
    }
}

/// Registry configuration for multiple sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    /// List of remote registries to search
    pub remotes: Vec<String>,
    /// Cache directory for registry data
    pub cache_dir: String,
    /// Cache TTL in minutes
    pub cache_ttl_minutes: i64,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            remotes: vec![
                "https://api.github.com".to_string(), // GitHub API as primary registry
            ],
            cache_dir: ".aikit/cache".to_string(),
            cache_ttl_minutes: 60, // 1 hour
        }
    }
}

/// Package installation request
#[derive(Debug, Clone)]
pub struct InstallRequest {
    /// Package name or repository URL
    pub source: String,
    /// Specific version to install (optional)
    pub version: Option<String>,
    /// Force reinstall even if already installed
    pub force: bool,
    /// Skip .gitignore modification prompt
    pub skip_gitignore: bool,
}

impl InstallRequest {
    /// Create a new install request
    pub fn new(source: String) -> Self {
        Self {
            source,
            version: None,
            force: false,
            skip_gitignore: false,
        }
    }
}

/// Package update request
#[derive(Debug, Clone)]
pub struct UpdateRequest {
    /// Package name to update
    pub package_name: String,
    /// Allow breaking changes (default: false)
    pub allow_breaking: bool,
}

impl UpdateRequest {
    /// Create a new update request
    pub fn new(package_name: String) -> Self {
        Self {
            package_name,
            allow_breaking: false,
        }
    }
}
