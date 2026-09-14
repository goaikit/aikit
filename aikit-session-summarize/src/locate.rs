//! Where sessions are: the adapters to scan, built from `--tool` filters and
//! `--path` overrides.
//!
//! Without a path, each adapter resolves its own roots (override env such as
//! `CLAUDE_HOME`, then every resolved `$HOME`). With `<tool>=<dir>` the root
//! is bound to that one adapter. A bare `<dir>` is offered to every compiled
//! adapter, and a file is claimed by the shape of its name, because two
//! adapters both claim `*.jsonl` under a root they were handed.

use std::path::{Path, PathBuf};

use aikit_session_capture::{Adapter, ToolKind};
use async_trait::async_trait;

/// One `--path` argument as parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationSpec {
    /// `Some` when written as `<tool>=<dir>`.
    pub tool: Option<ToolKind>,
    pub path: PathBuf,
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum LocateError {
    #[error("unknown tool '{0}' (expected claude_code, codex or open_code)")]
    UnknownTool(String),
    #[error("empty path in '{0}'")]
    EmptyPath(String),
}

/// Parse a tool name as `session sync` accepts it: `claude_code` (also
/// `claudecode`, `claude`), `codex`, `open_code` (also `opencode`).
pub fn parse_tool_kind(s: &str) -> Option<ToolKind> {
    match s.trim() {
        "claude_code" | "claudecode" | "claude" => Some(ToolKind::ClaudeCode),
        "codex" => Some(ToolKind::Codex),
        "open_code" | "opencode" => Some(ToolKind::OpenCode),
        _ => None,
    }
}

/// Parse `[<tool>=]<dir>`. A Windows drive letter (`C:\...`) is not a tool
/// prefix because the separator is `=`, not `:`.
pub fn parse_location(raw: &str) -> Result<LocationSpec, LocateError> {
    let raw = raw.trim();
    if let Some((tool, path)) = raw.split_once('=') {
        let kind = parse_tool_kind(tool).ok_or_else(|| LocateError::UnknownTool(tool.into()))?;
        if path.is_empty() {
            return Err(LocateError::EmptyPath(raw.into()));
        }
        return Ok(LocationSpec {
            tool: Some(kind),
            path: PathBuf::from(path),
        });
    }
    if raw.is_empty() {
        return Err(LocateError::EmptyPath(raw.into()));
    }
    Ok(LocationSpec {
        tool: None,
        path: PathBuf::from(raw),
    })
}

