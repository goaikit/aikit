//! Claude Code adapter: parses `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl`.
//!
//! See spec 010 §12.1. The adapter reads the file from a byte offset,
//! streams line-by-line, and emits normalized `ToolEvent` / `TokenEvent` /
//! `CacheObservation` rows.
//!
//! Watch-path resolution priority (mirrors observer's `codex/adapter.go:60`):
//!   1. Explicit override (CLI flag / config / test fixture).
//!   2. `CLAUDE_HOME` env var.
//!   3. Crossmount-resolved `$HOME/.claude/projects`.

mod transcript;

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::adapter::{Adapter, AdapterError, ParseResult};
use crate::homes::HomeResolver;
use crate::models::ToolKind;
use crate::scrub::SecretScrubber;

pub struct ClaudeCodeAdapter {
    scrubber: SecretScrubber,
    homes: Vec<crate::homes::HomeRoot>,
    override_root: Option<PathBuf>,
}

impl ClaudeCodeAdapter {
    /// Production constructor: uses `DefaultHomeResolver` + default scrubber.
    pub fn new() -> Self {
        Self {
            scrubber: SecretScrubber::default(),
            homes: crate::homes::DefaultHomeResolver.homes(),
            override_root: None,
        }
    }

    /// Test / config constructor: caller supplies a scrubber + home resolver.
    pub fn with(scrubber: SecretScrubber, homes: Vec<crate::homes::HomeRoot>) -> Self {
        Self {
            scrubber,
            homes,
            override_root: None,
        }
    }

    /// Explicit watch-root override. When set, `watch_paths()` returns only
    /// this path — env var and crossmount expansion are suppressed. Used by
    /// tests and the `[adapter.claudecode] root = "..."` config field.
    pub fn with_override_root(mut self, root: PathBuf) -> Self {
        self.override_root = Some(root);
        self
    }
}

impl Default for ClaudeCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Adapter for ClaudeCodeAdapter {
    fn kind(&self) -> ToolKind {
        ToolKind::ClaudeCode
    }

    fn watch_paths(&self) -> Vec<PathBuf> {
        if let Some(root) = &self.override_root {
            return vec![root.clone()];
        }
        // CLAUDE_HOME (single explicit path; suppresses crossmount expansion).
        if let Ok(env_home) = std::env::var("CLAUDE_HOME") {
            if !env_home.is_empty() {
                return vec![PathBuf::from(env_home).join("projects")];
            }
        }
        // Default: every resolved home gets a `.claude/projects` candidate.
        // The registry's `detected()` filter skips the ones that don't exist.
        self.homes
            .iter()
            .map(|h| h.path.join(".claude").join("projects"))
            .collect()
    }

    fn is_session_file(&self, path: &Path) -> bool {
        // Claude Code session files are `<session-uuid>.jsonl` under a
        // watch root. Subagent transcripts live under `<session-uuid>/
        // subagents/agent-*.jsonl` and share the parent session_id — they
        // also match this predicate.
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        if !name.ends_with(".jsonl") {
            return false;
        }
        // Constrain to a watch path so a random `.jsonl` elsewhere on disk
        // doesn't get claimed.
        let abs = path.to_path_buf();
        self.watch_paths().iter().any(|root| abs.starts_with(root))
    }

    async fn parse_session_file(
        &self,
        path: &Path,
        from_offset: u64,
    ) -> Result<ParseResult, AdapterError> {
        // Read the whole file. Phase 2 simplicity: the file is read into
        // memory; for very large transcripts a streaming BufReader would
        // reduce peak memory, but observer's transcripts are bounded (~MB
        // range, the adapter.go comment at line 437 calls this out). A future
        // optimization can swap to memmap or BufRead without changing the
        // trait contract.
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|source| AdapterError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        transcript::parse(path, &bytes, from_offset, &self.scrubber)
    }
}
