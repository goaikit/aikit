//! Filesystem operations for .aikit/ directory management
//!
//! This module handles creating, managing, and cleaning up the .aikit/
//! directory structure for installed packages.

use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// .aikit/ directory manager
#[allow(dead_code)]
pub struct AikDirectory {
    base_path: PathBuf,
}

#[allow(dead_code)]
impl AikDirectory {
    /// Create a new .aikit/ directory manager by finding .aikit in the directory hierarchy
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Find .aikit directory by searching up the directory hierarchy
    pub fn find() -> Result<Self, Box<dyn std::error::Error>> {
        let mut current_dir = std::env::current_dir()?;

        loop {
            let aikit_path = current_dir.join(".aikit");
            if aikit_path.exists() && aikit_path.is_dir() {
                return Ok(Self::new(aikit_path));
            }

            // Move up one directory
            if let Some(parent) = current_dir.parent() {
                current_dir = parent.to_path_buf();
            } else {
                // Reached root directory, .aikit not found
                return Err("Could not find .aikit directory in current directory or any parent directory".into());
            }
        }
    }

    /// Create .aikit/ directory structure
    pub fn create(&self) -> Result<(), Box<dyn std::error::Error>> {
        fs::create_dir_all(&self.base_path)?;
        fs::create_dir_all(self.packages_path())?;
        fs::create_dir_all(self.cache_path())?;
        Ok(())
    }

    /// Check if .aikit/ directory exists
    pub fn exists(&self) -> bool {
        self.base_path.exists() && self.base_path.is_dir()
    }

    /// Get the project root directory (parent of .aikit)
    pub fn project_root(&self) -> PathBuf {
        self.base_path.parent().unwrap_or(&self.base_path).to_path_buf()
    }

    /// Get packages installation directory
    pub fn packages_path(&self) -> PathBuf {
        self.base_path.join("packages")
    }

    /// Get cache directory
    pub fn cache_path(&self) -> PathBuf {
        self.base_path.join("cache")
    }

    /// Get registry file path
    pub fn registry_path(&self) -> PathBuf {
        self.base_path.join("registry.toml")
    }

    /// Get installed packages file path
    pub fn installed_path(&self) -> PathBuf {
        self.base_path.join("installed.toml")
    }

    /// Install package files to .aikit/packages/
    pub fn install_package(
        &self,
        package_name: &str,
        version: &str,
        source_dir: &Path,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let install_path = self
            .packages_path()
            .join(format!("{}-{}", package_name, version));

        // Create package directory
        fs::create_dir_all(&install_path)?;

        // Copy package files
        self.copy_directory(source_dir, &install_path)?;

        Ok(install_path)
    }

    /// Remove installed package
    pub fn remove_package(
        &self,
        package_name: &str,
        version: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let package_path = self
            .packages_path()
            .join(format!("{}-{}", package_name, version));

        if package_path.exists() {
            fs::remove_dir_all(package_path)?;
        }

        // Clean up empty directories
        self.cleanup_empty_dirs()?;

        Ok(())
    }

    /// List installed packages
    pub fn list_packages(&self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let packages_dir = self.packages_path();
        if !packages_dir.exists() {
            return Ok(Vec::new());
        }

        let mut packages = Vec::new();

        for entry in fs::read_dir(packages_dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    packages.push(name.to_string());
                }
            }
        }

        Ok(packages)
    }

    /// Get package installation path
    pub fn get_package_path(&self, package_name: &str, version: &str) -> PathBuf {
        self.packages_path()
            .join(format!("{}-{}", package_name, version))
    }

    /// Check if package is installed
    pub fn is_package_installed(&self, package_name: &str, version: &str) -> bool {
        self.get_package_path(package_name, version).exists()
    }

    /// Clean up empty directories
    pub fn cleanup_empty_dirs(&self) -> Result<(), Box<dyn std::error::Error>> {
        fn remove_empty_dirs(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
            if !dir.exists() || !dir.is_dir() {
                return Ok(());
            }

            let entries = fs::read_dir(dir)?;

            for entry in entries {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    remove_empty_dirs(&path)?;
                    // Check again after recursive cleanup
                    if path.exists() && fs::read_dir(&path)?.next().is_some() {
                        // Directory still has content, keep it
                    } else if path.exists() {
                        fs::remove_dir(path)?;
                    }
                } else {
                    // File exists, directory has content
                }
            }

            Ok(())
        }

        remove_empty_dirs(&self.packages_path())
    }

    /// Copy directory recursively
    fn copy_directory(&self, from: &Path, to: &Path) -> Result<(), Box<dyn std::error::Error>> {
        for entry in WalkDir::new(from).into_iter().filter_map(|e| e.ok()) {
            let source_path = entry.path();
            let relative_path = source_path.strip_prefix(from)?;
            let dest_path = to.join(relative_path);

            if source_path.is_dir() {
                fs::create_dir_all(&dest_path)?;
            } else {
                if let Some(parent) = dest_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(source_path, dest_path)?;
            }
        }

        Ok(())
    }
}

/// .gitignore management for .aikit/ directory
#[allow(dead_code)]
pub struct GitIgnoreManager {
    gitignore_path: PathBuf,
}

#[allow(dead_code)]
impl GitIgnoreManager {
    /// Create a new .gitignore manager
    pub fn new(project_root: &Path) -> Self {
        Self {
            gitignore_path: project_root.join(".gitignore"),
        }
    }

    /// Check if .aikit/ is already in .gitignore
    pub fn contains_aikit(&self) -> bool {
        if !self.gitignore_path.exists() {
            return false;
        }

        match fs::read_to_string(&self.gitignore_path) {
            Ok(content) => content.lines().any(|line| line.trim() == ".aikit/"),
            Err(_) => false,
        }
    }

    /// Add .aikit/ to .gitignore
    pub fn add_aikit(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.contains_aikit() {
            return Ok(());
        }

        let mut content = String::new();

        // Read existing .gitignore if it exists
        if self.gitignore_path.exists() {
            content = fs::read_to_string(&self.gitignore_path)?;
            content.push('\n');
        }

        // Add .aikit/ entry
        content.push_str("# AIKIT package directory\n.aikit/\n");

        fs::write(&self.gitignore_path, content)?;
        Ok(())
    }

    /// Prompt user for .gitignore modification (returns true if should proceed)
    pub fn prompt_user(&self) -> bool {
        if self.contains_aikit() {
            return true; // Already added, no need to prompt
        }

        println!("AIKIT packages will be installed to .aikit/ directory.");
        println!("Add .aikit/ to .gitignore? (y/N): ");

        // For now, assume yes in automated context
        // TODO: Implement proper user prompting when interactive
        true
    }
}
