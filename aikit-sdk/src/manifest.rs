use crate::install::InstallError;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
pub struct TemplateManifest {
    pub package: PackageInfo,
    #[serde(default)]
    pub artifacts: HashMap<String, String>,
}

impl TemplateManifest {
    pub fn from_toml_str(s: &str) -> Result<Self, InstallError> {
        toml::from_str(s).map_err(|e| InstallError::ManifestParse(format!("{}", e)))
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

[artifacts]
"newton/**" = ".newton"
"templates/*.md" = ".templates"
"#;

        let manifest = TemplateManifest::from_toml_str(toml_str).unwrap();
        assert_eq!(manifest.package.name, "test-template");
        assert_eq!(manifest.package.version, "1.0.0");
        assert_eq!(manifest.artifacts.len(), 2);
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
        assert!(matches!(
            result.unwrap_err(),
            InstallError::ManifestParse(_)
        ));
    }

    #[test]
    fn test_parse_missing_package_section() {
        let toml_str = r#"
[artifacts]
"test/**" = ".test"
"#;

        let result = TemplateManifest::from_toml_str(toml_str);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InstallError::ManifestParse(_)
        ));
    }

    #[test]
    fn test_parse_missing_name() {
        let toml_str = r#"
[package]
version = "1.0.0"
"#;

        let result = TemplateManifest::from_toml_str(toml_str);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InstallError::ManifestParse(_)
        ));
    }

    #[test]
    fn test_parse_missing_version() {
        let toml_str = r#"
[package]
name = "test"
"#;

        let result = TemplateManifest::from_toml_str(toml_str);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InstallError::ManifestParse(_)
        ));
    }
}
