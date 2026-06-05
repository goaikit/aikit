//! Input validation utilities for the AIKIT CLI
//!
//! This module provides comprehensive validation functions for user inputs,

//! path sanitization, and data validation to prevent security issues and
//! improve user experience.

use super::super::error::AikError;
use regex::Regex;
use std::sync::OnceLock;

static RE_PACKAGE_NAME: OnceLock<Regex> = OnceLock::new();
static RE_SEMVER: OnceLock<Regex> = OnceLock::new();
static RE_REPO_NAME: OnceLock<Regex> = OnceLock::new();
static RE_OWNER_NAME: OnceLock<Regex> = OnceLock::new();

fn re_package_name() -> &'static Regex {
    RE_PACKAGE_NAME.get_or_init(|| Regex::new(r"^[a-zA-Z0-9_-]+$").unwrap())
}
fn re_semver() -> &'static Regex {
    RE_SEMVER.get_or_init(|| Regex::new(r"^v?\d+\.\d+\.\d+$").unwrap())
}
fn re_repo_name() -> &'static Regex {
    RE_REPO_NAME.get_or_init(|| Regex::new(r"^[a-zA-Z0-9_.-]+$").unwrap())
}
fn re_owner_name() -> &'static Regex {
    RE_OWNER_NAME.get_or_init(|| Regex::new(r"^[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?$").unwrap())
}

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
    if !re_package_name().is_match(name) {
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
    if !re_semver().is_match(version) {
        return Err(AikError::InvalidVersion(
            "Version must be in semantic format (e.g., 1.0.0 or v1.0.0)".to_string(),
        ));
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
    if !re_repo_name().is_match(name) {
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
    if !re_owner_name().is_match(name) {
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
}
