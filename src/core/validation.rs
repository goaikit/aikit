//! Input validation utilities for the AIKIT CLI
//!
//! This module provides comprehensive validation functions for user inputs,
//! path sanitization, and data validation to prevent security issues and
//! improve user experience.

use super::super::error::AikError;
use regex::Regex;
use std::path::{Path, PathBuf};

/// Validate package name according to naming conventions
pub fn validate_package_name(name: &str) -> Result<(), AikError> {
    // Check length
    if name.is_empty() {
        return Err(AikError::InvalidSource(
            "Package name cannot be empty".to_string(),
        ));
    }

    if name.len() > 50 {
        return Err(AikError::InvalidSource(
            "Package name too long (max 50 characters)".to_string(),
        ));
    }

    // Check characters (alphanumeric, hyphens, underscores only)
    let valid_chars = Regex::new(r"^[a-zA-Z0-9_-]+$")
        .map_err(|_| AikError::Generic("Invalid regex".to_string()))?;
    if !valid_chars.is_match(name) {
        return Err(AikError::InvalidSource(
            "Package name can only contain letters, numbers, hyphens, and underscores".to_string(),
        ));
    }

    // Check reserved names
    let reserved = ["aikit", "node_modules", ".git", ".aikit"];
    if reserved.contains(&name.to_lowercase().as_str()) {
        return Err(AikError::InvalidSource(format!(
            "'{}' is a reserved name",
            name
        )));
    }

    Ok(())
}

/// Validate semantic version format
pub fn validate_version_format(version: &str) -> Result<(), AikError> {
    // Basic semantic versioning: major.minor.patch
    let semver_regex = Regex::new(r"^v?\d+\.\d+\.\d+$")
        .map_err(|_| AikError::Generic("Invalid regex".to_string()))?;

    if !semver_regex.is_match(version) {
        return Err(AikError::InvalidVersion(
            "Version must be in semantic format (e.g., 1.0.0 or v1.0.0)".to_string(),
        ));
    }

    Ok(())
}

/// Sanitize and validate file path to prevent directory traversal
pub fn sanitize_path(path: &str) -> Result<PathBuf, AikError> {
    let path_buf = PathBuf::from(path);

    // Explicitly check for ".." components
    // This prevents directory traversal attempts
    if path_buf
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        return Err(AikError::InvalidSource(
            "Path cannot contain '..' components".to_string(),
        ));
    }

    // Get and canonicalize current directory first for consistent comparison
    let current_dir = std::env::current_dir()?;
    let canonical_current_dir = current_dir.canonicalize()?;

    // Normalize the path without requiring it to exist
    // For relative paths, join with canonicalized current directory
    // For absolute paths, use as-is
    let normalized = if path_buf.is_relative() {
        canonical_current_dir.join(&path_buf)
    } else {
        path_buf.clone()
    };

    // Try to canonicalize if the path exists, otherwise use the normalized path
    let canonical = if normalized.exists() {
        normalized
            .canonicalize()
            .map_err(|e| AikError::InvalidSource(format!("Invalid path '{}': {}", path, e)))?
    } else {
        // Path doesn't exist yet, use normalized version
        normalized
    };

    // Prevent absolute paths that go outside current working directory
    if !canonical.starts_with(&canonical_current_dir) {
        return Err(AikError::InvalidSource(
            "Path must be within current working directory".to_string(),
        ));
    }

    Ok(canonical)
}

/// Validate GitHub URL format
pub fn validate_github_url(url: &str) -> Result<(), AikError> {
    let github_regex = Regex::new(r"^https?://github\.com/[a-zA-Z0-9_-]+/[a-zA-Z0-9_.-]+$")
        .map_err(|_| AikError::Generic("Invalid regex".to_string()))?;

    if !github_regex.is_match(url) {
        return Err(AikError::InvalidGitHubUrl(
            "Must be a valid GitHub repository URL (https://github.com/owner/repo)".to_string(),
        ));
    }

    Ok(())
}

/// Validate local path for package installation
pub fn validate_local_path(path: &Path) -> Result<(), AikError> {
    // Check if path exists
    if !path.exists() {
        return Err(AikError::InvalidSource(format!(
            "Path '{}' does not exist",
            path.display()
        )));
    }

    // Check if it's a directory
    if !path.is_dir() {
        return Err(AikError::InvalidSource(format!(
            "Path '{}' is not a directory",
            path.display()
        )));
    }

    // Check if it contains aikit.toml
    let aikit_toml = path.join("aikit.toml");
    if !aikit_toml.exists() {
        return Err(AikError::InvalidSource(format!(
            "Directory '{}' does not contain aikit.toml",
            path.display()
        )));
    }

    // Check if aikit.toml is readable
    if !aikit_toml.is_file() {
        return Err(AikError::InvalidSource(format!(
            "'{}' is not a regular file",
            aikit_toml.display()
        )));
    }

    Ok(())
}

/// Validate GitHub repository name format
pub fn validate_github_repo_name(name: &str) -> Result<(), AikError> {
    if name.is_empty() {
        return Err(AikError::InvalidGitHubUrl(
            "Repository name cannot be empty".to_string(),
        ));
    }

    if name.len() > 100 {
        return Err(AikError::InvalidGitHubUrl(
            "Repository name too long".to_string(),
        ));
    }

    // GitHub repo names can contain letters, numbers, hyphens, underscores, and periods
    let valid_repo = Regex::new(r"^[a-zA-Z0-9_.-]+$")
        .map_err(|_| AikError::Generic("Invalid regex".to_string()))?;
    if !valid_repo.is_match(name) {
        return Err(AikError::InvalidGitHubUrl(
            "Repository name can only contain letters, numbers, hyphens, underscores, and periods"
                .to_string(),
        ));
    }

    Ok(())
}

