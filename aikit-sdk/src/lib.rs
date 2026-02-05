use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Represents a deploy concept that an agent may or may not support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployConcept {
    Command,
    Skill,
    Subagent,
}

impl std::fmt::Display for DeployConcept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeployConcept::Command => write!(f, "command"),
            DeployConcept::Skill => write!(f, "skill"),
            DeployConcept::Subagent => write!(f, "subagent"),
        }
    }
}

/// Error types for deploy operations.
#[derive(Debug)]
pub enum DeployError {
    /// Agent key not found in catalog
    AgentNotFound(String),
    /// The agent does not support the requested concept
    UnsupportedConcept {
        agent_key: String,
        concept: DeployConcept,
    },
    /// Filesystem operation failed
    Io(io::Error),
}

impl std::fmt::Display for DeployError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeployError::AgentNotFound(key) => write!(f, "Agent not found: {}", key),
            DeployError::UnsupportedConcept { agent_key, concept } => {
                write!(f, "Agent '{}' does not support '{}'", agent_key, concept)
            }
            DeployError::Io(err) => write!(f, "Filesystem error: {}", err),
        }
    }
}

impl Error for DeployError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            DeployError::Io(err) => Some(err),
            _ => None,
        }
    }
}

/// Configuration for an AI agent.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentConfig {
    /// Display name of the agent
    pub name: String,
    /// Directory for agent commands
    pub commands_dir: String,
    /// Optional directory for agent skills
    pub skills_dir: Option<String>,
    /// Optional directory for agent subagents
    pub agents_dir: Option<String>,
}

impl AgentConfig {
    /// Get the agent key (lowercase name for matching)
    pub fn key(&self) -> String {
        self.name.to_lowercase()
    }
}

/// Returns the filename for a subagent based on the agent type.
///
/// Copilot uses `.agent.md` extension, all others use `.md`.
pub fn subagent_filename(agent_key: &str, name: &str) -> String {
    if agent_key == "copilot" {
        format!("{}.agent.md", name)
    } else {
        format!("{}.md", name)
    }
}

/// Returns the filename for a command based on the agent type.
///
/// Different agents may have different naming conventions.
/// Default is `{name}.md` unless overridden by specific agents.
pub fn command_filename(agent_key: &str, name: &str) -> String {
    match agent_key {
        "codex" => format!("{}.prompt", name),
        "qwen" => format!("{}.cmd", name),
        "roo" => format!("{}.command", name),
        "codebuddy" => format!("{}.command", name),
        "shai" => format!("{}.command", name),
        "q" => format!("{}.prompt", name),
        "bob" => format!("{}.command", name),
        _ => format!("{}.md", name),
    }
}

/// Returns the subagent file path for an agent.
///
/// Returns an error if the agent does not have an `agents_dir`.
pub fn subagent_path(
    project_root: &Path,
    agent_key: &str,
    name: &str,
) -> Result<PathBuf, DeployError> {
    let config =
        agent(agent_key).ok_or_else(|| DeployError::AgentNotFound(agent_key.to_string()))?;

    let agents_dir = config
        .agents_dir
        .ok_or_else(|| DeployError::UnsupportedConcept {
            agent_key: agent_key.to_string(),
            concept: DeployConcept::Subagent,
        })?;

    let path = project_root.join(&agents_dir);
    Ok(path.join(subagent_filename(agent_key, name)))
}

/// Returns the command directory for an agent.
///
/// Every agent has a commands directory.
pub fn commands_dir(project_root: &Path, agent_key: &str) -> Result<PathBuf, DeployError> {
    let config =
        agent(agent_key).ok_or_else(|| DeployError::AgentNotFound(agent_key.to_string()))?;

    Ok(project_root.join(&config.commands_dir))
}

