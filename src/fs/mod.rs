//! File system operations module
//!
//! This module contains file system utilities with cross-platform support.

pub mod merge;
pub mod permissions;

use std::path::{Path, PathBuf};

/// Copy directory recursively
pub fn copy_directory<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> anyhow::Result<()> {
    use walkdir::WalkDir;

    let from = from.as_ref();
    let to = to.as_ref();

    for entry in WalkDir::new(from) {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(from)?;
        let dest = to.join(relative);

        if path.is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(path, &dest)?;
        }
    }

    Ok(())
}

/// Create directory if it doesn't exist
pub fn ensure_directory<P: AsRef<Path>>(path: P) -> anyhow::Result<()> {
    let path = path.as_ref();
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

/// Normalize a path for cross-platform compatibility
///
/// This function:
/// - Resolves relative paths to absolute paths
/// - Normalizes path separators (though Rust's Path already handles this)
/// - Resolves `~` to home directory on Unix systems
/// - Handles `.` and `..` components
pub fn normalize_path<P: AsRef<Path>>(path: P) -> anyhow::Result<PathBuf> {
    let path = path.as_ref();

    // Handle tilde expansion on Unix systems
    let expanded = if cfg!(unix) {
        let path_str = path.to_string_lossy();
        if path_str == "~" {
            if let Ok(home) = std::env::var("HOME") {
                PathBuf::from(home)
            } else {
                path.to_path_buf()
            }
        } else if let Some(stripped) = path_str.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                PathBuf::from(home).join(stripped)
            } else {
                path.to_path_buf()
            }
        } else {
            path.to_path_buf()
        }
    } else {
        path.to_path_buf()
    };

    // Resolve to absolute path and normalize
    if expanded.is_absolute() {
        Ok(expanded.canonicalize().unwrap_or(expanded))
    } else {
        let current_dir = std::env::current_dir()?;
        let absolute = current_dir.join(&expanded);
        Ok(absolute.canonicalize().unwrap_or(absolute))
    }
}

/// Convert a path to a string with forward slashes (for display/cross-platform compatibility)
///
/// This is useful when displaying paths in output that should be consistent
/// across platforms, or when working with URLs or network paths.
pub fn path_to_string<P: AsRef<Path>>(path: P) -> String {
    let path = path.as_ref();
    #[cfg(windows)]
    {
        // On Windows, convert backslashes to forward slashes for display
        path.to_string_lossy().replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        path.to_string_lossy().to_string()
    }
}

/// Join path components in a cross-platform way
///
/// This is a convenience wrapper around PathBuf::join that ensures
/// consistent behavior across platforms.
pub fn join_paths<P: AsRef<Path>>(base: P, components: &[&str]) -> PathBuf {
    let mut path = base.as_ref().to_path_buf();
    for component in components {
        path.push(component);
    }
    path
}

/// Get the home directory in a cross-platform way
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                // Fallback to HOME if USERPROFILE is not set
                std::env::var("HOME").ok().map(PathBuf::from)
            })
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_paths() {
        let base = PathBuf::from("tmp");
        let result = join_paths(&base, &["a", "b", "c"]);
        assert!(result.ends_with("c"));
    }

    #[test]
    fn test_path_to_string() {
        let path = PathBuf::from("test/path");
        let s = path_to_string(&path);
        assert!(!s.contains('\\')); // Should not contain backslashes
    }

    #[test]
    fn test_home_dir() {
        // Just verify it doesn't panic
        let _ = home_dir();
    }
}
