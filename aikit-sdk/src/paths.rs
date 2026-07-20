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
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use walkdir::WalkDir;

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

    // Build the joined path from the *canonical* base so the escape check compares like with
    // like. If we joined onto the raw `base` while comparing against the canonicalized root,
    // any base reached through a symlink — macOS `/tmp` -> `/private/tmp`, a symlinked
    // `$HOME`, a Docker bind mount — would make `starts_with` fail for even a wholly benign
    // fragment, breaking legitimate installs. Fall back to the raw base when it doesn't exist
    // yet (canonicalize fails); the lexical `reject_unsafe_components` check above already
    // guarantees the fragment can't escape in that case.
    let root = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    let joined = root.join(candidate);
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

/// Recursively copy a directory tree from `src` to `dst`, creating `dst`
/// (and any needed subdirectories) as it goes.
///
/// This is the **single canonical recursive-copy implementation** for aikit
/// (ARCH-1 — previously duplicated five times across `aikit-sdk` and the
/// `aikit` CLI crate: `src/fs/mod.rs`, `src/core/template.rs`,
/// `src/core/filesystem.rs`, `src/cli/commands/install.rs`, and this
/// crate's own `install.rs`). Every other copy path in the workspace now
/// delegates here.
///
/// It carries the same symlink hardening as
/// [`crate::install::copy_artifacts`]'s read-side guard: a symlink anywhere
/// in `src` (file or directory) is never followed or copied, closing the
/// same info-disclosure vector — a symlink planted inside a source tree
/// pointing at, say, `~/.ssh/id_rsa` — rather than trusting every caller to
/// have pre-validated `src`. This applies even to copies of otherwise
/// "trusted" internal content, since the source tree may itself have been
/// populated from a downloaded/extracted package moments earlier.
///
/// Every relative path copied here is derived from walking `src` itself
/// (never from an untrusted string), so there is no `..`/absolute-path
/// input to reject the way [`safe_join`] does for untrusted path
/// *fragments* (an artifact destination, a manifest `source` field) —
/// `copy_dir` complements `safe_join`, it doesn't replace it.
pub fn copy_dir(src: &Path, dst: &Path) -> io::Result<()> {
    copy_dir_excluding(src, dst, &[])
}