/// Returns the skill directory for an agent.
///
/// Returns an error if the agent does not have a `skills_dir`.
pub fn skill_dir(
    project_root: &Path,
    agent_key: &str,
    skill_name: &str,
) -> Result<PathBuf, DeployError> {
    let config =
        agent(agent_key).ok_or_else(|| DeployError::AgentNotFound(agent_key.to_string()))?;

    let skills_dir = config
        .skills_dir
        .ok_or_else(|| DeployError::UnsupportedConcept {
            agent_key: agent_key.to_string(),
            concept: DeployConcept::Skill,
        })?;

    let path = project_root.join(&skills_dir);
    Ok(path.join(skill_name))
}

/// Validates an agent key.
///
/// Returns an error if the agent is not in the catalog.
pub fn validate_agent_key(key: &str) -> Result<(), DeployError> {
    if agent(key).is_none() {
        return Err(DeployError::AgentNotFound(key.to_string()));
    }
    Ok(())
}

/// Returns all agents in the catalog.
pub fn all_agents() -> Vec<AgentConfig> {
    AGENTS
        .iter()
        .map(|entry| AgentConfig {
            name: entry.name.to_string(),
            commands_dir: entry.commands.to_string(),
            skills_dir: entry.skills.map(|s| s.to_string()),
            agents_dir: entry.subagents.map(|a| a.to_string()),
        })
        .collect()
}

/// Returns an agent by key.
///
/// Returns `None` if the agent is not in the catalog.
pub fn agent(key: &str) -> Option<AgentConfig> {
    AGENTS
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| AgentConfig {
            name: entry.name.to_string(),
            commands_dir: entry.commands.to_string(),
            skills_dir: entry.skills.map(|s| s.to_string()),
            agents_dir: entry.subagents.map(|a| a.to_string()),
        })
}

/// Deploys a command to an agent's commands directory.
///
/// Creates the commands directory if it doesn't exist.
/// Returns the full path where the file was written.
pub fn deploy_command(
    agent_key: &str,
    project_root: &Path,
    name: &str,
    content: &str,
) -> Result<PathBuf, DeployError> {
    validate_agent_key(agent_key)?;

    let commands_dir = commands_dir(project_root, agent_key)?;
    fs::create_dir_all(&commands_dir).map_err(DeployError::Io)?;

    let filename = command_filename(agent_key, name);
    let path = commands_dir.join(&filename);

    fs::write(&path, content).map_err(DeployError::Io)?;

    Ok(path)
}

/// Deploys a skill to an agent's skills directory.
///
/// Creates the skill directory and subdirectories if they don't exist.
/// Returns the full path to the SKILL.md file.
pub fn deploy_skill(
    agent_key: &str,
    project_root: &Path,
    skill_name: &str,
    skill_md_content: &str,
    optional_scripts: Option<&[(&str, &[u8])]>,
) -> Result<PathBuf, DeployError> {
    validate_agent_key(agent_key)?;

    let skill_dir_path = skill_dir(project_root, agent_key, skill_name)?;

    // Create skill directory and subdirectories
    fs::create_dir_all(&skill_dir_path).map_err(DeployError::Io)?;

    // Create scripts subdirectory if scripts are provided
    if let Some(scripts) = optional_scripts {
        let scripts_dir = skill_dir_path.join("scripts");
        fs::create_dir_all(&scripts_dir).map_err(DeployError::Io)?;

        for (filename, content) in scripts {
            let script_path = scripts_dir.join(filename);
            fs::write(&script_path, content).map_err(DeployError::Io)?;
        }
    }

    // Write SKILL.md
    let skill_md_path = skill_dir_path.join("SKILL.md");
    fs::write(&skill_md_path, skill_md_content).map_err(DeployError::Io)?;

    Ok(skill_md_path)
}

