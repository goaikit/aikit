//! Package lock file management
//!
//! This module handles package lock files for tracking installed package
//! versions and ensuring reproducible installations.
//!
//! FEAT-4 / SEC-7: this module used to be an entire `#![allow(dead_code)]`
//! module — no lock file was ever written or read, and `checksum` was always
//! `None`. It now has real callers (`src/cli/commands/install.rs`, both
//! `execute_install` and `execute_update`): every GitHub-sourced install
//! resolves the fetched ref to an immutable commit SHA and hashes the
//! downloaded archive, and both are persisted here. A later re-install or
//! update at the *same* recorded version whose freshly computed checksum
//! disagrees with what's on record is treated as an integrity violation
//! (e.g. a branch/tag was moved to different content underneath an
//! unchanged manifest version) and is rejected via [`IntegrityError`].

use crate::models::package::InstalledPackage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

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
    /// Immutable commit SHA the installed ref was resolved to at fetch time
    /// (SEC-7). `None` for local-folder installs, or if resolution failed
    /// and installation proceeded anyway (best-effort, non-fatal).
    #[serde(default)]
    pub commit_sha: Option<String>,
    /// SHA-256 hex digest of the fetched archive (SEC-7). `None` for
    /// local-folder installs, which have no archive to hash.
    #[serde(default)]
    pub checksum: Option<String>,
}

/// Integrity violation detected while consulting the lock file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityError {
    /// A package already locked at `version` was fetched again and produced
    /// a different archive checksum than what's on record — the content
    /// behind a mutable ref changed without a version bump.
    ChecksumMismatch {
        package: String,
        version: String,
        expected: String,
        actual: String,
    },
}

impl fmt::Display for IntegrityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntegrityError::ChecksumMismatch {
                package,
                version,
                expected,
                actual,
            } => write!(
                f,
                "checksum mismatch for package '{}' v{}: locked archive checksum is {}, but the \
                 freshly fetched archive hashes to {}. The source ref may have been moved to \
                 different content without a version bump — refusing to install. If this is \
                 expected, remove the package and lock entry and reinstall.",
                package, version, expected, actual
            ),
        }
    }
}

