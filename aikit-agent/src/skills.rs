use std::path::{Path, PathBuf};

use crate::errors::AgentError;

#[derive(Debug, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct DiscoveredSkill {
    pub metadata: SkillMetadata,
    /// Full content is NOT loaded at discovery time.
    content_path: PathBuf,
}

impl DiscoveredSkill {
    /// Load the full skill content from disk.
    pub fn load_content(&self) -> Result<String, AgentError> {
        std::fs::read_to_string(&self.content_path).map_err(|e| AgentError::SkillParseError {
            name: self.metadata.name.clone(),
            reason: format!("failed to read: {}", e),
        })
    }
}

pub trait SkillProvider: Send + Sync {
    fn discover(&self, roots: &[PathBuf]) -> Vec<DiscoveredSkill>;
    /// Load skill content by name. When multiple entries share the same name,
    /// the first entry in slice order is selected (first-match policy).
    fn load(&self, skill_name: &str, skills: &[DiscoveredSkill]) -> Result<String, AgentError>;
}

/// Filesystem-based skill provider that scans directories for SKILL.md files.
pub struct FilesystemSkillProvider;

impl SkillProvider for FilesystemSkillProvider {
    fn discover(&self, roots: &[PathBuf]) -> Vec<DiscoveredSkill> {
        let mut skills = Vec::new();
        for root in roots {
            if let Ok(entries) = std::fs::read_dir(root) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let skill_md = path.join("SKILL.md");
                        if skill_md.exists() {
                            match parse_skill_metadata(&skill_md) {
                                Ok(metadata) => {
                                    skills.push(DiscoveredSkill {
                                        metadata,
                                        content_path: skill_md,
                                    });
                                }
                                Err(e) => {
                                    tracing::warn!("skipping skill at {:?}: {}", path, e);
                                }
                            }
                        }
                    }
                }
            }
        }
        skills
    }

    fn load(&self, skill_name: &str, skills: &[DiscoveredSkill]) -> Result<String, AgentError> {
        let skill = skills
            .iter()
            .find(|s| s.metadata.name == skill_name)
            .ok_or_else(|| AgentError::SkillParseError {
                name: skill_name.to_string(),
                reason: "skill not found".to_string(),
            })?;
        skill.load_content()
    }
}

/// Parse skill frontmatter from a SKILL.md file.
///
/// Reads only the YAML frontmatter (between `---` delimiters) to extract
/// `name` and `description`. The full file content is NOT loaded.
fn parse_skill_metadata(skill_md: &Path) -> Result<SkillMetadata, AgentError> {
    let content = std::fs::read_to_string(skill_md).map_err(|e| AgentError::SkillParseError {
        name: skill_md.display().to_string(),
        reason: format!("failed to read: {}", e),
    })?;

    let (name, description) = extract_frontmatter_fields(&content, skill_md)?;

    Ok(SkillMetadata {
        name,
        description,
        path: skill_md.to_path_buf(),
    })
}

fn extract_frontmatter_fields(content: &str, path: &Path) -> Result<(String, String), AgentError> {
    let path_str = path.display().to_string();

    let mut lines = content.lines();

    // Skip optional empty lines
    let first = lines.next();
    if first != Some("---") {
        return Err(AgentError::SkillParseError {
            name: path_str,
            reason: "missing frontmatter delimiter '---'".to_string(),
        });
    }

    let mut name = None;
    let mut description = None;

    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("name:") {
            name = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("description:") {
            description = Some(value.trim().to_string());
        }
    }

    let name = name.ok_or_else(|| AgentError::SkillParseError {
        name: path_str.clone(),
        reason: "missing 'name' field in frontmatter".to_string(),
    })?;

    let description = description.unwrap_or_default();

    Ok((name, description))
}

/// Fastskill-backed skill provider compiled only when the `fastskill` feature is enabled.
///
/// The backend initializes a fastskill-core service per discovery root, lets the
/// service index local skills, and maps the indexed definitions into the agent's
/// existing skill metadata shape.
#[cfg(feature = "fastskill")]
#[derive(Debug)]
pub struct FastskillSkillBackend {
    runtime: tokio::runtime::Runtime,
}

#[cfg(feature = "fastskill")]
impl FastskillSkillBackend {
    /// Construct from AgentConfig. Returns `AgentError::FastskillInit` on failure.
    pub fn new(config: &crate::config::AgentConfig) -> Result<Self, AgentError> {
        for root in &config.skills_dirs {
            if root.exists() && !root.is_dir() {
                return Err(AgentError::FastskillInit {
                    reason: format!("skill root is not a directory: {}", root.display()),
                });
            }
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| AgentError::FastskillInit {
                reason: e.to_string(),
            })?;

        Ok(Self { runtime })
    }

