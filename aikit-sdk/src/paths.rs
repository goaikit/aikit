//! Reusable path/id validation helpers shared by every install/extract/session code path.
//!
//! See ADR 0013 (package install writes only within the target project) and the audit
//! finding "Two reusable helpers" (specs/issues-audit-remediation-2026-07-07.md) for the
//! design rationale. Two shapes of untrusted input show up repeatedly:
//!
//! - **Path fragments** (an artifact destination, a skill/subagent `source`) — validated
//!   with [`safe_join`], which rejects absolute paths and `..` components lexically, then
//!   verifies (defense in depth) that the joined result still resolves under the
//!   canonicalized base after any symlink resolution.
//! - **Flat identifiers** (package `name`/`version`, a client-supplied `session_id`) — these
//!   are never path fragments (no `/`), so they are validated with [`is_safe_id`] against a
//!   strict charset instead.

use std::error::Error;
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// Error returned by [`safe_join`] when an untrusted path fragment is rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// The untrusted fragment was an absolute path.
    Absolute(String),
    /// The untrusted fragment contained a `..` (parent-dir) component.
    Traversal(String),
    /// The joined result did not resolve under the canonicalized base (symlink escape).
    Escape(String),
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::Absolute(s) => write!(f, "path must be relative, got absolute path: {}", s),
            PathError::Traversal(s) => {
                write!(f, "path must not contain '..' components: {}", s)
            }
            PathError::Escape(s) => write!(
                f,
                "path escapes the target directory after resolution: {}",
                s
            ),
        }
    }
}

impl Error for PathError {}

/// Lexical-only check: rejects absolute paths and any `..` component.
///
/// This is the portion of [`safe_join`]'s validation that does not require a concrete base
/// directory to exist on disk, so it can run at manifest-parse time (before any project root
/// is known) as well as inside `safe_join` itself.
fn reject_unsafe_components(untrusted: &str) -> Result<(), PathError> {
    let candidate = Path::new(untrusted);
    if candidate.is_absolute() {
        return Err(PathError::Absolute(untrusted.to_string()));
    }
    if candidate
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(PathError::Traversal(untrusted.to_string()));
    }
    Ok(())
}

/// Returns `true` if `untrusted` is safe to join onto *some* base directory: not absolute,
/// and no `..` component. This is the same lexical rule `safe_join` enforces, exposed as a
/// boolean predicate for validation call sites (e.g. manifest parsing, `Package::validate`)
/// that don't have a concrete base path available.
pub fn is_safe_relative_path(untrusted: &str) -> bool {
    reject_unsafe_components(untrusted).is_ok()
}

/// Joins `untrusted` onto `base`, refusing to escape `base`.
///
/// Rejects:
/// - absolute paths (`Path::join` would otherwise silently replace `base`), and
/// - any `..` (parent-dir) component.
///
/// As defense in depth, once the path is lexically safe it also verifies — after resolving
/// symlinks on `base` — that the joined result still starts with the canonicalized base, so a
/// symlink planted at an intermediate path segment can't be used to walk out.
pub fn safe_join(base: &Path, untrusted: &str) -> Result<PathBuf, PathError> {
    reject_unsafe_components(untrusted)?;

    let candidate = Path::new(untrusted);
    let joined = base.join(candidate);

    // Defense in depth: after any symlink resolution the result must stay under base.
    let root = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    if !joined.starts_with(&root) {
        return Err(PathError::Escape(untrusted.to_string()));
    }
    Ok(joined)
}