impl std::error::Error for IntegrityError {}

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

    /// Add a package to the lock file, recording the resolved commit SHA and
    /// archive checksum (SEC-7). Does not itself check for a mismatch — call
    /// [`PackageLock::verify_checksum`] first if that check is wanted.
    pub fn add_package_with_integrity(
        &mut self,
        installed_package: &InstalledPackage,
        commit_sha: Option<String>,
        checksum: Option<String>,
    ) {
        let entry = LockEntry {
            name: installed_package.package.name.clone(),
            version: installed_package.package.version.clone(),
            source: installed_package.source_url.clone(),
            installed_at: installed_package.installed_at,
            commit_sha,
            checksum,
        };

        self.packages
            .insert(installed_package.package.name.clone(), entry);
    }

    /// Verify a freshly computed archive checksum against any existing lock
    /// entry for `name` at the same `version` (SEC-7).
    ///
    /// - No existing entry, or an existing entry at a different version
    ///   (a normal update): `Ok(())` — nothing to compare against.
    /// - An existing entry at the same version with no recorded checksum:
    ///   `Ok(())` — nothing to compare against.
    /// - An existing entry at the same version with a *different* recorded
    ///   checksum: `Err(IntegrityError::ChecksumMismatch)`.
    pub fn verify_checksum(
        &self,
        name: &str,
        version: &str,
        checksum: &str,
    ) -> Result<(), IntegrityError> {
        if let Some(existing) = self.packages.get(name) {
            if existing.version == version {
                if let Some(expected) = &existing.checksum {
                    if expected != checksum {
                        return Err(IntegrityError::ChecksumMismatch {
                            package: name.to_string(),
                            version: version.to_string(),
                            expected: expected.clone(),
                            actual: checksum.to_string(),
                        });
                    }
                }
            }
        }
        Ok(())
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

    /// Validate that installed packages match lock file.
    ///
    /// Not yet wired to a CLI entry point — kept as forward-looking API for
    /// a future `aikit doctor`/CI-verify style consistency check (distinct
    /// from `verify_checksum`, which checks archive *content* integrity
    /// rather than registry/lock *version* agreement).
    #[allow(dead_code)]
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
    /// Create a new lock manager rooted at `aikit_dir` (the `.aikit`
    /// directory) — the lock file lives at `<aikit_dir>/packages.lock`.
    pub fn new(aikit_dir: &Path) -> Self {
        let lock_file_path = aikit_dir.join("packages.lock");
        let lock = PackageLock::load_from_file(&lock_file_path).unwrap_or_default();

        Self {
            lock_file_path,
            lock,
        }
    }

    /// Read-only integrity check (SEC-7): verify `checksum` against any
    /// existing lock entry for `package_name` at `version`, without mutating
    /// the lock file. Intended to run *before* extracting a freshly
    /// downloaded archive, so a mismatch can be rejected before any content
    /// hits disk. See [`PackageLock::verify_checksum`] for the exact rules.
    pub fn verify_checksum(
        &self,
        package_name: &str,
        version: &str,
        checksum: &str,
    ) -> Result<(), IntegrityError> {
        self.lock.verify_checksum(package_name, version, checksum)
    }

    /// Verify `checksum` against any existing lock entry for this package at
    /// the same version (SEC-7), then record the new entry (resolved commit
    /// SHA + checksum) and persist. Returns the [`IntegrityError`] boxed, and
    /// leaves the lock file untouched, if a mismatch is detected — callers
    /// should treat this as fatal and not proceed with extraction/install.
    pub fn lock_package_with_integrity(
        &mut self,
        installed_package: &InstalledPackage,
        commit_sha: Option<String>,
        checksum: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(cs) = &checksum {
            self.lock.verify_checksum(
                &installed_package.package.name,
                &installed_package.package.version,
                cs,
            )?;
        }
        self.lock
            .add_package_with_integrity(installed_package, commit_sha, checksum);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::package::PackageMetadata;
    use tempfile::TempDir;

    fn installed(name: &str, version: &str, source: &str) -> InstalledPackage {
        InstalledPackage {
            package: PackageMetadata {
                name: name.to_string(),
                version: version.to_string(),
                description: "test package".to_string(),
                authors: vec![],
                license: None,
                homepage: None,
                repository: None,
            },
            installed_at: chrono::Utc::now(),
            source_url: source.to_string(),
            install_path: format!("packages/{}-{}", name, version),
        }
    }

    // -- PackageLock round-trip -------------------------------------------

    #[test]
    fn test_lock_round_trip_write_then_read_checksum_present() {
        let temp = TempDir::new().unwrap();
        let lock_path = temp.path().join("packages.lock");

        let mut lock = PackageLock::new();
        lock.add_package_with_integrity(
            &installed("demo", "1.0.0", "owner/demo"),
            Some("abc123".to_string()),
            Some("deadbeef".to_string()),
        );
        lock.save_to_file(&lock_path).unwrap();

        let loaded = PackageLock::load_from_file(&lock_path).unwrap();
        let entry = loaded.packages.get("demo").expect("entry present");
        assert_eq!(entry.version, "1.0.0");
        assert_eq!(entry.commit_sha.as_deref(), Some("abc123"));
        assert_eq!(entry.checksum.as_deref(), Some("deadbeef"));
        assert_ne!(
            entry.checksum, None,
            "checksum must round-trip, not be None"
        );
    }

    #[test]
    fn test_lock_manager_round_trip_via_disk() {
        let temp = TempDir::new().unwrap();
        let aikit_dir = temp.path().join(".aikit");
        std::fs::create_dir_all(&aikit_dir).unwrap();

        {
            let mut manager = LockManager::new(&aikit_dir);
            manager
                .lock_package_with_integrity(
                    &installed("demo", "2.0.0", "owner/demo"),
                    Some("sha-1".to_string()),
                    Some("checksum-1".to_string()),
                )
                .unwrap();
        }

        // Re-open a fresh manager over the same directory — must read back
        // what was written.
        let manager2 = LockManager::new(&aikit_dir);
        assert!(manager2.is_locked("demo"));
        assert_eq!(manager2.get_locked_version("demo"), Some("2.0.0"));
        assert!(aikit_dir.join("packages.lock").exists());
    }

    #[test]
    fn test_lock_package_without_integrity_has_no_checksum() {
        // Local-folder installs: no archive, no checksum — but the entry
        // still round-trips.
        let temp = TempDir::new().unwrap();
        let lock_path = temp.path().join("packages.lock");

        let mut lock = PackageLock::new();
        lock.add_package_with_integrity(
            &installed("local-pkg", "0.1.0", "/some/local/path"),
            None,
            None,
        );
        lock.save_to_file(&lock_path).unwrap();

        let loaded = PackageLock::load_from_file(&lock_path).unwrap();
        let entry = loaded.packages.get("local-pkg").unwrap();
        assert_eq!(entry.checksum, None);
        assert_eq!(entry.commit_sha, None);
    }

    // -- Integrity verification (SEC-7) ------------------------------------

    #[test]
    fn test_verify_checksum_no_existing_entry_is_ok() {
        let lock = PackageLock::new();
        assert!(lock.verify_checksum("demo", "1.0.0", "whatever").is_ok());
    }

    #[test]
    fn test_verify_checksum_same_version_same_checksum_is_ok() {
        let mut lock = PackageLock::new();
        lock.add_package_with_integrity(
            &installed("demo", "1.0.0", "owner/demo"),
            Some("sha1".to_string()),
            Some("cksum-a".to_string()),
        );
        assert!(lock.verify_checksum("demo", "1.0.0", "cksum-a").is_ok());
    }

    #[test]
    fn test_verify_checksum_different_version_is_ok_even_if_checksum_differs() {
        // A normal update: new version, necessarily different content — not
        // a mismatch.
        let mut lock = PackageLock::new();
        lock.add_package_with_integrity(
            &installed("demo", "1.0.0", "owner/demo"),
            Some("sha1".to_string()),
            Some("cksum-a".to_string()),
        );
        assert!(lock.verify_checksum("demo", "2.0.0", "cksum-b").is_ok());
    }

    #[test]
    fn test_verify_checksum_mismatch_is_rejected() {
        let mut lock = PackageLock::new();
        lock.add_package_with_integrity(
            &installed("demo", "1.0.0", "owner/demo"),
            Some("sha1".to_string()),
            Some("cksum-a".to_string()),
        );

        let err = lock
            .verify_checksum("demo", "1.0.0", "cksum-DIFFERENT")
            .expect_err("same version, different checksum must be rejected");

        assert!(err.to_string().contains("checksum mismatch"));
        match &err {
            IntegrityError::ChecksumMismatch {
                package,
                version,
                expected,
                actual,
            } => {
                assert_eq!(package, "demo");
                assert_eq!(version, "1.0.0");
                assert_eq!(expected, "cksum-a");
                assert_eq!(actual, "cksum-DIFFERENT");
            }
        }
    }

    #[test]
    fn test_lock_package_with_integrity_mismatch_does_not_mutate_lock_file() {
        let temp = TempDir::new().unwrap();
        let aikit_dir = temp.path().join(".aikit");
        std::fs::create_dir_all(&aikit_dir).unwrap();

        let mut manager = LockManager::new(&aikit_dir);
        manager
            .lock_package_with_integrity(
                &installed("demo", "1.0.0", "owner/demo"),
                Some("sha1".to_string()),
                Some("cksum-a".to_string()),
            )
            .unwrap();

        // Same version, tampered/moved-ref content -> different checksum.
        let result = manager.lock_package_with_integrity(
            &installed("demo", "1.0.0", "owner/demo"),
            Some("sha2".to_string()),
            Some("cksum-TAMPERED".to_string()),
        );
        assert!(result.is_err());

        // The on-disk lock file must still reflect the original, trusted
        // checksum — the rejected write must not have landed.
        let reloaded = LockManager::new(&aikit_dir);
        assert_eq!(
            reloaded
                .lock
                .packages
                .get("demo")
                .unwrap()
                .checksum
                .as_deref(),
            Some("cksum-a")
        );
    }

    #[test]
    fn test_is_locked_and_get_locked_version() {
        let mut lock = PackageLock::new();
        assert!(!lock.is_locked("demo"));
        lock.add_package_with_integrity(&installed("demo", "1.2.3", "owner/demo"), None, None);
        assert!(lock.is_locked("demo"));
        assert_eq!(lock.get_locked_version("demo"), Some("1.2.3"));
    }

    #[test]
    fn test_remove_package_from_lock() {
        let mut lock = PackageLock::new();
        lock.add_package_with_integrity(&installed("demo", "1.0.0", "owner/demo"), None, None);
        assert!(lock.is_locked("demo"));
        let removed = lock.remove_package("demo");
        assert!(removed.is_some());
        assert!(!lock.is_locked("demo"));
    }

    #[test]
    fn test_load_from_file_missing_returns_empty_lock() {
        let temp = TempDir::new().unwrap();
        let lock_path = temp.path().join("does-not-exist.lock");
        let lock = PackageLock::load_from_file(&lock_path).unwrap();
        assert!(lock.packages.is_empty());
    }
}
