use anyhow::Result;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_build_command_basic() {
        // Create a test aikit.toml file
        let temp_dir = tempfile::TempDir::new().unwrap();
        let toml_path = temp_dir.path().join("aikit.toml");
        let toml_content = r#"[package]
name = "test-package"
version = "1.0.0"
description = "Test package"
[commands]
help = { description = "Show help", template = "templates/help.md" }
"#;

        fs::write(&toml_path, toml_content).unwrap();

        let args = super::PackageBuildArgs {
            output: "dist".to_string(),
            agents: None,
            include_sources: false,
        };

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute(args));

        assert!(result.is_ok());

        // Check if dist directory was created
        let dist_dir = temp_dir.path().join("dist");
        assert!(dist_dir.exists());
    }

    #[test]
    fn test_package_build_command_with_agents() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let toml_path = temp_dir.path().join("aikit.toml");
        let toml_content = r#"[package]
name = "test-package"
version = "1.0.0"
description = "Test package"
[commands]
help = { description = "Show help", template = "templates/help.md" }
"#;

        fs::write(&toml_path, toml_content).unwrap();

        let args = super::PackageBuildArgs {
            output: "dist".to_string(),
            agents: Some("claude,copilot".to_string()),
            include_sources: false,
        };

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute(args));

        assert!(result.is_ok());
    }

    #[test]
    fn test_package_build_command_no_toml() {
        let args = super::PackageBuildArgs {
            output: "dist".to_string(),
            agents: None,
            include_sources: false,
        };

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute(args));
        assert!(result.is_err());
    }
}
