//! Scratch `CODEX_HOME` management for codex skill isolation (spec 016 D3).
//!
//! codex has no per-run flag that suppresses `$CODEX_HOME/skills`
//! (`--ignore-user-config` is a measured no-op for skills — spec 016
//! Appendix B). The working mechanism is a home-directory swap: point
//! `CODEX_HOME` at a scratch directory containing **only** a copy of the real
//! `auth.json`. This module owns that directory's whole lifecycle:
//!
//! - **Allocated once per eval run**, not per case (the [`crate::runner::AikitEvalRunner`]
//!   holds one lazily-created instance) — per-case copies multiply the
//!   auth-rotation hazard below.
//! - **Credential hygiene:** the scratch home holds credentials. It is deleted
//!   unconditionally when the runner is dropped — even when a failed case's
//!   workspace is retained for debugging — and its *contents* are never
//!   logged (paths only).
//! - **Auth rotation write-back:** if codex refreshes its OAuth token mid-run
//!   it writes the rotated token to the *scratch* copy. If the provider
//!   invalidated the old refresh token, the user's real `~/.codex/auth.json`
//!   is now orphaned. On drop, if the scratch `auth.json` content changed, it
//!   is copied back over the real one atomically and the action is logged
//!   loudly. (Residual risk: this write-back is exercised with fake auth
//!   files in unit tests; a real provider-side refresh cannot be forced
//!   deterministically.)

use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// A scratch `CODEX_HOME` holding only a copied `auth.json`. See module docs.
#[derive(Debug)]
pub(crate) struct CodexScratchHome {
    dir: tempfile::TempDir,
    /// The user's real `auth.json` location, for the rotation write-back.
    real_auth: PathBuf,
    /// Bytes of the real `auth.json` at allocation time (`None` = it did not
    /// exist; env-var API-key auth still works through the scratch home).
    original: Option<Vec<u8>>,
}

impl CodexScratchHome {
    /// Allocate a scratch home mirroring the ambient codex home
    /// (`$CODEX_HOME`, else `~/.codex`). Returns `None` (with a loud warning)
    /// when allocation fails — the caller then degrades user-scope isolation
    /// and reports it, rather than running with a half-configured env.
    pub(crate) fn allocate() -> Option<CodexScratchHome> {
        let real_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))
            .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".codex")))?;
        match Self::allocate_with(real_home.join("auth.json")) {
            Ok(home) => Some(home),
            Err(e) => {
                eprintln!(
                    "warning: could not allocate scratch CODEX_HOME for skill isolation: {e}; \
                     codex user scope will NOT be isolated this run"
                );
                None
            }
        }
    }

    /// Allocate against an explicit real `auth.json` path (unit-test seam).
    pub(crate) fn allocate_with(real_auth: PathBuf) -> io::Result<CodexScratchHome> {
        let dir = tempfile::TempDir::new()?;
        let original = match std::fs::read(&real_auth) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        if let Some(bytes) = &original {
            let scratch_auth = dir.path().join("auth.json");
            std::fs::write(&scratch_auth, bytes)?;
            restrict_permissions(&scratch_auth)?;
        }
        Ok(CodexScratchHome {
            dir,
            real_auth,
            original,
        })
    }

    /// Path of the scratch home (the value for the `CODEX_HOME` env var).
    pub(crate) fn path(&self) -> &Path {
        self.dir.path()
    }
}