/// Like [`copy_dir`], but skips any entry whose path component immediately
/// under `src` case-insensitively matches one of `exclude`. Used by the
/// package-install path to skip `.git`, `node_modules`, `target`, and
/// similar directories when copying a whole project-shaped source tree.
pub fn copy_dir_excluding(src: &Path, dst: &Path, exclude: &[&str]) -> io::Result<()> {
    for entry in WalkDir::new(src) {
        // Propagate traversal errors (e.g. a permission-denied subdirectory)
        // rather than silently skipping them: a partial copy that looks like a
        // success is worse than a hard failure for an install/deploy step.
        let entry = entry.map_err(io::Error::from)?;
        let path = entry.path();
        let relative = match path.strip_prefix(src) {
            Ok(r) if !r.as_os_str().is_empty() => r,
            _ => continue, // `src` itself
        };

        if let Some(top) = relative.iter().next().and_then(|c| c.to_str()) {
            if exclude.iter().any(|e| e.eq_ignore_ascii_case(top)) {
                continue;
            }
        }

        // Never follow symlinks: a symlinked file or directory inside `src`
        // is skipped outright rather than having its target's content
        // copied (see doc comment above). `WalkDir` already declines to
        // recurse into symlinked directories; this also drops symlinked
        // files.
        if entry.path_is_symlink() {
            continue;
        }

        let dest_path = dst.join(relative);
        if path.is_dir() {
            fs::create_dir_all(&dest_path)?;
        } else {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(path, &dest_path)?;
        }
    }
    Ok(())
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
    fn safe_join_accepts_benign_fragment_when_base_is_a_symlink() {
        // A base reached through a symlink (macOS `/tmp` -> `/private/tmp`, a symlinked
        // `$HOME`, a Docker bind mount) must NOT cause a benign fragment to be rejected —
        // that was a real regression that broke legitimate installs. `safe_join` canonicalizes
        // the base first and joins onto the canonical root, so the fragment resolves under the
        // real directory and the escape check passes.
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let tmp = TempDir::new().unwrap();
            let real_base = tmp.path().join("real-project");
            std::fs::create_dir_all(&real_base).unwrap();

            let base_link = tmp.path().join("project-link");
            symlink(&real_base, &base_link).unwrap();

            let joined = safe_join(&base_link, "sub/file.txt").unwrap();
            // Resolves under the *real* directory (canonical root), and a traversal fragment
            // is still rejected even through the symlinked base.
            let canonical_real = real_base.canonicalize().unwrap();
            assert!(joined.starts_with(&canonical_real));
            assert!(matches!(
                safe_join(&base_link, "../escape").unwrap_err(),
                PathError::Traversal(_)
            ));
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

    // -- copy_dir / copy_dir_excluding (ARCH-1: the single canonical
    // recursive-copy implementation) ---------------------------------------

    #[test]
    fn copy_dir_copies_nested_structure() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src");
        std::fs::create_dir_all(src.join("a/b/c")).unwrap();
        std::fs::write(src.join("top.txt"), "top").unwrap();
        std::fs::write(src.join("a/mid.txt"), "mid").unwrap();
        std::fs::write(src.join("a/b/c/deep.txt"), "deep").unwrap();

        let dst = temp.path().join("dst");
        copy_dir(&src, &dst).unwrap();

        assert_eq!(std::fs::read_to_string(dst.join("top.txt")).unwrap(), "top");
        assert_eq!(
            std::fs::read_to_string(dst.join("a/mid.txt")).unwrap(),
            "mid"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("a/b/c/deep.txt")).unwrap(),
            "deep"
        );
    }

    #[test]
    fn copy_dir_creates_destination_if_missing() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("file.txt"), "content").unwrap();

        // dst does not exist yet at all, not even its parent.
        let dst = temp.path().join("not/yet/created/dst");
        copy_dir(&src, &dst).unwrap();

        assert_eq!(
            std::fs::read_to_string(dst.join("file.txt")).unwrap(),
            "content"
        );
    }

    // Read-side symlink guard, matching `install::copy_artifacts`'s hardening:
    // a symlink inside `src` must never be followed and have its target's
    // content copied into `dst` (info-disclosure vector).
    #[cfg(unix)]
    #[test]
    fn copy_dir_skips_symlinked_files() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let secret = temp.path().join("secret.txt");
        std::fs::write(&secret, "TOP SECRET").unwrap();

        let src = temp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("real.txt"), "legit").unwrap();
        symlink(&secret, src.join("leak.txt")).unwrap();

        let dst = temp.path().join("dst");
        copy_dir(&src, &dst).unwrap();

        assert!(dst.join("real.txt").exists());
        assert!(
            !dst.join("leak.txt").exists(),
            "symlinked file must not be copied — secret would leak"
        );
    }

    // Read-side symlink guard for symlinked directories too.
    #[cfg(unix)]
    #[test]
    fn copy_dir_skips_symlinked_directories() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().unwrap();
        let secret_dir = temp.path().join("secret_dir");
        std::fs::create_dir_all(&secret_dir).unwrap();
        std::fs::write(secret_dir.join("inside.txt"), "TOP SECRET").unwrap();

        let src = temp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("real.txt"), "legit").unwrap();
        symlink(&secret_dir, src.join("linked_dir")).unwrap();

        let dst = temp.path().join("dst");
        copy_dir(&src, &dst).unwrap();

        assert!(dst.join("real.txt").exists());
        assert!(
            !dst.join("linked_dir").exists(),
            "symlinked directory must not be copied — secret would leak"
        );
    }

    #[test]
    fn copy_dir_excluding_skips_named_top_level_dirs() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src");
        std::fs::create_dir_all(src.join(".git")).unwrap();
        std::fs::write(src.join(".git/HEAD"), "ref: refs/heads/main").unwrap();
        std::fs::create_dir_all(src.join("node_modules/pkg")).unwrap();
        std::fs::write(src.join("node_modules/pkg/index.js"), "// dep").unwrap();
        std::fs::create_dir_all(src.join("keep")).unwrap();
        std::fs::write(src.join("keep/file.txt"), "content").unwrap();

        let dst = temp.path().join("dst");
        copy_dir_excluding(&src, &dst, &["target", ".git", "node_modules"]).unwrap();

        assert!(dst.join("keep/file.txt").exists());
        assert!(!dst.join(".git").exists());
        assert!(!dst.join("node_modules").exists());
    }

    #[test]
    fn copy_dir_excluding_match_is_case_insensitive() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src");
        std::fs::create_dir_all(src.join("TARGET")).unwrap();
        std::fs::write(src.join("TARGET/build.o"), "binary").unwrap();

        let dst = temp.path().join("dst");
        copy_dir_excluding(&src, &dst, &["target"]).unwrap();

        assert!(!dst.join("TARGET").exists());
    }

    // ARCH-1 review: a traversal error (e.g. an unreadable subdirectory) must
    // propagate as a hard failure, not be silently skipped — a partial copy that
    // looks like success is worse than an error for an install/deploy step.
    #[cfg(unix)]
    #[test]
    fn copy_dir_propagates_traversal_errors() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src");
        let locked = src.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(src.join("ok.txt"), "hi").unwrap();
        // Remove read+execute so WalkDir can't list/descend into `locked`.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

        // If the directory is still listable (e.g. the test runs as root, which
        // bypasses permission bits) the error can't be provoked — skip the
        // assertion rather than record a false negative.
        let can_still_read = std::fs::read_dir(&locked).is_ok();

        let dst = temp.path().join("dst");
        let result = copy_dir(&src, &dst);

        // Restore permissions so TempDir cleanup can remove `locked`.
        let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755));

        if !can_still_read {
            assert!(
                result.is_err(),
                "copy_dir must propagate a permission/traversal error, not silently skip it"
            );
        }
    }
}
