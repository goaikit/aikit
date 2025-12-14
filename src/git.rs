//! Basic Git operations for AIKIT

use std::path::Path;
use std::process::Command;

/// Check if a directory is a git repository
pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

/// Initialize a new git repository
pub fn init_git_repo(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git").arg("init").current_dir(path).output()?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Failed to initialize git repo: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}

/// Create initial commit with basic files
pub fn create_initial_commit(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // Add all files
    Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()?;

    // Create initial commit
    let output = Command::new("git")
        .args(["commit", "-m", "Initial AIKIT project setup"])
        .current_dir(path)
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Failed to create initial commit: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}
