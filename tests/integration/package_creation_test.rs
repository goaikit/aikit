//! Integration tests for package creation and building workflow

use std::fs;
use std::path::Path;
use tempfile::tempdir;

#[cfg(test)]
mod tests {
    use super::*;
    use aikit::models::package::Package;

    #[test]
    fn test_full_package_creation_workflow() {
        let temp_dir = tempdir().unwrap();
        let package_dir = temp_dir.path().join("test-workflow-pkg");

        // Create a package
        let package = Package::create_template(
            "test-workflow-pkg".to_string(),
            Some("Integration test package".to_string()),
            Some("Test Author".to_string()),
        );

        // Create package structure
        package.create_example_structure(&package_dir).unwrap();

        // Verify structure
        assert!(package_dir.exists());
        assert!(package_dir.join("aikit.toml").exists());
        assert!(package_dir.join("README.md").exists());
        assert!(package_dir.join("templates").exists());
        assert!(package_dir.join("templates").join("example.md").exists());

        // Verify aikit.toml content
        let toml_content = fs::read_to_string(package_dir.join("aikit.toml")).unwrap();
        assert!(toml_content.contains("name = \"test-workflow-pkg\""));
        assert!(toml_content.contains("description = \"Integration test package\""));
        assert!(toml_content.contains("authors = [\"Test Author\"]"));

        // Verify README content
        let readme_content = fs::read_to_string(package_dir.join("README.md")).unwrap();
        assert!(readme_content.contains("# test-workflow-pkg"));
        assert!(readme_content.contains("Integration test package"));

        // Load package back from disk
        let loaded_package = Package::from_toml_file(&package_dir.join("aikit.toml")).unwrap();
        assert_eq!(loaded_package.package.name, "test-workflow-pkg");
        assert_eq!(loaded_package.package.version, "0.1.0");

        // Validate package
        assert!(loaded_package.validate().is_ok());
    }

    #[test]
    fn test_package_build_process() {
        let temp_dir = tempdir().unwrap();
        let package_dir = temp_dir.path().join("build-test-pkg");
        let output_dir = temp_dir.path().join("build-output");

        // Create package
        let package = Package::create_template(
            "build-test-pkg".to_string(),
            Some("Package for build testing".to_string()),
            None,
        );

        // Create structure with example content
        package.create_example_structure(&package_dir).unwrap();

        // Create output directory
        fs::create_dir_all(&output_dir).unwrap();

        // Test building (using the build function from CLI)
        // Note: This would normally be called via CLI, but we'll test the core logic

        // Verify that package.toml exists and is valid
        assert!(package_dir.join("aikit.toml").exists());
        let loaded_package = Package::from_toml_file(&package_dir.join("aikit.toml")).unwrap();
        assert!(loaded_package.validate().is_ok());

        // The actual ZIP building would be tested in the CLI integration tests
        // For now, just verify the package structure is ready for building
        assert!(package_dir.join("templates").exists());
        assert!(!fs::read_dir(package_dir.join("templates")).unwrap().next().is_none());
    }

    #[test]
    fn test_package_template_variables() {
        let temp_dir = tempdir().unwrap();
        let package_dir = temp_dir.path().join("template-test-pkg");

        let package = Package::create_template(
            "template-test-pkg".to_string(),
            Some("Template variable test".to_string()),
            None,
        );

        package.create_example_structure(&package_dir).unwrap();

        // Read the example template
        let template_content = fs::read_to_string(package_dir.join("templates").join("example.md")).unwrap();

        // Verify template contains package variables
        assert!(template_content.contains("{{package_name}}"));
        assert!(template_content.contains("{{command_description}}"));
        assert!(template_content.contains("{{package_version}}"));
    }

    #[test]
    fn test_package_validation_edge_cases() {
        // Test package with empty commands
        let mut package = Package::new("edge-case".to_string(), "1.0.0".to_string(), "Test".to_string());
        package.commands.clear(); // Remove default commands
        assert!(package.validate().is_ok()); // Should still be valid

        // Test package with invalid template reference
        package.commands.insert(
            "invalid".to_string(),
            aikit::models::package::CommandDefinition {
                description: "Invalid command".to_string(),
                template: Some("".to_string()), // Empty template path
            },
        );
        assert!(package.validate().is_err()); // Should fail validation
    }
}
