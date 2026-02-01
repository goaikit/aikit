use anyhow::Result;
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_install_command_local_directory() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let package_dir = temp_dir.path().join("test-package");

        // Create package structure
        fs::create_dir_all(&package_dir).unwrap();
        fs::write(
            package_dir.join("aikit.toml"),
            r#"[package]
name = "test-package"
version = "1.0.0"
description = "Test package"
[commands]
help = { description = "Show help", template = "templates/help.md" }
"#,
        )
        .unwrap();

        let args = super::InstallArgs {
            source: package_dir.to_string_lossy().to_string(),
            install_version: None,
            token: None,
            force: true,
            yes: true,
            ai: Some("copilot".to_string()),
        };

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute_install(args));

        assert!(result.is_ok());
    }

    #[test]
    fn test_install_command_github_url() {
        let args = super::InstallArgs {
            source: "github.com/aroff/spec-kit".to_string(),
            install_version: Some("1.0.0".to_string()),
            token: None,
            force: true,
            yes: true,
            ai: Some("copilot".to_string()),
        };

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute_install(args));
        assert!(result.is_ok());
    }

    #[test]
    fn test_install_command_invalid_source() {
        let args = super::InstallArgs {
            source: "invalid-source".to_string(),
            install_version: None,
            token: None,
            force: false,
            yes: false,
            ai: None,
        };

        let result = args.detect_source_type();
        assert!(result.is_err());
    }

    #[test]
    fn test_install_command_no_ai_agent() {
        let args = super::InstallArgs {
            source: "github.com/aroff/spec-kit".to_string(),
            install_version: None,
            token: None,
            force: true,
            yes: true,
            ai: None,
        };

        // Should fail without AI agent specified
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute_install(args));
        assert!(result.is_err());
    }

    #[test]
    fn test_install_command_force_reinstall() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let package_dir = temp_dir.path().join("test-package");

        // Create package structure
        fs::create_dir_all(&package_dir).unwrap();
        fs::write(
            package_dir.join("aikit.toml"),
            r#"[package]
name = "test-package"
version = "1.0.0"
description = "Test package"
"#,
        )
        .unwrap();

        let args = super::InstallArgs {
            source: package_dir.to_string_lossy().to_string(),
            install_version: None,
            token: None,
            force: true,
            yes: true,
            ai: Some("copilot".to_string()),
        };

        // Should succeed even if already exists
        let result1 = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute_install(args));
        assert!(result1.is_ok());

        let result2 = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute_install(args));
        assert!(result2.is_ok());
    }
}
