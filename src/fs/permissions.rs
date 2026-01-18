//! File permission handling for Unix systems
//!
//! This module handles setting execute permissions on script files
//! for Unix-like systems.

use anyhow::Result;
use std::path::Path;

/// Set execute permissions on a file (Unix only)
///
/// This function only works on Unix-like systems. On Windows, it's a no-op.
#[cfg(unix)]
pub fn set_execute_permission<P: AsRef<Path>>(path: P) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let path = path.as_ref();
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

/// Set execute permissions on a file (Windows - no-op)
#[cfg(not(unix))]
pub fn set_execute_permission<P: AsRef<Path>>(_path: P) -> Result<()> {
    // Windows doesn't have Unix-style execute permissions
    Ok(())
}

/// Check if a file has a shebang (#!/bin/bash, etc.)
pub fn has_shebang<P: AsRef<Path>>(path: P) -> bool {
    let path = path.as_ref();
    if let Ok(content) = std::fs::read_to_string(path) {
        content.starts_with("#!/")
    } else {
        false
    }
}

/// Set execute permissions on .sh files with shebangs
pub fn set_script_permissions<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();

    // Only process .sh files
    if path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        != Some("sh".to_string())
    {
        return Ok(());
    }

    // Only set permissions if file has shebang
    if has_shebang(path) {
        set_execute_permission(path)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_has_shebang() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.sh");

        // File with shebang
        fs::write(&file_path, "#!/bin/bash\necho hello").unwrap();
        assert!(has_shebang(&file_path));

        // File without shebang
        fs::write(&file_path, "echo hello").unwrap();
        assert!(!has_shebang(&file_path));
    }
}
