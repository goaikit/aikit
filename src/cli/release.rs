//! `aikit release` command implementation
//!
//! This module implements the GitHub release creation command.

use anyhow::{Context, Result};
use clap::Args;
use std::path::PathBuf;
use std::process::Command;

/// Create GitHub release with package files
#[derive(Args, Debug)]
pub struct ReleaseArgs {
    /// Version string with 'v' prefix (e.g., v1.0.0)
    #[arg(value_name = "VERSION")]
    pub version: String,

    /// Path to release notes file
    #[arg(long, value_name = "FILE", default_value = "release_notes.md")]
    pub notes_file: String,

    /// GitHub token for API requests
    #[arg(long, value_name = "TOKEN")]
    pub github_token: Option<String>,
}

/// Execute the release command
pub async fn execute(args: ReleaseArgs) -> Result<()> {
    // Validate version format
    validate_version_format(&args.version)?;

    // Find package files in .genreleases/
    let package_dir = PathBuf::from(".genreleases");
    if !package_dir.exists() {
        return Err(anyhow::anyhow!(
            "Package directory '.genreleases/' not found. Run 'aikit package {}' first.",
            args.version
        ));
    }

    let package_files: Vec<PathBuf> = std::fs::read_dir(&package_dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_file() && path.extension()? == "zip" {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    if package_files.is_empty() {
        return Err(anyhow::anyhow!(
            "No package files found in '.genreleases/'. Run 'aikit package {}' first.",
            args.version
        ));
    }

    println!("Found {} package file(s)", package_files.len());

    // Check for GitHub CLI
    let gh_available = which::which("gh").is_ok();
    if !gh_available && args.github_token.is_none() {
        return Err(anyhow::anyhow!(
            "GitHub CLI ('gh') not found. Install it or provide --github-token"
        ));
    }

    // Format release title
    let version_without_v = args
        .version
        .strip_prefix('v')
        .ok_or_else(|| anyhow::anyhow!("Version must start with 'v'"))?;
    let release_title = format!("Spec Kit Templates - {}", version_without_v);

    // Read release notes if file exists
    let notes_content = if PathBuf::from(&args.notes_file).exists() {
        Some(std::fs::read_to_string(&args.notes_file).context(format!(
            "Failed to read release notes from {}",
            args.notes_file
        ))?)
    } else {
        None
    };

    // Create release using GitHub CLI
    if gh_available {
        create_release_with_gh(
            &args.version,
            &release_title,
            &package_files,
            notes_content.as_deref(),
        )?;
    } else {
        // TODO: Implement GitHub API-based release creation if needed
        return Err(anyhow::anyhow!(
            "GitHub CLI required for release creation. Install 'gh' or use GitHub API directly."
        ));
    }

    println!("Release '{}' created successfully", args.version);
    Ok(())
}

/// Validate version format (vX.Y.Z)
fn validate_version_format(version: &str) -> Result<()> {
    if !version.starts_with('v') {
        return Err(anyhow::anyhow!("Version '{}' must start with 'v'", version));
    }

    let version_part = &version[1..];
    let parts: Vec<&str> = version_part.split('.').collect();
    if parts.len() != 3 {
        return Err(anyhow::anyhow!(
            "Version '{}' must match pattern vX.Y.Z",
            version
        ));
    }

    for part in parts {
        if part.parse::<u32>().is_err() {
            return Err(anyhow::anyhow!(
                "Version '{}' contains invalid numeric parts",
                version
            ));
        }
    }

    Ok(())
}

/// Create GitHub release using `gh release create`
fn create_release_with_gh(
    tag: &str,
    title: &str,
    assets: &[PathBuf],
    notes: Option<&str>,
) -> Result<()> {
    let mut cmd = Command::new("gh");
    cmd.arg("release");
    cmd.arg("create");
    cmd.arg(tag);
    cmd.arg("--title");
    cmd.arg(title);

    if let Some(notes) = notes {
        // Write notes to temp file for gh
        let temp_notes = tempfile::NamedTempFile::new()?;
        std::fs::write(temp_notes.path(), notes)?;
        cmd.arg("--notes-file");
        cmd.arg(temp_notes.path());
    }

    // Add all asset files
    for asset in assets {
        cmd.arg(asset);
    }

    let output = cmd
        .output()
        .context("Failed to execute 'gh release create'")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Check for "release already exists" error
        if stderr.contains("already exists") || stderr.contains("Release already exists") {
            return Err(anyhow::anyhow!(
                "Release '{}' already exists on GitHub",
                tag
            ));
        }

        return Err(anyhow::anyhow!("Failed to create release: {}", stderr));
    }

    Ok(())
}
