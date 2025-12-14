//! Unit tests for package data structures

use aikit::models::package::{Package, PackageMetadata, CommandDefinition};
use std::collections::HashMap;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_validation() {
        let mut package = Package::new("test-package".to_string(), "1.0.0".to_string(), "Test package".to_string());
        assert!(package.validate().is_ok());

        // Test invalid name
        package.package.name = "invalid name".to_string();
        assert!(package.validate().is_err());

        // Reset and test invalid version
        package.package.name = "valid-name".to_string();
        package.package.version = "1.0".to_string();
        assert!(package.validate().is_err());
    }

    #[test]
    fn test_package_install_dir() {
        let package = Package::new("my-package".to_string(), "2.1.3".to_string(), "Description".to_string());
        assert_eq!(package.install_dir(), "my-package-2.1.3");
    }

    #[test]
    fn test_package_template_creation() {
        let package = Package::create_template(
            "test-pkg".to_string(),
            Some("A test package".to_string()),
            Some("Test Author".to_string()),
        );

        assert_eq!(package.package.name, "test-pkg");
        assert_eq!(package.package.version, "0.1.0");
        assert_eq!(package.package.description, "A test package");
        assert_eq!(package.package.authors, vec!["Test Author"]);

        // Check default commands
        assert!(package.commands.contains_key("help"));
        assert_eq!(package.commands["help"].description, "Show help information");

        // Check default artifacts
        assert!(package.artifacts.contains_key("templates/*.md"));
        assert!(package.artifacts.contains_key("scripts/*"));
    }

    #[test]
    fn test_package_toml_roundtrip() {
        let original = Package::create_template(
            "roundtrip-test".to_string(),
            Some("Roundtrip test package".to_string()),
            Some("Test Author".to_string()),
        );

        // Convert to TOML string
        let toml_str = original.to_toml_string().unwrap();

        // Parse back from TOML string
        let parsed = Package::from_toml_str(&toml_str).unwrap();

        // Verify they match
        assert_eq!(parsed.package.name, original.package.name);
        assert_eq!(parsed.package.version, original.package.version);
        assert_eq!(parsed.package.description, original.package.description);
        assert_eq!(parsed.package.authors, original.package.authors);
        assert_eq!(parsed.commands.len(), original.commands.len());
        assert_eq!(parsed.artifacts.len(), original.artifacts.len());
    }

    #[test]
    fn test_package_structure_creation() {
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let package_dir = temp_dir.path().join("test-package");

        let package = Package::create_template(
            "test-package".to_string(),
            Some("Test package".to_string()),
            None,
        );

        // Create structure
        package.create_structure(&package_dir).unwrap();

        // Verify directories exist
        assert!(package_dir.exists());
        assert!(package_dir.join("templates").exists());
        assert!(package_dir.join("scripts").exists());
        assert!(package_dir.join("docs").exists());
        assert!(package_dir.join("package.toml").exists());

        // Verify package.toml content
        let toml_content = std::fs::read_to_string(package_dir.join("package.toml")).unwrap();
        assert!(toml_content.contains("name = \"test-package\""));
        assert!(toml_content.contains("version = \"0.1.0\""));
    }

    #[test]
    fn test_package_dependency_resolution() {
        let package = Package::new("test-pkg".to_string(), "1.0.0".to_string(), "Test".to_string());

        // Should return empty vec for self-contained packages
        let deps = package.resolve_dependencies().unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn test_invalid_package_names() {
        // Test various invalid package names
        let invalid_names = vec![
            "package with spaces",
            "package@symbol",
            "package#hash",
            "",
            "package/with/slashes",
        ];

        for invalid_name in invalid_names {
            let package = Package::new(invalid_name.to_string(), "1.0.0".to_string(), "Test".to_string());
            assert!(package.validate().is_err(), "Package name '{}' should be invalid", invalid_name);
        }
    }

    #[test]
    fn test_valid_package_names() {
        let valid_names = vec![
            "my-package",
            "package123",
            "package_name",
            "a",
            "package-name-123",
        ];

        for valid_name in valid_names {
            let package = Package::new(valid_name.to_string(), "1.0.0".to_string(), "Test".to_string());
            assert!(package.validate().is_ok(), "Package name '{}' should be valid", valid_name);
        }
    }
}
