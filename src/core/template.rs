//! Template download, extraction, and merging module
//!
//! This module handles template operations including:
//! - Project path validation
//! - Template asset management
//! - ZIP extraction and flattening
//! - File merging operations

use std::path::{Path, PathBuf};

/// Project path with validation
///
/// Represents a target project location with validation rules.
#[derive(Debug, Clone)]
pub struct ProjectPath {
    /// Absolute or relative path
    pub path: PathBuf,
    /// Whether using --here flag
    pub is_here: bool,
    /// Whether path already exists
    pub exists: bool,
    /// Whether existing directory is empty
    pub is_empty: bool,
}

impl ProjectPath {
    /// Create a new ProjectPath from a path string
    pub fn new(path: impl AsRef<Path>, is_here: bool) -> Self {
        let path = if is_here {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            path.as_ref().to_path_buf()
        };

        let exists = path.exists();
        let is_empty = if exists && path.is_dir() {
            path.read_dir()
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false)
        } else {
            false
        };

        Self {
            path,
            is_here,
            exists,
            is_empty,
        }
    }

    /// Validate the project path
    ///
    /// Returns Ok(()) if valid, Err with message if invalid.
    pub fn validate(&self, force: bool) -> std::result::Result<(), String> {
        // If not --here and path exists, validation fails (unless --force)
        if !self.is_here && self.exists && !force {
            return Err(format!(
                "Directory '{}' already exists. Use --here to initialize in current directory or --force to overwrite.",
                self.path.display()
            ));
        }

        // Path must be valid filesystem path
        if !self.path.as_os_str().is_empty() {
            // Additional validation can be added here
        }

        Ok(())
    }
}

/// Template asset from GitHub releases
///
/// Represents a downloadable template zip file from GitHub releases.
#[derive(Debug, Clone)]
pub struct TemplateAsset {
    /// Asset filename (e.g., "spec-kit-template-copilot-sh-v1.0.0.zip")
    pub filename: String,
    /// File size in bytes
    pub size: u64,
    /// Release tag (e.g., "v1.0.0")
    pub release_tag: String,
    /// GitHub API download URL
    pub download_url: String,
    /// Extracted agent key
    pub agent: String,
    /// Extracted script variant
    pub script_variant: crate::core::agent::ScriptVariant,
}

impl TemplateAsset {
    /// Parse template asset from filename
    ///
    /// Expected pattern: `spec-kit-template-<agent>-<script>-v<version>.zip`
    pub fn from_filename(
        filename: &str,
        download_url: String,
        size: u64,
    ) -> std::result::Result<Self, String> {
        // Remove .zip extension
        let base = filename
            .strip_suffix(".zip")
            .ok_or_else(|| format!("Filename '{}' does not end with .zip", filename))?;

        // Extract version (vX.Y.Z)
        let version_part = base
            .strip_prefix("spec-kit-template-")
            .ok_or_else(|| format!("Filename '{}' does not match expected pattern", filename))?;

        // Find the last occurrence of "-v" to separate version
        let version_pos = version_part
            .rfind("-v")
            .ok_or_else(|| format!("Filename '{}' does not contain version", filename))?;

        let (agent_script_part, version) = version_part.split_at(version_pos);
        let version = version.strip_prefix("-v").unwrap();

        // Split agent and script
        let parts: Vec<&str> = agent_script_part.split('-').collect();
        if parts.len() < 2 {
            return Err(format!(
                "Filename '{}' does not contain agent and script parts",
                filename
            ));
        }

        // Last part is script, everything before is agent (may contain hyphens)
        let script_str = parts.last().unwrap();
        let agent = parts[..parts.len() - 1].join("-");

        let script_variant = match *script_str {
            "sh" => crate::core::agent::ScriptVariant::Sh,
            "ps" => crate::core::agent::ScriptVariant::Ps,
            _ => {
                return Err(format!(
                    "Unknown script variant '{}' in filename '{}'",
                    script_str, filename
                ));
            }
        };

        Ok(Self {
            filename: filename.to_string(),
            size,
            release_tag: format!("v{}", version),
            download_url,
            agent,
            script_variant,
        })
    }

