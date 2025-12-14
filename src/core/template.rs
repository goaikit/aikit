//! Template processing and extraction utilities

use std::fs;
use std::path::{Path, PathBuf};

/// Project path information
pub struct ProjectPath {
    pub path: PathBuf,
}

impl ProjectPath {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

/// Select template asset based on agent and script variant (stub)
pub fn select_template_asset(
    _assets: &[String],
    _agent_key: &str,
    _script_variant: &str,
) -> Option<String> {
    // TODO: Implement template asset selection
    None
}

/// Extract and flatten ZIP archive (stub)
pub fn extract_and_flatten_zip(
    _zip_data: &[u8],
    _dest_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // TODO: Implement ZIP extraction
    Ok(())
}

/// Copy directory recursively (moved from fs module to avoid conflicts)
pub fn copy_directory(from: &Path, to: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for entry in walkdir::WalkDir::new(from)
        .into_iter()
        .filter_map(|e| e.ok())
    {
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
