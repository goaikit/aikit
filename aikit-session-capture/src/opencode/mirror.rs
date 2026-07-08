//! Foreign-mount mirror staging for the OpenCode adapter.
//!
//! See spec 010 §12.3 + §19.3. When `opencode.db` lives on a foreign mount
//! (e.g. `/mnt/c/Users/<u>/.local/share/opencode/opencode.db` on a WSL2
//! Linux host reading the Windows side), `rusqlite` hits
//! `SQLITE_IOERR_SHORT_READ` while the Windows process is actively writing
//! the WAL. The fix: stage a local mirror — copy the trio (`.db` + `-wal` +
//! `-shm`) into a per-source cache dir and open the mirror read-only.
//!
//! Reference: `superbased-observer/internal/adapter/opencode/adapter.go:1176`.
//!
//! # Priority chain (spec 010 §19.3)
//!
//! 1. `dirs::cache_dir()` — preferred; persists across runs.
//! 2. `std::env::temp_dir()` — works on read-only hosts; may not persist.
//! 3. None — fall through to direct-open with retry semantics.

use std::path::{Path, PathBuf};

use crate::adapter::AdapterError;

/// Pick the mirror root directory according to the §19.3 priority chain.
/// Returns `Err(MirrorError::NoWritableCache)` when no writable candidate
/// is found; callers then open the source directly with retry semantics.
pub(crate) fn pick_mirror_root() -> Result<PathBuf, MirrorError> {
    // 1. `dirs::cache_dir()` — preferred.
    if let Some(c) = dirs::cache_dir() {
        if dir_is_writable(&c) {
            return Ok(c);
        }
    }
    // 2. `std::env::temp_dir()` — works on read-only hosts.
    let t = std::env::temp_dir();
    if dir_is_writable(&t) {
        return Ok(t);
    }
    Err(MirrorError::NoWritableCache)
}

/// Error returned by `pick_mirror_root`. The adapter turns this into a
/// `ParseResult { retry_suggested: true, warnings: [ForeignMountRetry] }`
/// and the host's poll loop re-attempts on the next tick.
#[derive(Debug)]
pub(crate) enum MirrorError {
    NoWritableCache,
}

/// Stage a mirror for `src_db` (an `opencode.db` path) when it lives on a
/// foreign mount; return the mirror path when one was created or already
/// up-to-date, or `src_db` unchanged when the source is native.
///
/// Native-mount sources short-circuit: `is_foreign_mount_path` returns
/// false, and the function returns `src_db` without copying anything.
pub(crate) fn stage_mirror_if_foreign(src_db: &Path) -> Result<PathBuf, AdapterError> {
    if !is_foreign_mount_path(src_db) {
        return Ok(src_db.to_path_buf());
    }
    let cache_root = pick_mirror_root().map_err(|reason| AdapterError::ForeignMountMirror {
        path: src_db.to_path_buf(),
        reason: format!("mirror root pick failed: {reason:?}"),
    })?;
    // Per-source cache subdir keyed by a short hash of the source path so
    // multiple foreign-mount sources don't collide.
    let hash = short_hash(&src_db.to_string_lossy());
    let mirror_dir = cache_root
        .join("aikit-session-capture")
        .join("opencode-mirror")
        .join(&hash[..8]);
    std::fs::create_dir_all(&mirror_dir).map_err(|e| AdapterError::ForeignMountMirror {
        path: src_db.to_path_buf(),
        reason: format!("mkdir mirror: {e}"),
    })?;
    let dst_db = mirror_dir.join("opencode.db");

    if mirror_up_to_date(src_db, &dst_db) {
        return Ok(dst_db);
    }

    // Copy the trio. Missing siblings are removed from the mirror so a
    // stale `-wal` doesn't shadow a freshly-checkpointed source.
    for suffix in &["", "-wal", "-shm"] {
        let src = format!("{}{suffix}", src_db.to_string_lossy());
        let dst = format!("{}{suffix}", dst_db.to_string_lossy());
        let src_path = Path::new(&src);
        match std::fs::read(src_path) {
            Ok(data) => {
                std::fs::write(&dst, data).map_err(|e| AdapterError::ForeignMountMirror {
                    path: src_db.to_path_buf(),
                    reason: format!("write {}: {e}", dst),
                })?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let _ = std::fs::remove_file(&dst); // stale sibling cleanup
                continue;
            }
            Err(e) => {
                return Err(AdapterError::ForeignMountMirror {
                    path: src_db.to_path_buf(),
                    reason: format!("read {src}: {e}"),
                });
            }
        }
    }
    Ok(dst_db)
}

/// `true` when every trio sibling's `(size, mtime)` on the source matches
/// the mirror. Cheap stat check; the WAL mtime is the fast-moving signal.
fn mirror_up_to_date(src_db: &Path, dst_db: &Path) -> bool {
    for suffix in &["", "-wal", "-shm"] {
        let src_string = format!("{}{suffix}", src_db.to_string_lossy());
        let dst_string = format!("{}{suffix}", dst_db.to_string_lossy());
        let src = Path::new(&src_string);
        let dst = Path::new(&dst_string);
        if !files_match(src, dst) {
            return false;
        }
    }
    true
}

fn files_match(src: &Path, dst: &Path) -> bool {
    let s = match std::fs::metadata(src) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let d = match std::fs::metadata(dst) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if s.len() != d.len() {
        return false;
    }
    use std::time::SystemTime;
    let s_mod = s.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let d_mod = d.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    // `true` when the source is newer than the destination → not up-to-date.
    s_mod <= d_mod
}

/// Detect whether `path` lives on a foreign mount — a filesystem whose
/// device id differs from the cache dir's device id. Linux/WSL2 /mnt/c is
/// the canonical case.
///
/// On platforms where `device_id` extraction isn't supported, returns
/// `false` (assume native). The cost of a false negative is that foreign-
/// mount sources open directly and may hit `SQLITE_IOERR_SHORT_READ`; the
/// adapter emits `ParseWarning::ForeignMountRetry` and the poll loop
/// re-attempts on the next tick.
pub(crate) fn is_foreign_mount_path(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let src_dev = match std::fs::metadata(path) {
            Ok(m) => m.dev(),
            Err(_) => return false,
        };
        let cache_dev = match dirs::cache_dir().and_then(|c| std::fs::metadata(&c).ok()) {
            Some(m) => m.dev(),
            None => return false,
        };
        src_dev != cache_dev
    }
    #[cfg(not(unix))]
    {
        // Non-Unix (Windows, wasm, …) — assume native. The mirror logic is
        // only load-bearing on Linux/WSL2 reading /mnt/c.
        let _ = path;
        false
    }
}

fn dir_is_writable(p: &Path) -> bool {
    std::fs::metadata(p)
        .and_then(|m| {
            if m.is_dir() {
                // Try writing a probe file. Cheaper than a full fsync.
                let probe = p.join(".aikit-session-capture-write-probe");
                std::fs::write(&probe, b"")?;
                let _ = std::fs::remove_file(&probe);
                Ok(())
            } else {
                // Not a directory. Use a generic error kind so the MSRV
                // check stays clean (ErrorKind::NotADirectory is 1.83+).
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "not a directory",
                ))
            }
        })
        .is_ok()
}

fn short_hash(s: &str) -> String {
    // FNV-1a 64-bit, full hex. Deterministic per path.
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in s.as_bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}