    async fn discover_root(root: PathBuf) -> Result<Vec<DiscoveredSkill>, AgentError> {
        use fastskill_core::{FastSkillService, ServiceConfig};

        let config = ServiceConfig {
            skill_storage_path: root,
            embedding: None,
            ..Default::default()
        };
        let mut service =
            FastSkillService::new(config)
                .await
                .map_err(|e| AgentError::FastskillResolve {
                    name: "*".to_string(),
                    reason: e.to_string(),
                })?;
        service
            .initialize()
            .await
            .map_err(|e| AgentError::FastskillResolve {
                name: "*".to_string(),
                reason: e.to_string(),
            })?;

        let definitions = service
            .skill_manager()
            .list_skills(None)
            .await
            .map_err(|e| AgentError::FastskillResolve {
                name: "*".to_string(),
                reason: e.to_string(),
            })?;

        Ok(definitions
            .into_iter()
            .map(|skill| {
                let content_path = skill.skill_file.clone();
                DiscoveredSkill {
                    metadata: SkillMetadata {
                        name: skill.name,
                        description: skill.description,
                        path: content_path.clone(),
                    },
                    content_path,
                }
            })
            .collect())
    }
}

#[cfg(feature = "fastskill")]
impl SkillProvider for FastskillSkillBackend {
    fn discover(&self, roots: &[PathBuf]) -> Vec<DiscoveredSkill> {
        if roots.is_empty() {
            return Vec::new();
        }
        let mut discovered = Vec::new();
        for root in roots {
            match self.runtime.block_on(Self::discover_root(root.clone())) {
                Ok(mut skills) => discovered.append(&mut skills),
                Err(e) => tracing::warn!("fastskill resolver skipped {}: {}", root.display(), e),
            }
        }
        discovered
    }

    fn load(&self, skill_name: &str, skills: &[DiscoveredSkill]) -> Result<String, AgentError> {
        // First-match policy: same as FilesystemSkillProvider::load
        let skill = skills
            .iter()
            .find(|s| s.metadata.name == skill_name)
            .ok_or_else(|| AgentError::SkillParseError {
                name: skill_name.to_string(),
                reason: "skill not found".to_string(),
            })?;
        std::fs::read_to_string(&skill.content_path).map_err(|e| AgentError::FastskillResolve {
            name: skill_name.to_string(),
            reason: format!("failed to read resolved skill content: {}", e),
        })
    }
}