/// Deploys a subagent to an agent's subagents directory.
///
/// Creates the subagents directory if it doesn't exist.
/// Returns the full path to the subagent file.
pub fn deploy_subagent(
    agent_key: &str,
    project_root: &Path,
    name: &str,
    content: &str,
) -> Result<PathBuf, DeployError> {
    validate_agent_key(agent_key)?;

    let subagent_path = subagent_path(project_root, agent_key, name)?;

    // Create agents directory if it doesn't exist
    if let Some(parent) = subagent_path.parent() {
        fs::create_dir_all(parent).map_err(DeployError::Io)?;
    }

    fs::write(&subagent_path, content).map_err(DeployError::Io)?;

    Ok(subagent_path)
}

/// Agent catalog entry containing all supported agents and their capabilities.
struct AgentEntry<'a> {
    key: &'a str,
    name: &'a str,
    commands: &'a str,
    skills: Option<&'a str>,
    subagents: Option<&'a str>,
}

const AGENTS: &[AgentEntry] = &[
    AgentEntry {
        key: "claude",
        name: "Claude Code",
        commands: ".claude/commands",
        skills: Some(".claude/skills"),
        subagents: Some(".claude/agents"),
    },
    AgentEntry {
        key: "gemini",
        name: "Google Gemini",
        commands: ".gemini/commands",
        skills: Some(".gemini/skills"),
        subagents: Some(".gemini/agents"),
    },
    AgentEntry {
        key: "copilot",
        name: "GitHub Copilot",
        commands: ".github/agents",
        skills: None,
        subagents: Some(".github/agents"),
    },
    AgentEntry {
        key: "cursor-agent",
        name: "Cursor",
        commands: ".cursor/commands",
        skills: Some(".cursor/skills"),
        subagents: Some(".cursor/agents"),
    },
    AgentEntry {
        key: "qwen",
        name: "Qwen Code",
        commands: ".qwen/commands",
        skills: None,
        subagents: None,
    },
    AgentEntry {
        key: "newton",
        name: "Newton",
        commands: ".newton/commands",
        skills: Some(".newton/skills"),
        subagents: Some(".newton/agents"),
    },
    AgentEntry {
        key: "opencode",
        name: "opencode",
        commands: ".opencode/commands",
        skills: None,
        subagents: None,
    },
    AgentEntry {
        key: "codex",
        name: "Codex CLI",
        commands: ".codex/prompts",
        skills: Some(".codex/skills"),
        subagents: None,
    },
    AgentEntry {
        key: "windsurf",
        name: "Windsurf",
        commands: ".windsurf/workflows",
        skills: Some(".windsurf/skills"),
        subagents: None,
    },
    AgentEntry {
        key: "kilocode",
        name: "Kilo Code",
        commands: ".kilocode/workflows",
        skills: Some(".kilocode/skills"),
        subagents: None,
    },
    AgentEntry {
        key: "auggie",
        name: "Auggie CLI",
        commands: ".augment/commands",
        skills: Some(".augment/skills"),
        subagents: Some(".augment/agents"),
    },
    AgentEntry {
        key: "roo",
        name: "Roo Code",
        commands: ".roo/commands",
        skills: Some(".roo/skills"),
        subagents: None,
    },
    AgentEntry {
        key: "codebuddy",
        name: "CodeBuddy CLI",
        commands: ".codebuddy/commands",
        skills: None,
        subagents: None,
    },
    AgentEntry {
        key: "qoder",
        name: "Qoder CLI",
        commands: ".qoder/commands",
        skills: None,
        subagents: Some(".qoder/agents"),
    },
    AgentEntry {
        key: "amp",
        name: "Amp",
        commands: ".agents/commands",
        skills: None,
        subagents: None,
    },
    AgentEntry {
        key: "shai",
        name: "SHAI",
        commands: ".shai/commands",
        skills: None,
        subagents: None,
    },
    AgentEntry {
        key: "q",
        name: "Amazon Q Developer",
        commands: ".amazonq/prompts",
        skills: None,
        subagents: None,
    },
    AgentEntry {
        key: "bob",
        name: "IBM Bob",
        commands: ".bob/commands",
        skills: None,
        subagents: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    use insta::assert_snapshot;

    fn with_snapshot_path<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path("../tests/fixture/snapshots");
        settings.bind(f)
    }

    #[test]
    fn test_all_agents_count() {
        assert_eq!(all_agents().len(), 18);
    }

    #[test]
    fn test_agent_validation() {
        assert!(validate_agent_key("claude").is_ok());
        assert!(validate_agent_key("copilot").is_ok());
        assert!(validate_agent_key("nonexistent").is_err());
    }

    #[test]
    fn test_agent_lookup() {
        assert!(agent("claude").is_some());
        assert!(agent("copilot").is_some());
        assert!(agent("nonexistent").is_none());
    }

    #[test]
    fn test_agent_fields() {
        let config = agent("claude").unwrap();
        assert_eq!(config.name, "Claude Code");
        assert_eq!(config.commands_dir, ".claude/commands");
        assert!(config.skills_dir.is_some());
        assert!(config.agents_dir.is_some());
    }

    #[test]
    fn test_agent_fields_for_copilot() {
        let config = agent("copilot").unwrap();
        assert_eq!(config.name, "GitHub Copilot");
        assert_eq!(config.commands_dir, ".github/agents");
        assert!(config.skills_dir.is_none());
        assert!(config.agents_dir.is_some());
    }

    #[test]
    fn test_agent_fields_for_qwen() {
        let config = agent("qwen").unwrap();
        assert_eq!(config.name, "Qwen Code");
        assert_eq!(config.commands_dir, ".qwen/commands");
        assert!(config.skills_dir.is_none());
        assert!(config.agents_dir.is_none());
    }

    #[test]
    fn test_agent_fields_for_newton() {
        let config = agent("newton").unwrap();
        assert_eq!(config.name, "Newton");
        assert_eq!(config.commands_dir, ".newton/commands");
        assert!(config.skills_dir.is_some());
        assert!(config.agents_dir.is_some());
    }

    #[test]
    fn test_commands_dir() {
        let temp_dir = TempDir::new().unwrap();
        let path = commands_dir(temp_dir.path(), "claude").unwrap();
        assert_eq!(path, temp_dir.path().join(".claude/commands"));
    }

    #[test]
    fn test_deploy_skill_unsupported() {
        let temp_dir = TempDir::new().unwrap();
        let result = deploy_skill("qwen", temp_dir.path(), "my-skill", "# skill", None);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DeployError::UnsupportedConcept { .. }
        ));
    }

    #[test]
    fn test_deploy_subagent() {
        let temp_dir = TempDir::new().unwrap();
        let content = "# My Subagent\nConfig here.";
        let path = deploy_subagent("claude", temp_dir.path(), "my-agent", content).unwrap();
        assert!(path.exists());
        let file_content = fs::read_to_string(&path).unwrap();
        assert_eq!(file_content, content);
    }

    #[test]
    fn test_deploy_subagent_unsupported() {
        let temp_dir = TempDir::new().unwrap();
        let result = deploy_subagent("qwen", temp_dir.path(), "my-agent", "# agent");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DeployError::UnsupportedConcept { .. }
        ));
    }

    #[test]
    fn test_skill_dir_qwen() {
        let temp_dir = TempDir::new().unwrap();
        let result = skill_dir(temp_dir.path(), "qwen", "my-skill");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DeployError::UnsupportedConcept { .. }
        ));
    }

    #[test]
    fn test_subagent_path_copilot() {
        let temp_dir = TempDir::new().unwrap();
        let path = subagent_path(temp_dir.path(), "copilot", "my-agent").unwrap();
        assert_eq!(
            path,
            temp_dir.path().join(".github/agents/my-agent.agent.md")
        );
    }

    #[test]
    fn test_subagent_filename_convention() {
        assert_eq!(subagent_filename("claude", "test"), "test.md");
        assert_eq!(subagent_filename("copilot", "test"), "test.agent.md");
        assert_eq!(subagent_filename("cursor-agent", "test"), "test.md");
    }

    #[test]
    fn test_subagent_path_qwen() {
        let temp_dir = TempDir::new().unwrap();
        let result = subagent_path(temp_dir.path(), "qwen", "my-agent");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DeployError::UnsupportedConcept { .. }
        ));
    }

    #[test]
    fn test_deploy_command() {
        let temp_dir = TempDir::new().unwrap();
        let content = "# My Command\nHello World";
        let path = deploy_command("claude", temp_dir.path(), "test-command", content).unwrap();
        assert!(path.exists());
        let file_content = fs::read_to_string(&path).unwrap();
        assert_eq!(file_content, content);
    }

    #[test]
    fn test_deploy_skill() {
        let temp_dir = TempDir::new().unwrap();
        let skill_md = "# Skill Name\n\nDescription here.";
        let scripts: &[(&str, &[u8])] = &[
            ("setup.sh", b"#!/bin/sh\necho 'setup'"),
            ("cleanup.sh", b"#!/bin/sh\necho 'cleanup'"),
        ];

        let path = deploy_skill(
            "cursor-agent",
            temp_dir.path(),
            "my-skill",
            skill_md,
            Some(scripts),
        )
        .unwrap();

        assert!(path.exists());
        assert!(path.parent().unwrap().join("scripts/setup.sh").exists());
        assert!(path.parent().unwrap().join("scripts/cleanup.sh").exists());
    }

    #[test]
    fn test_deploy_skill_for_newton() {
        let temp_dir = TempDir::new().unwrap();
        let skill_md = "# Newton Skill\n\nThis is a skill for Newton.";
        let scripts: &[(&str, &[u8])] =
            &[("newton_script.sh", b"#!/bin/sh\necho 'Hello from Newton'")];

        let path = deploy_skill(
            "newton",
            temp_dir.path(),
            "newton-skill",
            skill_md,
            Some(scripts),
        )
        .unwrap();

        assert!(path.exists());
        assert!(path
            .parent()
            .unwrap()
            .join("scripts/newton_script.sh")
            .exists());

        let skill_content = fs::read_to_string(&path).unwrap();
        assert_eq!(skill_content, skill_md);

        let script_content =
            fs::read_to_string(path.parent().unwrap().join("scripts/newton_script.sh")).unwrap();
        assert_eq!(script_content, "#!/bin/sh\necho 'Hello from Newton'");
    }

    #[test]
    fn test_command_filename_convention() {
        assert_eq!(command_filename("claude", "test"), "test.md");
        assert_eq!(command_filename("codex", "test"), "test.prompt");
        assert_eq!(command_filename("qwen", "test"), "test.cmd");
        assert_eq!(command_filename("roo", "test"), "test.command");
        assert_eq!(command_filename("codebuddy", "test"), "test.command");
        assert_eq!(command_filename("shai", "test"), "test.command");
        assert_eq!(command_filename("q", "test"), "test.prompt");
        assert_eq!(command_filename("bob", "test"), "test.command");
    }

    #[test]
    fn test_deployed_command_snapshot() {
        with_snapshot_path(|| {
            let temp_dir = TempDir::new().unwrap();
            let content = "# My Command\nHello World";

            let path = deploy_command("claude", temp_dir.path(), "test-command", content).unwrap();

            let deployed_content = fs::read_to_string(&path).unwrap();
            assert_snapshot!(deployed_content, @"
            # My Command
            Hello World
            ");
        });
    }

    #[test]
    fn test_deployed_skill_snapshot() {
        let temp_dir = TempDir::new().unwrap();
        let skill_md = "# Skill Name\n\nDescription here.";
        let scripts: &[(&str, &[u8])] = &[
            ("setup.sh", b"#!/bin/sh\necho 'setup'"),
            ("cleanup.sh", b"#!/bin/sh\necho 'cleanup'"),
        ];

        let path = deploy_skill(
            "cursor-agent",
            temp_dir.path(),
            "my-skill",
            skill_md,
            Some(scripts),
        )
        .unwrap();

        let skill_content = fs::read_to_string(&path).unwrap();
        assert_snapshot!(skill_content, @"
        # Skill Name

        Description here.
        ");

        let setup_content =
            fs::read_to_string(path.parent().unwrap().join("scripts/setup.sh")).unwrap();
        assert_snapshot!(setup_content, @"
        #!/bin/sh
        echo 'setup'
        ");

        let cleanup_content =
            fs::read_to_string(path.parent().unwrap().join("scripts/cleanup.sh")).unwrap();
        assert_snapshot!(cleanup_content, @"
        #!/bin/sh
        echo 'cleanup'
        ");
    }

    #[test]
    fn test_deployed_subagent_snapshot() {
        with_snapshot_path(|| {
            let temp_dir = TempDir::new().unwrap();
            let content = "# My Subagent\nConfig here.";

            let path = deploy_subagent("claude", temp_dir.path(), "my-agent", content).unwrap();

            let deployed_content = fs::read_to_string(&path).unwrap();
            assert_snapshot!(deployed_content, @"
            # My Subagent
            Config here.
            ");
        });
    }

    #[test]
    fn test_copilot_subagent_filename_snapshot() {
        with_snapshot_path(|| {
            assert_snapshot!(subagent_filename("copilot", "my-agent"), @"my-agent.agent.md");
            assert_snapshot!(subagent_filename("claude", "test"), @"test.md");
        });
    }

    #[test]
    fn test_all_agents_catalog_snapshot() {
        with_snapshot_path(|| {
            let agents = all_agents();
            let catalog: Vec<String> = agents.iter().map(|a| a.name.clone()).collect();
            let catalog_str = catalog
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            assert_snapshot!(catalog_str);
        });
    }
}

#[cfg(test)]
mod catalog_tests {
    use super::*;

    #[test]
    fn test_catalog_contains_all_agents() {
        let all = all_agents();
        assert_eq!(all.len(), 18);

        let keys: Vec<_> = all.iter().map(|a| a.name.clone()).collect();
        assert!(keys.contains(&"Claude Code".to_string()));
        assert!(keys.contains(&"Google Gemini".to_string()));
        assert!(keys.contains(&"GitHub Copilot".to_string()));
        assert!(keys.contains(&"Cursor".to_string()));
        assert!(keys.contains(&"Qwen Code".to_string()));
        assert!(keys.contains(&"Newton".to_string()));
        assert!(keys.contains(&"opencode".to_string()));
        assert!(keys.contains(&"Codex CLI".to_string()));
        assert!(keys.contains(&"Windsurf".to_string()));
        assert!(keys.contains(&"Kilo Code".to_string()));
        assert!(keys.contains(&"Auggie CLI".to_string()));
        assert!(keys.contains(&"Roo Code".to_string()));
        assert!(keys.contains(&"CodeBuddy CLI".to_string()));
        assert!(keys.contains(&"Qoder CLI".to_string()));
        assert!(keys.contains(&"Amp".to_string()));
        assert!(keys.contains(&"SHAI".to_string()));
        assert!(keys.contains(&"Amazon Q Developer".to_string()));
        assert!(keys.contains(&"IBM Bob".to_string()));
    }

    #[test]
    fn test_catalog_copilot_no_skills() {
        let config = agent("copilot").unwrap();
        assert!(config.skills_dir.is_none());
        assert!(config.agents_dir.is_some());
    }

    #[test]
    fn test_catalog_qwen_no_skills_or_agents() {
        let config = agent("qwen").unwrap();
        assert!(config.skills_dir.is_none());
        assert!(config.agents_dir.is_none());
    }

    #[test]
    fn test_catalog_cursor_has_both() {
        let config = agent("cursor-agent").unwrap();
        assert!(config.skills_dir.is_some());
        assert!(config.agents_dir.is_some());
    }
}
