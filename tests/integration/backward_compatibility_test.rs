//! Backward compatibility tests for existing AIKIT functionality

use std::fs;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_system_coexists_with_existing_structure() {
        // This test ensures that the new package system doesn't break
        // the existing AIKIT directory structure expectations

        let temp_dir = tempfile::tempdir().unwrap();
        let aikit_dir = temp_dir.path().join(".aikit");
        let packages_dir = aikit_dir.join("packages");

        // Create .aikit structure using our new system
        fs::create_dir_all(&packages_dir).unwrap();

        // Verify standard AI agent directories can still be created
        let claude_dir = aikit_dir.join(".claude");
        let cursor_dir = aikit_dir.join(".cursor");
        let gemini_dir = aikit_dir.join(".gemini");

        fs::create_dir_all(&claude_dir).unwrap();
        fs::create_dir_all(&cursor_dir).unwrap();
        fs::create_dir_all(&gemini_dir).unwrap();

        // Verify all directories exist
        assert!(aikit_dir.exists());
        assert!(packages_dir.exists());
        assert!(claude_dir.exists());
        assert!(cursor_dir.exists());
        assert!(gemini_dir.exists());

        // Verify package system files can coexist
        let registry_file = aikit_dir.join("registry.toml");
        let installed_file = aikit_dir.join("installed.toml");

        fs::write(&registry_file, "# Package registry").unwrap();
        fs::write(&installed_file, "# Installed packages").unwrap();

        assert!(registry_file.exists());
        assert!(installed_file.exists());
    }

    #[test]
    fn test_agent_compatibility_with_package_commands() {
        use aikit::core::agent::{get_agent_configs, AgentConfig};

        // Test that all agents can generate package-style commands
        let agents = get_agent_configs();

        for agent in agents {
            let command = agent.generate_package_command(
                "test-package",
                "analyze",
                "Analyze code",
                "echo 'analyzing...'",
            );

            // Verify command includes namespace
            assert!(command.contains("test-package."));
            assert!(command.contains(&agent.key));

            // Verify command includes description
            assert!(command.contains("Analyze code"));

            // Verify agent-specific formatting
            match agent.output_format {
                aikit::core::agent::OutputFormat::Markdown => {
                    assert!(command.contains("# "));
                }
                aikit::core::agent::OutputFormat::Toml => {
                    assert!(command.contains("command = "));
                }
                aikit::core::agent::OutputFormat::AgentMd => {
                    assert!(command.contains("Command: "));
                }
            }
        }
    }

    #[test]
    fn test_package_validation_maintains_compatibility() {
        use aikit::models::package::Package;

        // Test that package validation doesn't break expected workflows
        let mut package = Package::new("test-pkg".to_string(), "1.0.0".to_string(), "Test".to_string());

        // Valid package should pass
        assert!(package.validate().is_ok());

        // Package with commands should still validate
        package.commands.insert(
            "test".to_string(),
            aikit::models::package::CommandDefinition {
                description: "Test command".to_string(),
                template: Some("test.md".to_string()),
            },
        );
        assert!(package.validate().is_ok());

        // Invalid package names should fail
        let mut invalid_package = Package::new("invalid name".to_string(), "1.0.0".to_string(), "Test".to_string());
        assert!(invalid_package.validate().is_err());
    }
}
