//! Universal Package Data Structures
//!
//! This module defines the data structures for AIKIT's universal package system.
//! Packages are defined by aikit.toml files and can contain any kind of reusable
//! content (prompts, templates, scripts, configurations) for AI agents.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Package metadata from package.toml [package] section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    /// Package name (must be unique, used as directory name)
    pub name: String,
    /// Semantic version (e.g., "1.0.0")
    pub version: String,
    /// Human-readable description
    pub description: String,
    /// Package authors
    pub authors: Vec<String>,
    /// License identifier (optional)
    pub license: Option<String>,
    /// Homepage URL (optional)
    pub homepage: Option<String>,
    /// Repository URL (optional)
    pub repository: Option<String>,
}

/// Command definition from package.toml [commands] section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDefinition {
    /// Human-readable description of what the command does
    pub description: String,
    /// Path to the template file within the package (optional, defaults to commands/{name}.md)
    pub template: Option<String>,
}

/// Artifact mapping from package.toml [artifacts] section
/// Maps source paths (in package) to destination paths (in .aikit/)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMapping {
    /// Source path pattern in the package (supports glob patterns)
    pub source: String,
    /// Destination path in .aikit/ directory
    pub destination: String,
}

/// Agent-specific override from package.toml [agents.{agent}] sections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOverride {
    /// Override for command script template
    pub script_template: Option<String>,
    /// Override for argument format
    pub arg_format: Option<String>,
    /// Agent-specific artifact mappings
    pub artifacts: Option<HashMap<String, String>>,
}

/// Complete package definition parsed from package.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    /// Package metadata
    pub package: PackageMetadata,
    /// Available commands in this package
    pub commands: HashMap<String, CommandDefinition>,
    /// Artifact mappings for installation
    pub artifacts: HashMap<String, String>,
    /// Agent-specific overrides
    pub agents: HashMap<String, AgentOverride>,
}

impl Package {
    /// Create a new package with minimal metadata
    pub fn new(name: String, version: String, description: String) -> Self {
        Self {
            package: PackageMetadata {
                name,
                version,
                description,
                authors: Vec::new(),
                license: None,
                homepage: None,
                repository: None,
            },
            commands: HashMap::new(),
            artifacts: HashMap::new(),
            agents: HashMap::new(),
        }
    }

    /// Validate package structure and required fields
    pub fn validate(&self) -> Result<(), String> {
        // Validate package name
        if self.package.name.is_empty() {
            return Err("Package name cannot be empty".to_string());
        }

        if !self
            .package
            .name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(
                "Package name must contain only alphanumeric characters, hyphens, and underscores"
                    .to_string(),
            );
        }

        // Validate version format (semantic versioning)
        let version_parts: Vec<&str> = self.package.version.split('.').collect();
        if version_parts.len() != 3 {
            return Err("Version must follow semantic versioning (X.Y.Z)".to_string());
        }

        for part in version_parts {
            if part.parse::<u32>().is_err() {
                return Err("Version parts must be numeric".to_string());
            }
        }

        // Validate command templates exist in artifacts or have valid paths
        for (cmd_name, cmd_def) in &self.commands {
            if let Some(template) = &cmd_def.template {
                if template.is_empty() {
                    return Err(format!("Command '{}' has empty template path", cmd_name));
                }
            }
        }

        // Validate dependency resolution (packages are self-contained)
        self.validate_dependencies()?;

