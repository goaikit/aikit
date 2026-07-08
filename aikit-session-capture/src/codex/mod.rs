//! Codex CLI adapter: parses `~/.codex/sessions/rollout-*.jsonl`.
//!
//! See spec 010 §12.2. Watch-path resolution priority mirrors Claude Code:
//!   1. Explicit override.
//!   2. `CODEX_HOME` env var (single explicit path; suppresses crossmount).
//!   3. Crossmount-resolved `$HOME/.codex/sessions`.

mod transcript;

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::adapter::{Adapter, AdapterError, ParseResult};
use crate::homes::HomeResolver;
use crate::models::ToolKind;
use crate::scrub::SecretScrubber;

pub struct CodexAdapter {
    scrubber: SecretScrubber,
    homes: Vec<crate::homes::HomeRoot>,
    override_root: Option<PathBuf>,
}

impl CodexAdapter {
    pub fn new() -> Self {
        Self {
            scrubber: SecretScrubber::default(),
            homes: crate::homes::DefaultHomeResolver.homes(),
            override_root: None,
        }
    }

    pub fn with(scrubber: SecretScrubber, homes: Vec<crate::homes::HomeRoot>) -> Self {
        Self {
            scrubber,
            homes,
            override_root: None,
        }
    }

    pub fn with_override_root(mut self, root: PathBuf) -> Self {
        self.override_root = Some(root);
        self
    }
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Adapter for CodexAdapter {
    fn kind(&self) -> ToolKind {
        ToolKind::Codex
    }

    fn watch_paths(&self) -> Vec<PathBuf> {
        if let Some(root) = &self.override_root {
            return vec![root.clone()];
        }
        // CODEX_HOME — single explicit path; suppresses crossmount expansion
        // (mirrors observer's codex/adapter.go:60).
        if let Ok(env_home) = std::env::var("CODEX_HOME") {
            if !env_home.is_empty() {
                return vec![PathBuf::from(env_home).join("sessions")];
            }
        }
        self.homes
            .iter()
            .map(|h| h.path.join(".codex").join("sessions"))
            .collect()
    }

    fn is_session_file(&self, path: &Path) -> bool {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        // Codex session files are `rollout-*.jsonl` under a watch root.
        // Be permissive on the prefix (some builds use `session-*.jsonl`)
        // but require the `.jsonl` suffix + watch-root constraint.
        if !name.ends_with(".jsonl") {
            return false;
        }
        let abs = path.to_path_buf();
        self.watch_paths().iter().any(|root| abs.starts_with(root))
    }

    async fn parse_session_file(
        &self,
        path: &Path,
        from_offset: u64,
    ) -> Result<ParseResult, AdapterError> {
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|source| AdapterError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        transcript::parse(path, &bytes, from_offset, &self.scrubber)
    }
}
