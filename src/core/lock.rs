//! Package lock file management
//!
//! This module handles package lock files for tracking installed package
//! versions and ensuring reproducible installations.

use crate::models::package::InstalledPackage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Package lock file entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockEntry {
    /// Package name
    pub name: String,
    /// Installed version
    pub version: String,
    /// Installation source (URL or local path)
    pub source: String,
    /// Installation timestamp
    pub installed_at: chrono::DateTime<chrono::Utc>,
    /// Package checksum (optional)
    pub checksum: Option<String>,
}

/// Package lock file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageLock {
    /// Lock file version
    pub version: String,
    /// Locked packages
    pub packages: HashMap<String, LockEntry>,
}

impl PackageLock {
    /// Create a new empty lock file
    pub fn new() -> Self {
        Self {
            version: "1.0".to_string(),
            packages: HashMap::new(),
        }
    }

    /// Add a package to the lock file
    pub fn add_package(&mut self, installed_package: &InstalledPackage) {
        let entry = LockEntry {
            name: installed_package.package.name.clone(),
            version: installed_package.package.version.clone(),
            source: installed_package.source_url.clone(),
            installed_at: installed_package.installed_at,
            checksum: None, // TODO: Calculate checksum
        };

        self.packages
            .insert(installed_package.package.name.clone(), entry);
    }

    /// Remove a package from the lock file
    pub fn remove_package(&mut self, package_name: &str) -> Option<LockEntry> {
        self.packages.remove(package_name)
    }

    /// Check if a package is locked
    pub fn is_locked(&self, package_name: &str) -> bool {
        self.packages.contains_key(package_name)
    }

    /// Get locked version for a package
    pub fn get_locked_version(&self, package_name: &str) -> Option<&str> {
        self.packages
            .get(package_name)
            .map(|entry| entry.version.as_str())
    }

    /// Load lock file from disk
    pub fn load_from_file(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(Self::new())
        }
    }

    /// Save lock file to disk
    pub fn save_to_file(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Validate that installed packages match lock file
    pub fn validate_installation(
        &self,
        installed_packages: &[InstalledPackage],
    ) -> Result<(), Box<dyn std::error::Error>> {
        for installed in installed_packages {
            if let Some(locked) = self.packages.get(&installed.package.name) {
                if locked.version != installed.package.version {
                    return Err(format!(
                        "Package '{}' version mismatch: locked={}, installed={}",
                        installed.package.name, locked.version, installed.package.version
                    )
                    .into());
                }
            }
        }
        Ok(())
    }
}

impl Default for PackageLock {
    fn default() -> Self {
        Self::new()
    }
}

/// Lock file manager
pub struct LockManager {
    lock_file_path: PathBuf,
    lock: PackageLock,
}

impl LockManager {
    /// Create a new lock manager
    pub fn new(aikit_dir: &PathBuf) -> Self {
        let lock_file_path = aikit_dir.join("packages.lock");
        let lock = PackageLock::load_from_file(&lock_file_path).unwrap_or_default();

        Self {
            lock_file_path,
            lock,
        }
    }

    /// Add package to lock file
    pub fn lock_package(
        &mut self,
        installed_package: &InstalledPackage,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.lock.add_package(installed_package);
        self.save()?;
        Ok(())
    }

    /// Remove package from lock file
    pub fn unlock_package(&mut self, package_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.lock.remove_package(package_name);
        self.save()?;
        Ok(())
    }

    /// Check if package is locked
    pub fn is_locked(&self, package_name: &str) -> bool {
        self.lock.is_locked(package_name)
    }

    /// Get locked version
    pub fn get_locked_version(&self, package_name: &str) -> Option<&str> {
        self.lock.get_locked_version(package_name)
    }

    /// Save lock file
    fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.lock.save_to_file(&self.lock_file_path)
    }
}
