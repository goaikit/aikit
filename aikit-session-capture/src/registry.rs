//! Adapter registry with auto-detection based on watch-path existence.
//!
//! See spec 010 §7. Mirrors `superbased-observer/internal/adapter/registry.go`.
//!
//! The nil-vs-empty allow-list distinction is load-bearing for TOML configs:
//! `None` = all adapters considered; `Some(&[])` = none; `Some(&[k])` =
//! restrict to those kinds.

use std::path::Path;

use crate::adapter::Adapter;
use crate::models::ToolKind;

/// Holds all known adapters, in registration order.
#[derive(Default)]
pub struct Registry {
    adapters: Vec<Box<dyn Adapter>>,
}

impl Registry {
    /// Empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an adapter. Registration order is preserved by [`Registry::all`].
    pub fn register(&mut self, adapter: Box<dyn Adapter>) {
        self.adapters.push(adapter);
    }

    /// Returns the first adapter matching `kind`, or `None`.
    pub fn get(&self, kind: ToolKind) -> Option<&dyn Adapter> {
        self.adapters
            .iter()
            .find(|a| a.kind() == kind)
            .map(|a| a.as_ref())
    }

    /// Every registered adapter, in registration order.
    pub fn all(&self) -> Vec<&dyn Adapter> {
        self.adapters.iter().map(|a| a.as_ref()).collect()
    }

    /// Filter to adapters whose `watch_paths()` include at least one
    /// directory that currently exists. Allow-list semantics (spec §7):
    ///
    /// - `allow == None`           → no filter (all adapters considered).
    /// - `allow == Some(empty)`    → filter to *zero* adapters. This is the
    ///   explicit user-intent case: a config `[adapter] enabled = []`
    ///   disables passive capture rather than silently falling through.
    /// - `allow == Some(non-empty)`→ restrict to the named kinds.
    pub fn detected(&self, allow: Option<&[ToolKind]>) -> Vec<&dyn Adapter> {
        if let Some([]) = allow {
            return Vec::new();
        }
        let allow_set: Option<std::collections::HashSet<ToolKind>> =
            allow.map(|ks| ks.iter().copied().collect());
        self.adapters
            .iter()
            .filter(|a| {
                if let Some(set) = &allow_set {
                    if !set.contains(&a.kind()) {
                        return false;
                    }
                }
                any_dir_exists(&a.watch_paths())
            })
            .map(|a| a.as_ref())
            .collect()
    }
}

fn any_dir_exists(paths: &[std::path::PathBuf]) -> bool {
    paths.iter().any(|p| p.is_dir())
}

// Lint: `Path` import kept for the `any_dir_exists` signature readability;
// the function uses `is_dir()` which is on `PathBuf`/`Path` interchangeably.
#[allow(clippy::needless_borrow)]
fn _path_typecheck(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::path::PathBuf;

    struct FakeAdapter {
        kind: ToolKind,
        paths: Vec<PathBuf>,
    }

    #[async_trait]
    impl Adapter for FakeAdapter {
        fn kind(&self) -> ToolKind {
            self.kind
        }
        fn watch_paths(&self) -> Vec<PathBuf> {
            self.paths.clone()
        }
        fn is_session_file(&self, _path: &Path) -> bool {
            false
        }
        async fn parse_session_file(
            &self,
            _path: &Path,
            _from_offset: u64,
        ) -> Result<crate::ParseResult, crate::AdapterError> {
            Ok(crate::ParseResult::default())
        }
    }

    #[test]
    fn allow_none_considers_all() {
        let mut r = Registry::new();
        r.register(Box::new(FakeAdapter {
            kind: ToolKind::ClaudeCode,
            paths: vec![std::env::temp_dir()],
        }));
        r.register(Box::new(FakeAdapter {
            kind: ToolKind::Codex,
            paths: vec![std::env::temp_dir()],
        }));
        assert_eq!(r.detected(None).len(), 2);
    }

    #[test]
    fn allow_empty_filters_to_zero() {
        let mut r = Registry::new();
        r.register(Box::new(FakeAdapter {
            kind: ToolKind::ClaudeCode,
            paths: vec![std::env::temp_dir()],
        }));
        // The explicit "disable everything" case — not a fallback to "all".
        assert_eq!(r.detected(Some(&[])).len(), 0);
    }

    #[test]
    fn allow_specific_restricts() {
        let mut r = Registry::new();
        r.register(Box::new(FakeAdapter {
            kind: ToolKind::ClaudeCode,
            paths: vec![std::env::temp_dir()],
        }));
        r.register(Box::new(FakeAdapter {
            kind: ToolKind::Codex,
            paths: vec![std::env::temp_dir()],
        }));
        let got = r.detected(Some(&[ToolKind::ClaudeCode]));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].kind(), ToolKind::ClaudeCode);
    }

    #[test]
    fn nonexistent_paths_filtered() {
        let mut r = Registry::new();
        r.register(Box::new(FakeAdapter {
            kind: ToolKind::ClaudeCode,
            paths: vec![PathBuf::from("/this/does/not/exist/anywhere/expected")],
        }));
        assert_eq!(r.detected(None).len(), 0);
    }
}
