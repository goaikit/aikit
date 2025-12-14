//! Package generation logic
//!
//! This module handles generation of template zip archives for releases.

use crate::core::agent::{get_agent_configs, AgentConfig, OutputFormat, ScriptVariant};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Package configuration
///
/// Represents packaging configuration for release builds.
#[derive(Debug, Clone)]
pub struct PackageConfig {
    /// Version with 'v' prefix (e.g., "v1.0.0")
    pub version: String,
    /// Optional agent filter list
    pub agents: Option<Vec<String>>,
    /// Optional script type filter
    pub scripts: Option<Vec<ScriptVariant>>,
    /// Output directory (default: ".genreleases/")
    pub output_dir: PathBuf,
}

impl PackageConfig {
    /// Validate the package configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate version format (vX.Y.Z)
        if !self.version.starts_with('v') {
            return Err(format!("Version '{}' must start with 'v'", self.version));
        }

        let version_part = &self.version[1..];
        let parts: Vec<&str> = version_part.split('.').collect();
        if parts.len() != 3 {
            return Err(format!(
                "Version '{}' must match pattern vX.Y.Z",
                self.version
            ));
        }

        for part in parts {
            if part.parse::<u32>().is_err() {
                return Err(format!(
                    "Version '{}' contains invalid numeric parts",
                    self.version
                ));
            }
        }

        // Validate agent filters if provided
        if let Some(ref agents) = self.agents {
            let valid_agents: Vec<String> =
                get_agent_configs().into_iter().map(|a| a.key).collect();
            for agent in agents {
                if !valid_agents.contains(agent) {
                    return Err(format!(
                        "Invalid agent '{}'. Valid agents: {}",
                        agent,
                        valid_agents.join(", ")
                    ));
                }
            }
        }

        // Validate script filters if provided
        if let Some(ref scripts) = self.scripts {
            for script in scripts {
                match script {
                    ScriptVariant::Sh | ScriptVariant::Ps => {} // Valid
                }
            }
        }