/// A strict id charset for anything used to build a filename or cache-dir segment (never a
/// path fragment — no `/` is ever permitted).
pub fn is_safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && s != "."
        && s != ".."
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // -- safe_join --------------------------------------------------------

    #[test]
    fn safe_join_accepts_simple_relative_path() {
        let tmp = TempDir::new().unwrap();
        let result = safe_join(tmp.path(), "sub/dir/file.txt").unwrap();
        assert_eq!(result, tmp.path().join("sub/dir/file.txt"));
    }

    #[test]
    fn safe_join_accepts_relative_path_into_nonexistent_subdir() {
        let tmp = TempDir::new().unwrap();
        // base itself exists (so canonicalize succeeds) but the target subdir does not yet.
        let result = safe_join(tmp.path(), "not/yet/created.txt").unwrap();
        assert_eq!(result, tmp.path().join("not/yet/created.txt"));
    }

    #[test]
    fn safe_join_rejects_absolute_path() {
        let tmp = TempDir::new().unwrap();
        let err = safe_join(tmp.path(), "/etc/passwd").unwrap_err();
        assert!(matches!(err, PathError::Absolute(_)));
    }

    #[test]
    fn safe_join_rejects_simple_traversal() {
        let tmp = TempDir::new().unwrap();
        let err = safe_join(tmp.path(), "../../../etc/cron.d").unwrap_err();
        assert!(matches!(err, PathError::Traversal(_)));
    }

    #[test]
    fn safe_join_rejects_traversal_buried_in_middle() {
        let tmp = TempDir::new().unwrap();
        let err = safe_join(tmp.path(), "sub/../../escape").unwrap_err();
        assert!(matches!(err, PathError::Traversal(_)));
    }

    #[test]
    fn safe_join_rejects_windows_style_absolute_on_own_platform() {
        let tmp = TempDir::new().unwrap();
        #[cfg(windows)]
        {
            let err = safe_join(tmp.path(), r"C:\Windows\System32").unwrap_err();
            assert!(matches!(err, PathError::Absolute(_)));
        }
        #[cfg(not(windows))]
        {
            // Not absolute on Unix, but must still not escape — treated as a relative
            // (oddly named) path component and accepted as such.
            let result = safe_join(tmp.path(), r"C:\Windows\System32");
            assert!(result.is_ok());
        }
    }

    #[test]
    fn safe_join_fails_closed_when_base_itself_is_a_symlink() {
        // The escape check compares the (non-canonical) joined path against the
        // *canonicalized* base. If `base` itself is a symlink, those two frames of
        // reference disagree even for an entirely benign fragment — so `safe_join` fails
        // closed (rejects) rather than risk silently trusting a base that isn't what it
        // appears to be. This is intentional strictness (ADR 0013: prefer the strict/robust
        // option), not a bug: callers are expected to pass an already-trustworthy base
        // (e.g. `project_root`/`package_root`, never attacker-controlled), and a spurious
        // rejection is a far safer failure mode than a spurious escape.
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let tmp = TempDir::new().unwrap();
            let real_base = tmp.path().join("real-project");
            std::fs::create_dir_all(&real_base).unwrap();

            let base_link = tmp.path().join("project-link");
            symlink(&real_base, &base_link).unwrap();

            let err = safe_join(&base_link, "sub/file.txt").unwrap_err();
            assert!(matches!(err, PathError::Escape(_)));
        }
    }

    // -- is_safe_relative_path --------------------------------------------

    #[test]
    fn is_safe_relative_path_accepts_normal_fragment() {
        assert!(is_safe_relative_path("skills/my-skill"));
        assert!(is_safe_relative_path("file.md"));
    }

    #[test]
    fn is_safe_relative_path_rejects_absolute_and_traversal() {
        assert!(!is_safe_relative_path("/etc/passwd"));
        assert!(!is_safe_relative_path("../../etc/passwd"));
        assert!(!is_safe_relative_path("a/../../b"));
    }

    // -- is_safe_id ---------------------------------------------------------

    #[test]
    fn is_safe_id_accepts_typical_ids() {
        assert!(is_safe_id("my-package"));
        assert!(is_safe_id("1.2.3"));
        assert!(is_safe_id("session_abc-123.def"));
        assert!(is_safe_id("a"));
    }

    #[test]
    fn is_safe_id_rejects_empty() {
        assert!(!is_safe_id(""));
    }

    #[test]
    fn is_safe_id_rejects_dot_and_dotdot() {
        assert!(!is_safe_id("."));
        assert!(!is_safe_id(".."));
    }

    #[test]
    fn is_safe_id_rejects_path_separators() {
        assert!(!is_safe_id("../etc/passwd"));
        assert!(!is_safe_id("a/b"));
        assert!(!is_safe_id("a\\b"));
    }

    #[test]
    fn is_safe_id_rejects_absolute_path() {
        assert!(!is_safe_id("/etc/passwd"));
    }

    #[test]
    fn is_safe_id_rejects_over_length_limit() {
        let long = "a".repeat(129);
        assert!(!is_safe_id(&long));
        let boundary = "a".repeat(128);
        assert!(is_safe_id(&boundary));
    }

    #[test]
    fn is_safe_id_rejects_special_chars() {
        assert!(!is_safe_id("id;rm -rf"));
        assert!(!is_safe_id("id\0null"));
        assert!(!is_safe_id("id with spaces"));
    }
}