/// Every tool this build has an adapter for, in registration order.
pub fn compiled_tools() -> Vec<ToolKind> {
    [
        cfg!(feature = "claudecode").then_some(ToolKind::ClaudeCode),
        cfg!(feature = "codex").then_some(ToolKind::Codex),
        cfg!(feature = "opencode").then_some(ToolKind::OpenCode),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Whether a file's name has the shape the tool writes. Used only when a
/// bare `--path` is offered to every adapter.
pub fn name_shape_matches(kind: ToolKind, path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let stem = name
        .rsplit_once('.')
        .map(|(s, _)| s.to_string())
        .unwrap_or_default();
    match kind {
        ToolKind::ClaudeCode => {
            is_uuid(&stem)
                || (stem.starts_with("agent-")
                    && path.components().any(|c| c.as_os_str() == "subagents"))
        }
        ToolKind::Codex => stem.starts_with("rollout-") || stem.starts_with("session-"),
        ToolKind::OpenCode => name == "opencode.db" || name == "opencode.db-wal",
        _ => false,
    }
}

fn is_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(&parts)
            .all(|(n, p)| p.len() == *n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// An adapter that also requires the file name shape of its tool. Wraps a
/// concrete adapter given a shared root.
struct ShapedAdapter {
    inner: Box<dyn Adapter>,
}

#[async_trait]
impl Adapter for ShapedAdapter {
    fn kind(&self) -> ToolKind {
        self.inner.kind()
    }
    fn watch_paths(&self) -> Vec<PathBuf> {
        self.inner.watch_paths()
    }
    fn is_session_file(&self, path: &Path) -> bool {
        name_shape_matches(self.kind(), path) && self.inner.is_session_file(path)
    }
    async fn parse_session_file(
        &self,
        path: &Path,
        from_offset: u64,
    ) -> Result<aikit_session_capture::ParseResult, aikit_session_capture::AdapterError> {
        self.inner.parse_session_file(path, from_offset).await
    }
}

/// Build one adapter for `kind`, rooted at `root` when given.
fn build_adapter(kind: ToolKind, root: Option<&Path>) -> Option<Box<dyn Adapter>> {
    match kind {
        #[cfg(feature = "claudecode")]
        ToolKind::ClaudeCode => {
            use aikit_session_capture::claudecode::ClaudeCodeAdapter;
            let a = ClaudeCodeAdapter::new();
            Some(Box::new(match root {
                Some(r) => a.with_override_root(r.to_path_buf()),
                None => a,
            }))
        }
        #[cfg(feature = "codex")]
        ToolKind::Codex => {
            use aikit_session_capture::codex::CodexAdapter;
            let a = CodexAdapter::new();
            Some(Box::new(match root {
                Some(r) => a.with_override_root(r.to_path_buf()),
                None => a,
            }))
        }
        #[cfg(feature = "opencode")]
        ToolKind::OpenCode => {
            use aikit_session_capture::opencode::OpenCodeAdapter;
            let a = OpenCodeAdapter::new();
            Some(Box::new(match root {
                Some(r) => a.with_override_roots(vec![r.to_path_buf()]),
                None => a,
            }))
        }
        _ => None,
    }
}

/// The adapters to scan.
///
/// - `tools`: `None` means every compiled tool; `Some` restricts to those.
/// - `locations`: empty means each adapter's own roots. Otherwise one
///   adapter per (tool, root): a bound root goes to its tool only (and is
///   dropped when `tools` excludes it); a bare root goes to every selected
///   tool, shape-checked.
pub fn adapters_for(
    tools: Option<&[ToolKind]>,
    locations: &[LocationSpec],
) -> Vec<Box<dyn Adapter>> {
    let selected: Vec<ToolKind> = match tools {
        Some(list) => compiled_tools()
            .into_iter()
            .filter(|k| list.contains(k))
            .collect(),
        None => compiled_tools(),
    };
    if locations.is_empty() {
        return selected
            .into_iter()
            .filter_map(|k| build_adapter(k, None))
            .collect();
    }
    let mut out: Vec<Box<dyn Adapter>> = Vec::new();
    for loc in locations {
        match loc.tool {
            Some(kind) => {
                if selected.contains(&kind) {
                    out.extend(build_adapter(kind, Some(&loc.path)));
                }
            }
            None => {
                for kind in &selected {
                    if let Some(inner) = build_adapter(*kind, Some(&loc.path)) {
                        out.push(Box::new(ShapedAdapter { inner }));
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tool_aliases_and_locations() {
        assert_eq!(parse_tool_kind("claude"), Some(ToolKind::ClaudeCode));
        assert_eq!(parse_tool_kind("open_code"), Some(ToolKind::OpenCode));
        assert_eq!(parse_tool_kind("gemini"), None);
        let bound = parse_location("codex=/scratch/codex").unwrap();
        assert_eq!(bound.tool, Some(ToolKind::Codex));
        assert_eq!(bound.path, PathBuf::from("/scratch/codex"));
        let bare = parse_location("/scratch").unwrap();
        assert_eq!(bare.tool, None);
        assert!(matches!(
            parse_location("nope=/x"),
            Err(LocateError::UnknownTool(_))
        ));
        assert!(matches!(
            parse_location("codex="),
            Err(LocateError::EmptyPath(_))
        ));
    }

    #[test]
    fn name_shapes_disambiguate_jsonl_files() {
        let claude = Path::new("/r/p/550e8400-e29b-41d4-a716-446655440000.jsonl");
        let sub = Path::new("/r/p/550e8400-e29b-41d4-a716-446655440000/subagents/agent-1.jsonl");
        let codex = Path::new("/r/2026/09/rollout-2026-09-01T10-00-00-abc.jsonl");
        assert!(name_shape_matches(ToolKind::ClaudeCode, claude));
        assert!(name_shape_matches(ToolKind::ClaudeCode, sub));
        assert!(!name_shape_matches(ToolKind::ClaudeCode, codex));
        assert!(name_shape_matches(ToolKind::Codex, codex));
        assert!(!name_shape_matches(ToolKind::Codex, claude));
        assert!(name_shape_matches(
            ToolKind::OpenCode,
            Path::new("/r/opencode.db")
        ));
    }

    #[cfg(all(feature = "claudecode", feature = "codex"))]
    #[test]
    fn bare_path_is_offered_to_every_selected_tool_with_shape_check() {
        let dir = tempfile::tempdir().unwrap();
        let claude_file = dir
            .path()
            .join("550e8400-e29b-41d4-a716-446655440000.jsonl");
        let codex_file = dir.path().join("rollout-1.jsonl");
        std::fs::write(&claude_file, "").unwrap();
        std::fs::write(&codex_file, "").unwrap();
        let locs = vec![parse_location(dir.path().to_str().unwrap()).unwrap()];

        let all = adapters_for(None, &locs);
        let kinds: Vec<ToolKind> = all.iter().map(|a| a.kind()).collect();
        assert!(kinds.contains(&ToolKind::ClaudeCode) && kinds.contains(&ToolKind::Codex));
        for a in &all {
            match a.kind() {
                ToolKind::ClaudeCode => {
                    assert!(a.is_session_file(&claude_file));
                    assert!(!a.is_session_file(&codex_file));
                }
                ToolKind::Codex => {
                    assert!(a.is_session_file(&codex_file));
                    assert!(!a.is_session_file(&claude_file));
                }
                _ => {}
            }
        }

        // A bound root skips the shape check and honours the tool filter.
        let bound = vec![LocationSpec {
            tool: Some(ToolKind::Codex),
            path: dir.path().to_path_buf(),
        }];
        let only = adapters_for(Some(&[ToolKind::Codex]), &bound);
        assert_eq!(only.len(), 1);
        assert!(
            only[0].is_session_file(&claude_file),
            "bound root: no shape check"
        );
        assert!(adapters_for(Some(&[ToolKind::ClaudeCode]), &bound).is_empty());
    }
}
