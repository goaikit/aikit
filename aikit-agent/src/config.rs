use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::agent_definition::AgentPersona;
use crate::errors::AgentError;
use crate::host_tools::HostToolProvider;
use crate::llm::openai_compat::resolve_api_key;

#[derive(Clone)]
pub struct AgentConfig {
    pub model: String,
    pub base_url: String,
    pub api_key: String,
    pub stream: bool,
    pub max_iterations: u32,
    pub max_subagent_depth: u32,
    pub context_budget_tokens: u64,
    pub workdir: PathBuf,
    pub allowed_roots: Vec<PathBuf>,
    pub skills_dirs: Vec<PathBuf>,
    pub agents_md_path: Option<PathBuf>,
    pub timeout_secs: u64,
    pub connect_timeout_secs: u64,
    /// Session persona to apply as main-thread defaults (set by CLI after from_env).
    pub session_persona: Option<AgentPersona>,
    /// Ephemeral per-process agent definitions (set by CLI after from_env).
    pub session_agents: HashMap<String, AgentPersona>,
    /// Optional host tool provider injected by the embedder.
    pub host_tool_provider: Option<Arc<dyn HostToolProvider>>,
}

impl std::fmt::Debug for AgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentConfig")
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field("stream", &self.stream)
            .field("max_iterations", &self.max_iterations)
            .field("max_subagent_depth", &self.max_subagent_depth)
            .field("workdir", &self.workdir)
            .field(
                "host_tool_provider",
                &self
                    .host_tool_provider
                    .as_ref()
                    .map(|_| "<HostToolProvider>"),
            )
            .finish_non_exhaustive()
    }
}

impl AgentConfig {
    /// Resolve configuration from environment variables and a workdir.
    ///
    /// Returns `Err(AgentError::NoApiKey)` if no API key is found.
    pub fn from_env(
        workdir: PathBuf,
        stream: bool,
        model: Option<String>,
    ) -> Result<Self, AgentError> {
        let base_url = std::env::var("AIKIT_LLM_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

        let api_key = resolve_api_key(None).map_err(|e| AgentError::NoApiKey {
            checked: e.to_string(),
        })?;

        let model = model
            .or_else(|| std::env::var("AIKIT_MODEL").ok())
            .unwrap_or_else(|| "gpt-4o".to_string());

        let stream = stream
            || std::env::var("AIKIT_STREAM")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false);

        let max_iterations = std::env::var("AIKIT_MAX_ITERATIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10u32);

        let max_subagent_depth = std::env::var("AIKIT_MAX_SUBAGENT_DEPTH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2u32);

        let context_budget_tokens = std::env::var("AIKIT_CONTEXT_BUDGET_TOKENS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(12000u64);

        let agents_md_path = {
            let agents_md = workdir.join("AGENTS.md");
            let claude_md = workdir.join("CLAUDE.md");
            if agents_md.exists() {
                Some(agents_md)
            } else if claude_md.exists() {
                Some(claude_md)
            } else {
                None
            }
        };

        let mut skills_dirs = Vec::new();
        let local_skills = workdir.join(".aikit").join("skills");
        if local_skills.exists() {
            skills_dirs.push(local_skills);
        }
        if let Ok(extra) = std::env::var("AIKIT_SKILLS_DIR") {
            let path = PathBuf::from(extra);
            if path.exists() {
                skills_dirs.push(path);
            }
        }

        let allowed_roots = vec![workdir.clone()];

        Ok(Self {
            model,
            base_url,
            api_key,
            stream,
            max_iterations,
            max_subagent_depth,
            context_budget_tokens,
            workdir,
            allowed_roots,
            skills_dirs,
            agents_md_path,
            timeout_secs: 60,
            connect_timeout_secs: 10,
            session_persona: None,
            session_agents: HashMap::new(),
            host_tool_provider: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::TempDir;

    #[test]
    fn test_from_env_no_api_key() {
        let _guard = crate::test_support::env_lock();
        env::remove_var("OPENAI_API_KEY");
        env::remove_var("AIKIT_API_KEY");
        let result = AgentConfig::from_env(std::env::current_dir().unwrap(), false, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            AgentError::NoApiKey { .. } => {}
            e => panic!("expected NoApiKey, got {:?}", e),
        }
    }

    #[test]
    fn test_from_env_with_api_key() {
        let _guard = crate::test_support::env_lock();
        env::set_var("OPENAI_API_KEY", "test-key");
        env::set_var("AIKIT_LLM_URL", "http://test-server/v1");
        let config = AgentConfig::from_env(std::env::current_dir().unwrap(), false, None).unwrap();
        assert_eq!(config.api_key, "test-key");
        assert_eq!(config.base_url, "http://test-server/v1");
        env::remove_var("OPENAI_API_KEY");
        env::remove_var("AIKIT_LLM_URL");
    }

    #[test]
    fn test_from_env_model_override() {
        let _guard = crate::test_support::env_lock();
        env::set_var("OPENAI_API_KEY", "test-key");
        env::set_var("AIKIT_MODEL", "gpt-3.5-turbo");
        let config = AgentConfig::from_env(std::env::current_dir().unwrap(), false, None).unwrap();
        assert_eq!(config.model, "gpt-3.5-turbo");
        env::remove_var("OPENAI_API_KEY");
        env::remove_var("AIKIT_MODEL");
    }

    #[test]
    fn test_from_env_explicit_model_overrides_env() {
        let _guard = crate::test_support::env_lock();
        env::set_var("OPENAI_API_KEY", "test-key");
        env::set_var("AIKIT_MODEL", "gpt-3.5-turbo");
        let config = AgentConfig::from_env(
            std::env::current_dir().unwrap(),
            false,
            Some("gpt-4o-mini".to_string()),
        )
        .unwrap();
        assert_eq!(config.model, "gpt-4o-mini");
        env::remove_var("OPENAI_API_KEY");
        env::remove_var("AIKIT_MODEL");
    }

    #[test]
    fn test_from_env_prefers_agents_md() {
        let _guard = crate::test_support::env_lock();
        env::set_var("OPENAI_API_KEY", "test-key");
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "agents content").unwrap();
        std::fs::write(tmp.path().join("CLAUDE.md"), "claude content").unwrap();
        let config = AgentConfig::from_env(tmp.path().to_path_buf(), false, None).unwrap();
        assert_eq!(config.agents_md_path, Some(tmp.path().join("AGENTS.md")));
        env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn test_from_env_falls_back_to_claude_md() {
        let _guard = crate::test_support::env_lock();
        env::set_var("OPENAI_API_KEY", "test-key");
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("CLAUDE.md"), "claude content").unwrap();
        let config = AgentConfig::from_env(tmp.path().to_path_buf(), false, None).unwrap();
        assert_eq!(config.agents_md_path, Some(tmp.path().join("CLAUDE.md")));
        env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn test_from_env_no_md_files_yields_none() {
        let _guard = crate::test_support::env_lock();
        env::set_var("OPENAI_API_KEY", "test-key");
        let tmp = TempDir::new().unwrap();
        let config = AgentConfig::from_env(tmp.path().to_path_buf(), false, None).unwrap();
        assert_eq!(config.agents_md_path, None);
        env::remove_var("OPENAI_API_KEY");
    }
}
