//! Git repository operations
//!
//! This module handles Git repository initialization and detection.

use anyhow::{Context, Result};
use std::path::Path;

/// Initialize a Git repository at the given path
pub fn init_git_repo<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    git2::Repository::init(path)
        .with_context(|| format!("Failed to initialize git repository at {}", path.display()))?;
    Ok(())
}

/// Check if a Git repository exists at the given path
pub fn is_git_repo<P: AsRef<Path>>(path: P) -> bool {
    let path = path.as_ref();
    git2::Repository::open(path).is_ok()
}

/// Create initial commit in Git repository
pub fn create_initial_commit<P: AsRef<Path>>(path: P) -> Result<()> {
    let path_ref = path.as_ref();
    let path_display = path_ref.display().to_string();
    let repo = git2::Repository::open(path_ref)
        .with_context(|| format!("Failed to open git repository at {}", path_display))?;

    let mut index = repo.index().context("Failed to get repository index")?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write().context("Failed to write index")?;

    let tree_id = index.write_tree().context("Failed to write tree")?;
    let tree = repo.find_tree(tree_id).context("Failed to find tree")?;

    let sig =
        git2::Signature::now("AIKIT", "aikit@example.com").context("Failed to create signature")?;

    repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
        .context("Failed to create commit")?;

    Ok(())
}

/// Validate branch name against GitHub's constraints
///
/// GitHub branch names must:
/// - Not be longer than 244 bytes (UTF-8 encoded)
/// - Not contain certain special characters
/// - Not be empty
/// - Not start with a dot or slash
/// - Not contain consecutive dots
/// - Not end with a dot or slash
/// - Not contain sequences like `..`, `@{`, `\`, or spaces
pub fn validate_branch_name(branch_name: &str) -> Result<(), String> {
    if branch_name.is_empty() {
        return Err("Branch name cannot be empty".to_string());
    }

    // Check length (244 bytes in UTF-8)
    if branch_name.len() > 244 {
        return Err(format!(
            "Branch name '{}' exceeds GitHub's 244-byte limit ({} bytes)",
            branch_name,
            branch_name.len()
        ));
    }

    // Check for invalid starting characters
    if branch_name.starts_with('.') || branch_name.starts_with('/') {
        return Err("Branch name cannot start with '.' or '/'".to_string());
    }

    // Check for invalid ending characters
    if branch_name.ends_with('.') || branch_name.ends_with('/') {
        return Err("Branch name cannot end with '.' or '/'".to_string());
    }

    // Check for invalid sequences
    if branch_name.contains("..") {
        return Err("Branch name cannot contain '..'".to_string());
    }

    if branch_name.contains("@{") {
        return Err("Branch name cannot contain '@{{'".to_string());
    }

    if branch_name.contains('\\') {
        return Err("Branch name cannot contain '\\'".to_string());
    }

    if branch_name.contains(' ') {
        return Err("Branch name cannot contain spaces".to_string());
    }

    // Check for consecutive dots (already covered by ".." but be explicit)
    if branch_name.contains("...") {
        return Err("Branch name cannot contain consecutive dots".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_is_git_repo() {
        let temp_dir = TempDir::new().unwrap();
        assert!(!is_git_repo(temp_dir.path()));

        init_git_repo(temp_dir.path()).unwrap();
        assert!(is_git_repo(temp_dir.path()));
    }

    #[test]
    fn test_validate_branch_name() {
        // Valid names
        assert!(validate_branch_name("main").is_ok());
        assert!(validate_branch_name("feature/123").is_ok());
        let max_length_name = "a".repeat(244);
        assert!(validate_branch_name(&max_length_name).is_ok()); // Max length

        // Invalid names
        assert!(validate_branch_name("").is_err());
        assert!(validate_branch_name(&"a".repeat(245)).is_err()); // Too long
        assert!(validate_branch_name(".hidden").is_err()); // Starts with dot
        assert!(validate_branch_name("/root").is_err()); // Starts with slash
        assert!(validate_branch_name("branch.").is_err()); // Ends with dot
        assert!(validate_branch_name("branch/").is_err()); // Ends with slash
        assert!(validate_branch_name("branch..name").is_err()); // Contains ..
        assert!(validate_branch_name("branch@{").is_err()); // Contains @{
        assert!(validate_branch_name("branch\\name").is_err()); // Contains backslash
        assert!(validate_branch_name("branch name").is_err()); // Contains space
    }
}