/// Validate GitHub owner/organization name format
pub fn validate_github_owner_name(name: &str) -> Result<(), AikError> {
    if name.is_empty() {
        return Err(AikError::InvalidGitHubUrl(
            "Owner name cannot be empty".to_string(),
        ));
    }

    if name.len() > 39 {
        // GitHub's limit
        return Err(AikError::InvalidGitHubUrl(
            "Owner name too long".to_string(),
        ));
    }

    // GitHub usernames/organization names: letters, numbers, hyphens only, no consecutive hyphens
    let valid_owner = Regex::new(r"^[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?$")
        .map_err(|_| AikError::Generic("Invalid regex".to_string()))?;

    if !valid_owner.is_match(name) {
        return Err(AikError::InvalidGitHubUrl(
            "Owner name can only contain letters, numbers, and hyphens (no consecutive hyphens)"
                .to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_package_name_valid() {
        assert!(validate_package_name("my-package").is_ok());
        assert!(validate_package_name("package123").is_ok());
        assert!(validate_package_name("my_package").is_ok());
        assert!(validate_package_name("package-name").is_ok());
    }

    #[test]
    fn test_validate_package_name_invalid() {
        assert!(validate_package_name("").is_err());
        assert!(validate_package_name("my package").is_err());
        assert!(validate_package_name("package@name").is_err());
        assert!(validate_package_name("aikit").is_err()); // reserved
        assert!(validate_package_name(&"a".repeat(51)).is_err()); // too long
    }

    #[test]
    fn test_validate_version_format_valid() {
        assert!(validate_version_format("1.0.0").is_ok());
        assert!(validate_version_format("v1.0.0").is_ok());
        assert!(validate_version_format("0.1.0").is_ok());
        assert!(validate_version_format("10.5.123").is_ok());
    }

    #[test]
    fn test_validate_version_format_invalid() {
        assert!(validate_version_format("1.0").is_err());
        assert!(validate_version_format("1.0.0.0").is_err());
        assert!(validate_version_format("v1.0").is_err());
        assert!(validate_version_format("1.0.a").is_err());
    }

    #[test]
    fn test_validate_github_url_valid() {
        assert!(validate_github_url("https://github.com/owner/repo").is_ok());
    }

    #[test]
    fn test_validate_github_url_invalid() {
        assert!(validate_github_url("https://gitlab.com/owner/repo").is_err());
        assert!(validate_github_url("github.com/owner/repo").is_err());
        assert!(validate_github_url("not-a-url").is_err());
    }

    #[test]
    fn test_sanitize_path_basic() {
        use tempfile::tempdir;
        let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");

        // Create temp directory and canonicalize the expected path before changing CWD
        let temp_dir_obj = tempdir().expect("Failed to create main temp dir");
        let temp_dir_path = temp_dir_obj.path();
        let temp_dir_canonical = temp_dir_path
            .canonicalize()
            .expect("Failed to canonicalize main temp dir");

        // Change to temp directory
        std::env::set_current_dir(temp_dir_path).expect("Failed to set CWD to main temp dir");

        // Test with a relative path that should work and resolve to current directory
        let result = sanitize_path(".");
        assert!(
            result.is_ok(),
            "Sanitizing '.' should be OK: {:?}",
            result.err()
        );
        let path_buf = result.expect("Should get a valid PathBuf for '.'");
        assert!(
            path_buf.exists(),
            "Path '.' should exist after canonicalization"
        );
        assert_eq!(
            path_buf.canonicalize().expect("Failed to canonicalize '.'"),
            temp_dir_canonical
        );

        // Test valid relative paths
        assert!(
            sanitize_path("foo/bar").is_ok(),
            "Sanitizing 'foo/bar' should be OK"
        );
        assert!(
            sanitize_path("foo/./bar").is_ok(),
            "Sanitizing 'foo/./bar' should be OK"
        );
        assert!(
            sanitize_path("foo/bar/").is_ok(),
            "Sanitizing 'foo/bar/' should be OK"
        );

        // Test invalid paths (directory traversal attempts)
        assert!(
            sanitize_path("foo/../bar").is_err(),
            "Sanitizing 'foo/../bar' should be an error"
        );
        assert!(
            sanitize_path("../bar").is_err(),
            "Sanitizing '../bar' should be an error"
        );

        // For absolute path outside current dir, create a temporary directory outside current tempdir
        let outside_temp_dir_obj = tempdir().expect("Failed to create outside temp dir");
        let outside_temp_dir_path = outside_temp_dir_obj.path();
        let outside_path = outside_temp_dir_path.join("file.txt");
        std::fs::write(&outside_path, "content").expect("Failed to write to outside_path");

        let result_outside =
            sanitize_path(outside_path.to_str().expect("Outside path not valid UTF-8"));
        assert!(
            result_outside.is_err(),
            "Sanitizing outside_path should be an error: {:?}",
            result_outside
        );

        // Restore CWD BEFORE temp directories are dropped
        std::env::set_current_dir(&orig_cwd).expect("Failed to restore original CWD");

        // Temp directories will be dropped here automatically
    }
}