impl Drop for CodexScratchHome {
    fn drop(&mut self) {
        // Rotation write-back (module docs). Runs before the TempDir field's
        // own Drop deletes the scratch home — field drop order is declaration
        // order, after this body.
        let scratch_auth = self.dir.path().join("auth.json");
        if let Ok(current) = std::fs::read(&scratch_auth) {
            if self.original.as_deref() != Some(current.as_slice()) {
                match write_atomic_0600(&self.real_auth, &current) {
                    Ok(()) => {
                        let msg = format!(
                            "codex rotated its auth token during this eval run; the refreshed \
                             auth.json was copied back to {} so the real credential is not \
                             orphaned",
                            self.real_auth.display()
                        );
                        eprintln!("notice: {msg}");
                        tracing::warn!(target: "aikit_evals::isolation", "{msg}");
                    }
                    Err(e) => {
                        let msg = format!(
                            "codex auth.json changed during this eval run but writing it back \
                             to {} FAILED ({e}); your codex login may be invalidated — re-run \
                             `codex login` if codex stops authenticating",
                            self.real_auth.display()
                        );
                        eprintln!("warning: {msg}");
                        tracing::warn!(target: "aikit_evals::isolation", "{msg}");
                    }
                }
            }
        }
        // TempDir drops next: the scratch home (credentials) is deleted
        // UNCONDITIONALLY — never retained, regardless of case outcome.
    }
}

fn restrict_permissions(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Write `bytes` to `dest` atomically (temp file in the same directory +
/// rename) with 0600 permissions.
fn write_atomic_0600(dest: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = dest
        .parent()
        .ok_or_else(|| io::Error::other("auth.json path has no parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.flush()?;
    restrict_permissions(tmp.path())?;
    tmp.persist(dest).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratch_home_copies_auth_and_is_deleted_on_drop() {
        let real = tempfile::tempdir().unwrap();
        let real_auth = real.path().join("auth.json");
        std::fs::write(&real_auth, b"{\"token\":\"original\"}").unwrap();

        let home = CodexScratchHome::allocate_with(real_auth.clone()).unwrap();
        let scratch_root = home.path().to_path_buf();
        let scratch_auth = scratch_root.join("auth.json");
        assert_eq!(
            std::fs::read(&scratch_auth).unwrap(),
            b"{\"token\":\"original\"}",
            "scratch home must contain a copy of the real auth.json"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&scratch_auth)
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "scratch auth.json must be 0600");
        }

        drop(home);
        assert!(
            !scratch_root.exists(),
            "scratch home holds credentials and must be deleted unconditionally on drop"
        );
    }

    #[test]
    fn rotated_scratch_auth_is_written_back_atomically() {
        let real = tempfile::tempdir().unwrap();
        let real_auth = real.path().join("auth.json");
        std::fs::write(&real_auth, b"{\"token\":\"original\"}").unwrap();

        let home = CodexScratchHome::allocate_with(real_auth.clone()).unwrap();
        // Simulate codex refreshing the OAuth token mid-run: it writes the
        // rotated token to the SCRATCH copy.
        std::fs::write(home.path().join("auth.json"), b"{\"token\":\"rotated\"}").unwrap();
        drop(home);

        assert_eq!(
            std::fs::read(&real_auth).unwrap(),
            b"{\"token\":\"rotated\"}",
            "a rotated scratch auth.json must be copied back over the real one on drop"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&real_auth).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "written-back auth.json must be 0600");
        }
    }

    #[test]
    fn unchanged_scratch_auth_leaves_real_auth_untouched() {
        let real = tempfile::tempdir().unwrap();
        let real_auth = real.path().join("auth.json");
        std::fs::write(&real_auth, b"{\"token\":\"original\"}").unwrap();
        let before = std::fs::metadata(&real_auth).unwrap().modified().unwrap();

        let home = CodexScratchHome::allocate_with(real_auth.clone()).unwrap();
        drop(home);

        assert_eq!(
            std::fs::read(&real_auth).unwrap(),
            b"{\"token\":\"original\"}"
        );
        let after = std::fs::metadata(&real_auth).unwrap().modified().unwrap();
        assert_eq!(
            before, after,
            "an unchanged scratch auth must not rewrite the real file"
        );
    }

    #[test]
    fn missing_real_auth_yields_empty_scratch_home() {
        let real = tempfile::tempdir().unwrap();
        let real_auth = real.path().join("auth.json");

        let home = CodexScratchHome::allocate_with(real_auth.clone()).unwrap();
        assert!(
            !home.path().join("auth.json").exists(),
            "no real auth.json → scratch home stays empty (env-var auth still works)"
        );
        drop(home);
        assert!(
            !real_auth.exists(),
            "no phantom auth.json may be created for the user"
        );
    }
}