        Ok(())
    }

    /// Validate package dependencies (packages must be self-contained)
    pub fn validate_dependencies(&self) -> Result<(), String> {
        // In the current design, packages are self-contained with no external dependencies
        // This method exists for future extension if dependencies are added later

        // For now, just ensure no dependency fields are present that shouldn't be
        // (The TOML parsing will handle this naturally)

        Ok(())
    }

    /// Resolve dependencies for this package (returns empty for self-contained packages)
    pub fn resolve_dependencies(&self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        // Packages are self-contained, so no dependencies to resolve
        Ok(Vec::new())
    }

    /// Get the install directory name for this package
    pub fn install_dir(&self) -> String {
        format!("{}-{}", self.package.name, self.package.version)
    }

    /// Get all artifact mappings including agent-specific overrides
    pub fn get_artifact_mappings(&self, agent: Option<&str>) -> HashMap<String, String> {
        let mut mappings = self.artifacts.clone();

        // Apply agent-specific overrides
        if let Some(agent_name) = agent {
            if let Some(agent_override) = self.agents.get(agent_name) {
                if let Some(agent_artifacts) = &agent_override.artifacts {
                    for (source, dest) in agent_artifacts {
                        mappings.insert(source.clone(), dest.clone());
                    }
                }
            }
        }

        mappings
    }

    /// Parse package from TOML file
    pub fn from_toml_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml_str(&content)
    }

    /// Parse package from TOML string
    pub fn from_toml_str(content: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let parsed: TomlPackage = toml::from_str(content)?;
        parsed.try_into()
    }

    /// Write package to TOML file
    pub fn to_toml_file(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let toml = self.to_toml_string()?;
        std::fs::write(path, toml)?;
        Ok(())
    }

    /// Convert package to TOML string
    pub fn to_toml_string(&self) -> Result<String, Box<dyn std::error::Error>> {
        let toml_package = TomlPackage::from(self.clone());
        Ok(toml::to_string_pretty(&toml_package)?)
    }

    /// Create package structure on disk
    pub fn create_structure(&self, base_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        use std::fs;

        // Create base directory
        fs::create_dir_all(base_path)?;

        // Create standard subdirectories
        fs::create_dir_all(base_path.join("templates"))?;
        fs::create_dir_all(base_path.join("scripts"))?;
        fs::create_dir_all(base_path.join("docs"))?;

        // Write package.toml
        self.to_toml_file(&base_path.join("aikit.toml"))?;

        Ok(())
    }

    /// Generate example package structure
    pub fn create_example_structure(
        &self,
        base_path: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::fs;

        // Create basic structure
        self.create_structure(base_path)?;

        // Create example template
        let example_template = r#"# Example Command

This is an example command for the {package_name} package.

**Description**: Demonstrates basic package functionality

**Usage**:
- Run `{package_name}.help` for help
- Add your own commands by creating templates in the templates/ directory

## Package Information

- **Name**: {package_name}
- **Version**: {package_version}
- **Description**: {package_description}

## Development

Edit this template in `templates/example.md` and rebuild the package with:

```bash
aikit package build
```
"#
        .replace("{package_name}", &self.package.name)
        .replace("{package_version}", &self.package.version)
        .replace("{package_description}", &self.package.description);

        fs::write(
            base_path.join("templates").join("example.md"),
            example_template,
        )?;

        // Create README
        let readme = format!(
            r#"# {}

{}

## Installation

```bash
aikit install <github-url-to-this-repo>
```

## Commands

After installation, these commands will be available:

{}

## Development

### Building

```bash
aikit package build
```

### Testing

```bash
aikit package validate
```

## License

Specify your license in package.toml
"#,
            self.package.name,
            self.package.description,
            self.commands
                .keys()
                .map(|cmd| format!("- `{}.{}`", self.package.name, cmd))
                .collect::<Vec<_>>()
                .join("\n")
        );

        fs::write(base_path.join("README.md"), readme)?;

        Ok(())
    }

    /// Create a default package.toml template
    pub fn create_template(
        name: String,
        description: Option<String>,
        author: Option<String>,
    ) -> Self {
        let mut package = Self::new(
            name.clone(),
            "0.1.0".to_string(),
            description.unwrap_or_else(|| format!("{} package", name)),
        );

        if let Some(author) = author {
            package.package.authors = vec![author];
        }

        // Add default commands
        package.commands.insert(
            "help".to_string(),
            CommandDefinition {
                description: "Show help information".to_string(),
                template: Some("help.md".to_string()),
            },
        );

        // Add default artifacts
        package.artifacts.insert(
            "templates/*.md".to_string(),
            ".aikit/templates/".to_string(),
        );
        package
            .artifacts
            .insert("scripts/*".to_string(), ".aikit/scripts/".to_string());

        package
    }
}

/// Intermediate TOML representation for parsing
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TomlPackage {
    package: PackageMetadata,
    #[serde(default)]
    commands: HashMap<String, CommandDefinition>,
    #[serde(default)]
    artifacts: HashMap<String, String>,
    #[serde(default)]
    agents: HashMap<String, AgentOverride>,
}

impl TryFrom<TomlPackage> for Package {
    type Error = Box<dyn std::error::Error>;

    fn try_from(toml: TomlPackage) -> Result<Self, Self::Error> {
        let package = Package {
            package: toml.package,
            commands: toml.commands,
            artifacts: toml.artifacts,
            agents: toml.agents,
        };

        package.validate()?;
        Ok(package)
    }
}

impl From<Package> for TomlPackage {
    fn from(package: Package) -> Self {
        Self {
            package: package.package,
            commands: package.commands,
            artifacts: package.artifacts,
            agents: package.agents,
        }
    }
}

/// Package installation state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackage {
    /// Package metadata
    pub package: PackageMetadata,
    /// Installation timestamp
    pub installed_at: chrono::DateTime<chrono::Utc>,
    /// Source repository URL
    pub source_url: String,
    /// Installation directory relative to .aikit/
    pub install_path: String,
}

/// Package registry entry for search/discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageRegistryEntry {
    /// Package name
    pub name: String,
    /// Latest version
    pub version: String,
    /// Description
    pub description: String,
    /// Repository URL
    pub repository: String,
    /// Download count (optional)
    pub downloads: Option<u32>,
    /// Last updated timestamp
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl PackageRegistryEntry {
    /// Create from a Package
    pub fn from_package(package: &Package, repository: String) -> Self {
        Self {
            name: package.package.name.clone(),
            version: package.package.version.clone(),
            description: package.package.description.clone(),
            repository,
            downloads: None,
            updated_at: Some(chrono::Utc::now()),
        }
    }
}