        Ok(())
    }

    /// Parse agent filter from environment variable
    pub fn parse_agents_env() -> Option<Vec<String>> {
        std::env::var("AGENTS").ok().map(|val| {
            val.split(|c: char| c == ',' || c.is_whitespace())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
    }

    /// Parse script filter from environment variable
    pub fn parse_scripts_env() -> Option<Vec<ScriptVariant>> {
        std::env::var("SCRIPTS").ok().map(|val| {
            val.split(|c: char| c == ',' || c.is_whitespace())
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .map(|s| match s.as_str() {
                    "sh" => ScriptVariant::Sh,
                    "ps" | "ps1" => ScriptVariant::Ps,
                    _ => ScriptVariant::Sh, // Default fallback
                })
                .collect()
        })
    }
}

/// Command template for package generation
///
/// Represents a command template file with metadata and body content.
#[derive(Debug, Clone)]
pub struct CommandTemplate {
    /// Template filename (e.g., "specify.md")
    pub name: String,
    /// Description from YAML frontmatter
    pub description: String,
    /// Script commands per variant
    pub script_commands: HashMap<ScriptVariant, String>,
    /// Optional agent-specific scripts
    pub agent_script_commands: Option<HashMap<ScriptVariant, String>>,
    /// Template body content (after frontmatter)
    pub body: String,
    /// Original frontmatter YAML (for removal of script sections)
    pub frontmatter: String,
}

impl CommandTemplate {
    /// Parse a command template from a file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read template file: {}", path.display()))?;

        // Parse YAML frontmatter manually (between --- delimiters)
        let lines: Vec<&str> = content.lines().collect();
        let mut in_frontmatter = false;
        let mut frontmatter_lines = Vec::new();
        let mut body_start = 0;

        for (i, line) in lines.iter().enumerate() {
            if line.trim() == "---" {
                if in_frontmatter {
                    body_start = i + 1;
                    break;
                } else {
                    in_frontmatter = true;
                    continue;
                }
            }
            if in_frontmatter {
                frontmatter_lines.push(*line);
            }
        }

        if !in_frontmatter || frontmatter_lines.is_empty() {
            return Err(anyhow::anyhow!("No YAML frontmatter found in template"));
        }

        let frontmatter_yaml = frontmatter_lines.join("\n");
        let body = if body_start < lines.len() {
            lines[body_start..].join("\n")
        } else {
            String::new()
        };

        // Parse frontmatter YAML
        let frontmatter_map: serde_json::Value = serde_yaml::from_str(&frontmatter_yaml)
            .map_err(|e| anyhow::anyhow!("Failed to parse frontmatter YAML: {}", e))?;

        let description = frontmatter_map["description"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Extract script commands
        let mut script_commands = HashMap::new();
        if let Some(scripts) = frontmatter_map.get("scripts").and_then(|s| s.as_object()) {
            if let Some(sh) = scripts.get("sh").and_then(|s| s.as_str()) {
                script_commands.insert(ScriptVariant::Sh, sh.to_string());
            }
            if let Some(ps) = scripts.get("ps").and_then(|s| s.as_str()) {
                script_commands.insert(ScriptVariant::Ps, ps.to_string());
            }
        }

        // Extract agent-specific script commands
        let mut agent_script_commands = None;
        if let Some(agent_scripts) = frontmatter_map
            .get("agent_scripts")
            .and_then(|s| s.as_object())
        {
            let mut agent_map = HashMap::new();
            if let Some(sh) = agent_scripts.get("sh").and_then(|s| s.as_str()) {
                agent_map.insert(ScriptVariant::Sh, sh.to_string());
            }
            if let Some(ps) = agent_scripts.get("ps").and_then(|s| s.as_str()) {
                agent_map.insert(ScriptVariant::Ps, ps.to_string());
            }
            if !agent_map.is_empty() {
                agent_script_commands = Some(agent_map);
            }
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(Self {
            name,
            description,
            script_commands,
            agent_script_commands,
            body,
            frontmatter: frontmatter_yaml,
        })
    }

    /// Generate processed content for a specific agent and script variant
    pub fn generate_content(
        &self,
        agent: &AgentConfig,
        script_variant: ScriptVariant,
    ) -> Result<String> {
        let mut content = self.body.clone();

        // Replace placeholders
        // {SCRIPT} - script command for the variant
        let script_cmd = self.script_commands.get(&script_variant).ok_or_else(|| {
            anyhow::anyhow!("Missing script command for variant {:?}", script_variant)
        })?;
        content = content.replace("{SCRIPT}", script_cmd);

        // {AGENT_SCRIPT} - agent-specific script if available
        if let Some(ref agent_scripts) = self.agent_script_commands {
            if let Some(agent_script) = agent_scripts.get(&script_variant) {
                content = content.replace("{AGENT_SCRIPT}", agent_script);
            }
        }

        // {ARGS} - agent-specific argument placeholder
        content = content.replace("{ARGS}", &agent.arg_placeholder);

        // __AGENT__ - agent key
        content = content.replace("__AGENT__", &agent.key);

        // Path rewriting: memory/ → .specify/memory/
        content = content.replace("memory/", ".specify/memory/");
        content = content.replace("scripts/", ".specify/scripts/");
        content = content.replace("templates/", ".specify/templates/");

        // Reconstruct frontmatter without script sections
        let mut frontmatter_map: serde_json::Value = serde_yaml::from_str(&self.frontmatter)
            .map_err(|e| anyhow::anyhow!("Failed to parse frontmatter: {}", e))?;

        // Remove scripts and agent_scripts sections
        if let Some(obj) = frontmatter_map.as_object_mut() {
            obj.remove("scripts");
            obj.remove("agent_scripts");
        }

        let cleaned_frontmatter = serde_yaml::to_string(&frontmatter_map)
            .map_err(|e| anyhow::anyhow!("Failed to serialize frontmatter: {}", e))?;

        // Combine frontmatter and body
        Ok(format!("---\n{}---\n{}", cleaned_frontmatter, content))
    }

    /// Get the output filename for this template based on agent format
    pub fn output_filename(&self, agent: &AgentConfig) -> String {
        match agent.output_format {
            OutputFormat::Markdown => self.name.clone(),
            OutputFormat::Toml => {
                // Replace .md with .toml
                self.name.replace(".md", ".toml")
            }
            OutputFormat::AgentMd => {
                // For agent.md format, use agent.md as filename
                "agent.md".to_string()
            }
        }
    }
}

/// Load all command templates from templates/commands/ directory
pub fn load_command_templates<P: AsRef<Path>>(templates_dir: P) -> Result<Vec<CommandTemplate>> {
    let templates_path = templates_dir.as_ref().join("commands");
    if !templates_path.exists() {
        return Ok(Vec::new());
    }

    let mut templates = Vec::new();
    for entry in WalkDir::new(&templates_path) {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("md") {
            match CommandTemplate::from_file(path) {
                Ok(template) => templates.push(template),
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to parse template {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }
    }

    Ok(templates)
}

/// Copy base directories with path rewriting
pub fn copy_base_directories<P: AsRef<Path>, Q: AsRef<Path>>(
    source_root: P,
    target_root: Q,
) -> Result<()> {
    let source_root = source_root.as_ref();
    let target_root = target_root.as_ref();

    // Copy memory/ → .specify/memory/
    let memory_source = source_root.join("memory");
    if memory_source.exists() {
        let memory_target = target_root.join(".specify").join("memory");
        fs::create_dir_all(&memory_target)?;
        crate::fs::copy_directory(&memory_source, &memory_target)?;
    }

    // Copy scripts/bash or scripts/powershell → .specify/scripts/<variant>/
    let scripts_source = source_root.join("scripts");
    if scripts_source.exists() {
        for variant_dir in ["bash", "powershell"] {
            let variant_source = scripts_source.join(variant_dir);
            if variant_source.exists() {
                let script_variant = if variant_dir == "bash" { "sh" } else { "ps" };
                let variant_target = target_root
                    .join(".specify")
                    .join("scripts")
                    .join(script_variant);
                fs::create_dir_all(&variant_target)?;
                crate::fs::copy_directory(&variant_source, &variant_target)?;
            }
        }
    }

    // Copy templates/ (excluding commands/* and vscode-settings.json) → .specify/templates/
    let templates_source = source_root.join("templates");
    if templates_source.exists() {
        let templates_target = target_root.join(".specify").join("templates");
        fs::create_dir_all(&templates_target)?;

        for entry in WalkDir::new(&templates_source) {
            let entry = entry?;
            let path = entry.path();
            let relative = path.strip_prefix(&templates_source)?;

            // Skip commands/ directory
            if relative.starts_with("commands") {
                continue;
            }

            // Skip vscode-settings.json
            if relative.file_name().and_then(|n| n.to_str()) == Some("vscode-settings.json") {
                continue;
            }

            let target_path = templates_target.join(relative);
            if path.is_dir() {
                fs::create_dir_all(&target_path)?;
            } else {
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(path, &target_path)?;
            }
        }
    }

    Ok(())
}

/// Generate package for a specific agent and script variant
pub fn generate_package(
    config: &PackageConfig,
    agent: &AgentConfig,
    script_variant: ScriptVariant,
    templates: &[CommandTemplate],
    source_root: &Path,
) -> Result<PathBuf> {
    // Create temporary directory for package
    let temp_dir = tempfile::tempdir()?;
    let package_root = temp_dir.path();

    // Copy base directories
    copy_base_directories(source_root, package_root)?;

    // Create agent-specific output directory
    let output_dir = package_root.join(&agent.output_dir);
    fs::create_dir_all(&output_dir)?;

    // Generate command files from templates
    for template in templates {
        let content = template.generate_content(agent, script_variant)?;
        let filename = template.output_filename(agent);
        let file_path = output_dir.join(&filename);

        let mut file = fs::File::create(&file_path)?;
        file.write_all(content.as_bytes())?;
    }

    // Generate Copilot prompt files if needed
    if agent.key == "copilot" {
        let prompts_dir = package_root.join(".github").join("prompts");
        fs::create_dir_all(&prompts_dir)?;

        // Generate prompt files from templates
        for template in templates {
            let prompt_filename = format!("{}.prompt.md", template.name.replace(".md", ""));
            let prompt_path = prompts_dir.join(&prompt_filename);
            let content = template.generate_content(agent, script_variant)?;
            let mut file = fs::File::create(&prompt_path)?;
            file.write_all(content.as_bytes())?;
        }
    }

    // Create ZIP archive
    let script_str = match script_variant {
        ScriptVariant::Sh => "sh",
        ScriptVariant::Ps => "ps",
    };
    let zip_filename = format!(
        "spec-kit-template-{}-{}-{}.zip",
        agent.key, script_str, config.version
    );
    let zip_path = config.output_dir.join(&zip_filename);

    create_zip_archive(package_root, &zip_path)?;

    Ok(zip_path)
}

/// Create ZIP archive from directory
fn create_zip_archive<P: AsRef<Path>, Q: AsRef<Path>>(source_dir: P, zip_path: Q) -> Result<()> {
    use std::fs::File;
    use zip::write::{FileOptions, ZipWriter};
    use zip::CompressionMethod;

    let file = File::create(&zip_path)
        .with_context(|| format!("Failed to create ZIP file: {}", zip_path.as_ref().display()))?;
    let mut zip = ZipWriter::new(file);
    let options = FileOptions::default().compression_method(CompressionMethod::Deflated);

    let source_dir = source_dir.as_ref();
    let base_path = source_dir.canonicalize()?;

    for entry in WalkDir::new(source_dir) {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .strip_prefix(&base_path)?
            .to_string_lossy()
            .replace('\\', "/");

        if path.is_dir() {
            zip.add_directory(&name, options)?;
        } else {
            let mut file = fs::File::open(path)?;
            zip.start_file(&name, options)?;
            std::io::copy(&mut file, &mut zip)?;
        }
    }

    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_config_validation() {
        let valid = PackageConfig {
            version: "v1.0.0".to_string(),
            agents: None,
            scripts: None,
            output_dir: PathBuf::from(".genreleases"),
        };
        assert!(valid.validate().is_ok());

        let invalid = PackageConfig {
            version: "1.0.0".to_string(), // Missing 'v'
            agents: None,
            scripts: None,
            output_dir: PathBuf::from(".genreleases"),
        };
        assert!(invalid.validate().is_err());
    }
}