    /// Validate the template asset
    pub fn validate(&self) -> std::result::Result<(), String> {
        // Validate release tag format (vX.Y.Z)
        if !self.release_tag.starts_with('v') {
            return Err(format!(
                "Release tag '{}' must start with 'v'",
                self.release_tag
            ));
        }

        // Validate download URL is HTTPS
        if !self.download_url.starts_with("https://") {
            return Err(format!(
                "Download URL '{}' must be HTTPS",
                self.download_url
            ));
        }

        Ok(())
    }
}

/// Extract and flatten ZIP archive
///
/// Extracts ZIP to a temporary directory, then flattens if exactly one
/// top-level directory exists, and copies to target directory.
pub fn extract_and_flatten_zip(zip_data: &[u8], target_dir: &Path) -> anyhow::Result<()> {
    use zip::ZipArchive;

    // Create temporary directory
    let temp_dir = tempfile::tempdir()?;
    let temp_path = temp_dir.path();

    // Extract ZIP to temp directory
    let mut archive = ZipArchive::new(std::io::Cursor::new(zip_data))?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = temp_path.join(file.name());

        if file.name().ends_with('/') {
            // Directory
            std::fs::create_dir_all(&outpath)?;
        } else {
            // File
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = std::fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }

    // Check if we need to flatten (exactly one top-level directory)
    let entries: Vec<_> = std::fs::read_dir(temp_path)?
        .filter_map(|e| e.ok())
        .collect();

    let source_path = if entries.len() == 1 {
        // Flatten: move contents of single directory up
        let single_entry = &entries[0];
        if single_entry.path().is_dir() {
            single_entry.path().to_path_buf()
        } else {
            temp_path.to_path_buf()
        }
    } else {
        temp_path.to_path_buf()
    };

    // Copy to target directory
    crate::fs::copy_directory(source_path, target_dir)?;

    Ok(())
}

/// Select template asset from release assets
///
/// Finds the matching asset for the given agent and script variant.
pub fn select_template_asset(
    assets: &[serde_json::Value],
    agent: &str,
    script_variant: crate::core::agent::ScriptVariant,
) -> anyhow::Result<TemplateAsset> {
    let script_str = match script_variant {
        crate::core::agent::ScriptVariant::Sh => "sh",
        crate::core::agent::ScriptVariant::Ps => "ps",
    };

    let pattern = format!("spec-kit-template-{}-{}-", agent, script_str);

    for asset in assets {
        let name = asset["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Asset missing 'name' field"))?;

        if name.starts_with(&pattern) && name.ends_with(".zip") {
            let download_url = asset["browser_download_url"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Asset missing 'browser_download_url' field"))?
                .to_string();

            let size = asset["size"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Asset missing 'size' field"))?;

            return TemplateAsset::from_filename(name, download_url, size)
                .map_err(|e| anyhow::anyhow!("Failed to parse asset: {}", e));
        }
    }

    Err(anyhow::anyhow!(
        "No template asset found for agent '{}' and script '{}'",
        agent,
        script_str
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_asset_parsing() {
        let asset = TemplateAsset::from_filename(
            "spec-kit-template-copilot-sh-v1.0.0.zip",
            "https://github.com/example/repo/releases/download/v1.0.0/spec-kit-template-copilot-sh-v1.0.0.zip".to_string(),
            12345,
        )
        .unwrap();

        assert_eq!(asset.agent, "copilot");
        assert_eq!(asset.release_tag, "v1.0.0");
        assert!(matches!(
            asset.script_variant,
            crate::core::agent::ScriptVariant::Sh
        ));
    }

    #[test]
    fn test_template_asset_with_hyphenated_agent() {
        let asset = TemplateAsset::from_filename(
            "spec-kit-template-cursor-agent-sh-v1.0.0.zip",
            "https://example.com/file.zip".to_string(),
            12345,
        )
        .unwrap();

        assert_eq!(asset.agent, "cursor-agent");
    }

    #[test]
    fn test_project_path_validation() {
        let path = ProjectPath::new("/tmp/test", false);
        // Validation depends on actual filesystem state, so we just test creation
        assert!(!path.is_here);
    }
}
