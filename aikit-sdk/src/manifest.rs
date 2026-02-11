use std::collections::HashMap;

use serde::Deserialize;

use crate::install::InstallError;

/// Package metadata from [package] section of aikit.toml
#[derive(Debug, Clone, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
}

/// Minimal template manifest from aikit.toml
#[derive(Debug, Clone, Deserialize)]
pub struct TemplateManifest {
    pub package: PackageInfo,
    #[serde(default)]
    pub artifacts: HashMap<String, String>,
}

impl TemplateManifest {
    /// Parse manifest from TOML string
    pub fn from_toml_str(s: &str) -> Result<Self, InstallError> {
        toml::from_str(s).map_err(|e| InstallError::ManifestParse(e.to_string()))
    }

    /// Get artifact mappings for copy_artifacts
    pub fn artifact_mappings(&self) -> &HashMap<String, String> {
        &self.artifacts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_manifest() {
        let toml_str = r#"
[package]
name = "test-template"
version = "1.0.0"
description = "Test template"
authors = ["test@example.com"]

[artifacts]
"newton/**" = ".newton"
"#;

        let manifest = TemplateManifest::from_toml_str(toml_str).unwrap();
        assert_eq!(manifest.package.name, "test-template");
        assert_eq!(manifest.package.version, "1.0.0");
        assert_eq!(manifest.package.description, "Test template");
        assert_eq!(manifest.package.authors, vec!["test@example.com"]);
        assert_eq!(manifest.artifacts.len(), 1);
        assert_eq!(
            manifest.artifacts.get("newton/**"),
            Some(&".newton".to_string())
        );
    }

    #[test]
    fn test_parse_minimal_manifest() {
        let toml_str = r#"
[package]
name = "minimal"
version = "0.1.0"
"#;

        let manifest = TemplateManifest::from_toml_str(toml_str).unwrap();
        assert_eq!(manifest.package.name, "minimal");
        assert_eq!(manifest.package.version, "0.1.0");
        assert!(manifest.package.description.is_empty());
        assert!(manifest.package.authors.is_empty());
        assert!(manifest.artifacts.is_empty());
    }

    #[test]
    fn test_parse_invalid_toml() {
        let toml_str = r#"
[package
name = "invalid
"#;

        let result = TemplateManifest::from_toml_str(toml_str);
        assert!(result.is_err());
        match result.unwrap_err() {
            InstallError::ManifestParse(_) => {}
            _ => panic!("Expected ManifestParse error"),
        }
    }

    #[test]
    fn test_parse_missing_required_fields() {
        let toml_str = r#"
[package]
name = "missing-version"
"#;

        let result = TemplateManifest::from_toml_str(toml_str);
        assert!(result.is_err());
        match result.unwrap_err() {
            InstallError::ManifestParse(_) => {}
            _ => panic!("Expected ManifestParse error"),
        }
    }

    #[test]
    fn test_artifact_mappings() {
        let toml_str = r#"
[package]
name = "multi-artifact"
version = "1.0.0"

[artifacts]
"newton/**" = ".newton"
"templates/**" = ".templates"
"#;

        let manifest = TemplateManifest::from_toml_str(toml_str).unwrap();
        let mappings = manifest.artifact_mappings();
        assert_eq!(mappings.len(), 2);
        assert!(mappings.contains_key("newton/**"));
        assert!(mappings.contains_key("templates/**"));
    }
}