/// Discover skills from the given root directories.
pub fn discover_skills(roots: &[PathBuf]) -> Vec<DiscoveredSkill> {
    let provider = FilesystemSkillProvider;
    provider.discover(roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_skill_dir(root: &Path, dir_name: &str, name: &str, description: &str) {
        let skill_dir = root.join(dir_name);
        fs::create_dir_all(&skill_dir).unwrap();
        let content = format!(
            "---\nname: {}\ndescription: {}\n---\n\n# Skill Content\n\nFull skill body here.",
            name, description
        );
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn test_skills_discovery_loads_metadata_only() {
        let tmp = TempDir::new().unwrap();
        create_skill_dir(tmp.path(), "my-skill", "my-skill", "Does something useful");

        let skills = discover_skills(&[tmp.path().to_path_buf()]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].metadata.name, "my-skill");
        assert_eq!(skills[0].metadata.description, "Does something useful");
        // Full content is NOT in metadata - only the file path is stored
        // The content is loaded lazily via load_content()
    }

    #[test]
    fn test_read_skill_loads_full_content() {
        let tmp = TempDir::new().unwrap();
        create_skill_dir(tmp.path(), "my-skill", "my-skill", "A useful skill");

        let skills = discover_skills(&[tmp.path().to_path_buf()]);
        assert_eq!(skills.len(), 1);

        let content = skills[0].load_content().unwrap();
        assert!(
            content.contains("Full skill body here."),
            "full content should be loaded"
        );
    }

    #[test]
    fn test_skills_parse_error() {
        let tmp = TempDir::new().unwrap();
        let skill_dir = tmp.path().join("bad-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        // Write a SKILL.md without valid frontmatter
        fs::write(skill_dir.join("SKILL.md"), "No frontmatter here").unwrap();

        let skills = discover_skills(&[tmp.path().to_path_buf()]);
        // Bad skills are skipped (logged as warning)
        assert_eq!(skills.len(), 0);
    }

    #[test]
    fn test_skills_parse_error_missing_name() {
        let tmp = TempDir::new().unwrap();
        let content = "---\ndescription: Missing name\n---\n\nBody.";
        let path = tmp.path().join("SKILL.md");
        fs::write(&path, content).unwrap();

        let result = extract_frontmatter_fields(content, &path);
        assert!(result.is_err());
        match result.unwrap_err() {
            AgentError::SkillParseError { reason, .. } => {
                assert!(reason.contains("name"), "error should mention 'name'");
            }
            _ => panic!("expected SkillParseError"),
        }
    }

    #[test]
    fn test_discover_multiple_skills() {
        let tmp = TempDir::new().unwrap();
        create_skill_dir(tmp.path(), "skill-a", "skill-a", "First skill");
        create_skill_dir(tmp.path(), "skill-b", "skill-b", "Second skill");
        // Non-skill directory (no SKILL.md)
        fs::create_dir_all(tmp.path().join("not-a-skill")).unwrap();

        let skills = discover_skills(&[tmp.path().to_path_buf()]);
        assert_eq!(skills.len(), 2);
        let names: Vec<_> = skills.iter().map(|s| s.metadata.name.as_str()).collect();
        assert!(names.contains(&"skill-a"));
        assert!(names.contains(&"skill-b"));
    }

    #[cfg(feature = "fastskill")]
    fn make_fastskill_config(tmp: &TempDir) -> crate::config::AgentConfig {
        crate::config::AgentConfig {
            model: "test-model".to_string(),
            base_url: "http://localhost".to_string(),
            api_key: "test-key".to_string(),
            stream: false,
            max_iterations: 1,
            max_subagent_depth: 0,
            context_budget_tokens: 1000,
            workdir: tmp.path().to_path_buf(),
            allowed_roots: vec![],
            skills_dirs: vec![],
            agents_md_path: None,
            timeout_secs: 10,
            connect_timeout_secs: 5,
            session_persona: None,
            session_agents: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn test_duplicate_skill_name_uses_first_match() {
        let tmp = TempDir::new().unwrap();

        let root1 = tmp.path().join("root1");
        let root2 = tmp.path().join("root2");

        let dir1 = root1.join("dup-skill");
        fs::create_dir_all(&dir1).unwrap();
        fs::write(
            dir1.join("SKILL.md"),
            "---\nname: dup-skill\ndescription: First version\n---\n\nFirst content",
        )
        .unwrap();

        let dir2 = root2.join("dup-skill");
        fs::create_dir_all(&dir2).unwrap();
        fs::write(
            dir2.join("SKILL.md"),
            "---\nname: dup-skill\ndescription: Second version\n---\n\nSecond content",
        )
        .unwrap();

        let provider = FilesystemSkillProvider;
        // Discover from root1 first so it appears first in the slice
        let skills = provider.discover(&[root1, root2]);

        assert_eq!(
            skills.len(),
            2,
            "both duplicate skills should be discovered"
        );

        // First-match policy: root1's skill is first in slice, so its content is returned
        let loaded = provider.load("dup-skill", &skills).unwrap();
        assert!(
            loaded.contains("First content"),
            "first-match policy: should return first entry in slice order"
        );
    }

    #[cfg(feature = "fastskill")]
    #[test]
    fn test_fastskill_init_failure_emits_init_error() {
        let tmp = TempDir::new().unwrap();
        let skill_root_file = tmp.path().join("not-a-directory");
        fs::write(&skill_root_file, "not a directory").unwrap();
        let mut config = make_fastskill_config(&tmp);
        config.skills_dirs = vec![skill_root_file];
        let result = FastskillSkillBackend::new(&config);
        assert!(
            matches!(result, Err(crate::errors::AgentError::FastskillInit { .. })),
            "invalid skill root should return FastskillInit error"
        );
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("E_AIKIT_FASTSKILL_INIT"),
            "error message should include E_AIKIT_FASTSKILL_INIT code"
        );
    }

    #[cfg(feature = "fastskill")]
    #[test]
    fn test_fastskill_resolve_error_on_load_failure() {
        let tmp = TempDir::new().unwrap();
        let config = make_fastskill_config(&tmp);
        let backend = FastskillSkillBackend::new(&config).unwrap();
        let skills = vec![DiscoveredSkill {
            metadata: SkillMetadata {
                name: "missing-skill".to_string(),
                description: "Missing content".to_string(),
                path: tmp.path().join("missing").join("SKILL.md"),
            },
            content_path: tmp.path().join("missing").join("SKILL.md"),
        }];

        let result = backend.load("missing-skill", &skills);
        assert!(
            matches!(
                result,
                Err(crate::errors::AgentError::FastskillResolve { .. })
            ),
            "backend should return FastskillResolve when resolved content cannot be read"
        );
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("E_AIKIT_FASTSKILL_RESOLVE"),
            "error message should include E_AIKIT_FASTSKILL_RESOLVE code"
        );
    }
}
