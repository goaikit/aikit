use anyhow::Result;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_publish_command_basic() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let toml_path = temp_dir.path().join("aikit.toml");
        let toml_content = r#"[package]
name = "test-package"
version = "1.0.0"
description = "Test package"
"#;

        fs::write(&toml_path, toml_content).unwrap();

        let args = super::PackagePublishArgs {
            repo: "test-owner/test-repo".to_string(),
            package: None,
            tag: Some("v1.0.0".to_string()),
            title: Some("Test Release".to_string()),
            notes: Some("Test notes".to_string()),
            token: None,
            no_release: false,
        };

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute(args));

        // This should fail because package file doesn't exist, but we're testing the flow
        assert!(result.is_err());
    }

    #[test]
    fn test_package_publish_command_invalid_repo() {
        let args = super::PackagePublishArgs {
            repo: "invalid".to_string(),
            package: None,
            tag: None,
            title: None,
            notes: None,
            token: None,
            no_release: false,
        };

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute(args));
        assert!(result.is_err());
    }

    #[test]
    fn test_package_publish_command_no_release() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let toml_path = temp_dir.path().join("aikit.toml");
        let toml_content = r#"[package]
name = "test-package"
version = "1.0.0"
"#;

        fs::write(&toml_path, toml_content).unwrap();

        let args = super::PackagePublishArgs {
            repo: "test-owner/test-repo".to_string(),
            package: None,
            tag: Some("v1.0.0".to_string()),
            title: Some("Test Release".to_string()),
            notes: Some("Test notes".to_string()),
            token: None,
            no_release: true,
        };

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute(args));

        // Should succeed without creating a release
        assert!(result.is_ok());
    }
}
