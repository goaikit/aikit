/// Resolves logical command names to platform-specific executable paths.
///
/// On Windows: performs PATH + PATHEXT resolution with AIKIT_CURSOR_AGENT override.
/// On other platforms: returns input unchanged (passthrough).
///
/// This module centralizes Windows-specific command resolution to handle cases where
/// programs like Cursor's agent ship as `agent.cmd` rather than `agent.exe`, which
/// Rust's `Command::new` cannot find when given the bare name "agent".
use std::ffi::OsString;

/// Resolves a logical command name to an OS-specific executable path.
///
/// On Windows, performs PATH + PATHEXT resolution so that bare names like "agent"
/// correctly resolve to "agent.cmd" when that is the form available on PATH.
/// The `AIKIT_CURSOR_AGENT` environment variable can override resolution for the
/// "agent" command specifically.
///
/// On non-Windows platforms, returns the input unchanged (passthrough).
pub(crate) fn resolve_command(logical_name: &str) -> OsString {
    resolve_command_impl(logical_name)
}

#[cfg(windows)]
fn resolve_command_impl(logical_name: &str) -> OsString {
    use std::path::Path;

    // AIKIT_CURSOR_AGENT override for "agent" commands
    if logical_name == "agent" {
        if let Some(override_path) = std::env::var_os("AIKIT_CURSOR_AGENT") {
            if Path::new(&override_path).exists() {
                return override_path;
            }
            // Invalid path: fall through to PATH resolution
        }
    }

    // If the name already contains path separators or exists as a file, return unchanged
    let path = Path::new(logical_name);
    if path.components().count() > 1 || path.exists() {
        return OsString::from(logical_name);
    }

    // Get PATH directories
    let path_var = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return OsString::from(logical_name),
    };

    // Get PATHEXT extensions (default to Windows standard set)
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());

    let extensions: Vec<&str> = pathext.split(';').collect();

    // Iterate PATH directories × PATHEXT extensions
    for dir in std::env::split_paths(&path_var) {
        for ext in &extensions {
            let candidate = dir.join(format!("{}{}", logical_name, ext));
            if std::fs::metadata(&candidate).is_ok() {
                return candidate.into_os_string();
            }
        }
    }

    // No match found: return original to preserve existing error behavior
    OsString::from(logical_name)
}

#[cfg(not(windows))]
fn resolve_command_impl(logical_name: &str) -> OsString {
    OsString::from(logical_name)
}

#[cfg(windows)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize tests that manipulate PATH/env vars
    static PATH_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_resolve_nonexistent_returns_original() {
        let _guard = PATH_LOCK.lock().unwrap();
        let result = resolve_command("nonexistent_program_xyz_12345");
        assert_eq!(result, OsString::from("nonexistent_program_xyz_12345"));
    }

    #[test]
    fn test_resolve_agent_with_valid_aikit_cursor_agent_override() {
        let _guard = PATH_LOCK.lock().unwrap();

        // Create a temporary file to simulate custom agent
        let temp_dir = std::env::temp_dir();
        let agent_path = temp_dir.join("custom_agent.cmd");
        std::fs::write(&agent_path, "@echo off").unwrap();

        std::env::set_var("AIKIT_CURSOR_AGENT", &agent_path);
        let result = resolve_command("agent");
        std::env::remove_var("AIKIT_CURSOR_AGENT");
        std::fs::remove_file(&agent_path).ok();

        assert_eq!(result, agent_path.as_os_str());
    }

    #[test]
    fn test_resolve_agent_with_invalid_aikit_cursor_agent_falls_back() {
        let _guard = PATH_LOCK.lock().unwrap();

        std::env::set_var("AIKIT_CURSOR_AGENT", "/nonexistent/path/agent.cmd");
        // Should not panic; falls back to PATH resolution and returns original if not found
        let result = resolve_command("agent");
        std::env::remove_var("AIKIT_CURSOR_AGENT");

        // Since "agent" likely doesn't exist on CI PATH, we get the original name back
        // The important thing is it doesn't return the invalid override path
        let result_str = result.to_string_lossy();
        assert!(!result_str.contains("/nonexistent/path/agent.cmd"));
    }

    #[test]
    fn test_resolve_cmd_file_in_temp_path() {
        let _guard = PATH_LOCK.lock().unwrap();

        // Create a temporary directory with a .cmd file
        let temp_dir = std::env::temp_dir().join("aikit_test_resolve");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let agent_cmd = temp_dir.join("testagent.cmd");
        std::fs::write(&agent_cmd, "@echo off").unwrap();

        // Prepend temp_dir to PATH
        let original_path = std::env::var_os("PATH").unwrap_or_default();
        let mut new_path = std::ffi::OsString::new();
        new_path.push(&temp_dir);
        new_path.push(";");
        new_path.push(&original_path);
        std::env::set_var("PATH", &new_path);

        let result = resolve_command("testagent");

        std::env::set_var("PATH", &original_path);
        std::fs::remove_file(&agent_cmd).ok();
        std::fs::remove_dir(&temp_dir).ok();

        let result_path = std::path::Path::new(&result);
        let file_name = result_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_lowercase();
        assert_eq!(file_name, "testagent.cmd");
    }
}

#[cfg(not(windows))]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unix_passthrough_agent() {
        let result = resolve_command("agent");
        assert_eq!(result, OsString::from("agent"));
    }

    #[test]
    fn test_unix_passthrough_nonexistent() {
        let result = resolve_command("nonexistent_xyz");
        assert_eq!(result, OsString::from("nonexistent_xyz"));
    }

    #[test]
    fn test_unix_passthrough_arbitrary_name() {
        let result = resolve_command("some_program");
        assert_eq!(result, OsString::from("some_program"));
    }
}
