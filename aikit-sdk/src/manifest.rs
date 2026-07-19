use std::collections::HashMap;

use serde::Deserialize;

use crate::install::InstallError;
use crate::paths::{is_safe_id, is_safe_relative_path};

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
    /// Parse manifest from TOML string.
    ///
    /// SEC-1 / SEC-4 / ADR 0013: every `[artifacts]` destination and the package
    /// `name`/`version` are validated here, *before* the manifest is ever used to write a
    /// single file. A single unsafe mapping aborts the whole parse — no partial installs,
    /// one error site (D3 in the audit's grill decisions).
    pub fn from_toml_str(s: &str) -> Result<Self, InstallError> {
        let manifest: TemplateManifest =
            toml::from_str(s).map_err(|e| InstallError::ManifestParse(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate that this manifest cannot be used to write outside the target project.
    fn validate(&self) -> Result<(), InstallError> {
        // SEC-4: name/version become a cache-dir segment (`{name}-{version}`) elsewhere in
        // the install pipeline; they are flat identifiers, not path fragments.
        if !is_safe_id(&self.package.name) {
            return Err(InstallError::UnsafePackageId(self.package.name.clone()));
        }
        if !is_safe_id(&self.package.version) {
            return Err(InstallError::UnsafePackageId(self.package.version.clone()));
        }

        // SEC-1: every artifact destination must be a safe relative path fragment. The
        // concrete project root isn't known at parse time, so this is the lexical half of
        // `safe_join`'s check (no absolute path, no `..`); the real join (with the
        // canonicalized-base escape check) happens in `install::copy_artifacts`.
        for dest in self.artifacts.values() {
            let trimmed = dest.trim_end_matches('/');
            if !is_safe_relative_path(trimmed) {
                return Err(InstallError::UnsafeArtifactDest(dest.clone()));
            }
        }

        Ok(())
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

    // -- SEC-1: malicious [artifacts] dest is rejected at parse time -----------------------

    #[test]
    fn test_parse_rejects_absolute_artifact_dest() {
        let toml_str = r#"
[package]
name = "evil"
version = "1.0.0"

[artifacts]
"payload/**" = "/home/victim/.ssh"
"#;

        let result = TemplateManifest::from_toml_str(toml_str);
        match result.unwrap_err() {
            InstallError::UnsafeArtifactDest(dest) => {
                assert_eq!(dest, "/home/victim/.ssh");
            }
            other => panic!("Expected UnsafeArtifactDest, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_rejects_traversal_artifact_dest() {
        let toml_str = r#"
[package]
name = "evil"
version = "1.0.0"

[artifacts]
"payload/**" = "../../../etc/cron.d"
"#;

        let result = TemplateManifest::from_toml_str(toml_str);
        match result.unwrap_err() {
            InstallError::UnsafeArtifactDest(dest) => {
                assert_eq!(dest, "../../../etc/cron.d");
            }
            other => panic!("Expected UnsafeArtifactDest, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_rejects_traversal_with_trailing_slash() {
        let toml_str = r#"
[package]
name = "evil"
version = "1.0.0"

[artifacts]
"payload/**" = "../escape/"
"#;

        let result = TemplateManifest::from_toml_str(toml_str);
        assert!(matches!(
            result.unwrap_err(),
            InstallError::UnsafeArtifactDest(_)
        ));
    }

    // -- SEC-4: malicious package name/version is rejected at parse time -------------------

    #[test]
    fn test_parse_rejects_traversal_package_name() {
        let toml_str = r#"
[package]
name = "../../etc"
version = "1.0.0"
"#;

        let result = TemplateManifest::from_toml_str(toml_str);
        assert!(matches!(
            result.unwrap_err(),
            InstallError::UnsafePackageId(_)
        ));
    }

    #[test]
    fn test_parse_rejects_unsafe_package_version() {
        let toml_str = r#"
[package]
name = "ok-name"
version = "../../etc"
"#;

        let result = TemplateManifest::from_toml_str(toml_str);
        assert!(matches!(
            result.unwrap_err(),
            InstallError::UnsafePackageId(_)
        ));
    }

    #[test]
    fn test_parse_valid_manifest_with_multiple_safe_artifacts_still_ok() {
        let toml_str = r#"
[package]
name = "good-pkg"
version = "1.0.0"

[artifacts]
"newton/**" = ".newton"
"templates/**" = ".templates/"
"#;

        let manifest = TemplateManifest::from_toml_str(toml_str).unwrap();
        assert_eq!(manifest.artifacts.len(), 2);
    }
}
