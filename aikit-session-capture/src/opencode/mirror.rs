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

// ---------------------------------------------------------------------------
// Idle snapshots: read-only toward OpenCode's own directory.
// ---------------------------------------------------------------------------

/// Whether `db` is a SQLite file in WAL mode: header bytes 18 and 19 (file
/// format write and read versions) are 2 in WAL mode and 1 otherwise.
pub(crate) fn is_wal_mode(db: &Path) -> bool {
    use std::io::Read;
    let mut header = [0u8; 20];
    match std::fs::File::open(db).and_then(|mut f| f.read_exact(&mut header)) {
        Ok(()) => &header[..16] == b"SQLite format 3\0" && (header[18] == 2 || header[19] == 2),
        Err(_) => false,
    }
}

fn sibling(db: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", db.to_string_lossy()))
}

/// Whether opening `db` read-only in place would create files next to it.
///
/// SQLite creates `-wal` and `-shm` beside a WAL-mode database when they are
/// missing, even for a read-only connection, and leaves them after closing.
/// OpenCode removes them when it closes cleanly, so an idle OpenCode database
/// is exactly this case. While OpenCode runs, both already exist and a
/// read-only open adds nothing.
pub(crate) fn in_place_open_creates_files(db: &Path) -> bool {
    is_wal_mode(db) && !(sibling(db, "-wal").exists() && sibling(db, "-shm").exists())
}

/// `(size, modified)` of a file, `None` when it cannot be read.
fn stamp(p: &Path) -> Option<(u64, std::time::SystemTime)> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.len(), m.modified().ok()?))
}

/// The path to open for `src_db` without writing anything next to it.
///
/// Returns `src_db` itself when an in-place read-only open creates no files
/// (a rollback-journal database, or a WAL database whose `-wal` and `-shm`
/// already exist). Otherwise copies the database into a snapshot directory
/// under `cache_root` (or the §19.3 cache chain when `None`) and returns the
/// copy; an unchanged source reuses the previous copy. Without a writable
/// cache this fails rather than fall back to an open that would write.
pub(crate) fn read_only_source(
    src_db: &Path,
    cache_root: Option<&Path>,
) -> Result<PathBuf, AdapterError> {
    if !in_place_open_creates_files(src_db) {
        return Ok(src_db.to_path_buf());
    }
    let fail = |reason: String| {
        AdapterError::Other(anyhow::anyhow!(
            "read-only snapshot of {} failed: {reason}",
            src_db.display()
        ))
    };
    let root = match cache_root {
        Some(r) => r.to_path_buf(),
        None => {
            pick_mirror_root().map_err(|e| fail(format!("no writable cache directory ({e:?})")))?
        }
    };
    let hash = short_hash(&src_db.to_string_lossy());
    let dir = root
        .join("aikit-session-capture")
        .join("opencode-snapshot")
        .join(&hash[..8]);
    std::fs::create_dir_all(&dir).map_err(|e| fail(format!("mkdir {}: {e}", dir.display())))?;
    let dst_db = dir.join("opencode.db");

    // Three tries: a copy taken while OpenCode starts writing is discarded.
    for _ in 0..3 {
        if !in_place_open_creates_files(src_db) {
            // OpenCode started meanwhile; its sidecars exist now.
            return Ok(src_db.to_path_buf());
        }
        let fresh = files_match(src_db, &dst_db);
        if !fresh {
            let before = stamp(src_db);
            let tmp = dir.join(format!("opencode.db.tmp-{}", std::process::id()));
            std::fs::copy(src_db, &tmp)
                .map_err(|e| fail(format!("copy to {}: {e}", tmp.display())))?;
            if stamp(src_db) != before || sibling(src_db, "-wal").exists() {
                let _ = std::fs::remove_file(&tmp);
                continue;
            }
            std::fs::rename(&tmp, &dst_db)
                .map_err(|e| fail(format!("rename to {}: {e}", dst_db.display())))?;
        }
        // The copy is fully checkpointed: sidecars left by an earlier open of
        // an older copy would not describe it. They live in aikit's cache.
        for suffix in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(sibling(&dst_db, suffix));
        }
        return Ok(dst_db);
    }
    Err(fail(
        "the database kept changing while it was copied; the next scan retries".into(),
    ))
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use rusqlite::Connection;

    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn wal_db(dir: &Path) -> PathBuf {
        let path = dir.join("opencode.db");
        let c = Connection::open(&path).unwrap();
        c.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0))
            .unwrap();
        c.execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES (7);")
            .unwrap();
        drop(c);
        path
    }

    #[test]
    fn detects_wal_mode_from_the_header() {
        let tmp = tempfile::tempdir().unwrap();
        let wal = wal_db(tmp.path());
        assert!(is_wal_mode(&wal));
        let rollback = tmp.path().join("rollback.db");
        Connection::open(&rollback)
            .unwrap()
            .execute_batch("CREATE TABLE t(x);")
            .unwrap();
        assert!(!is_wal_mode(&rollback));
        assert!(!is_wal_mode(&tmp.path().join("missing.db")));
    }

    #[test]
    fn an_idle_wal_database_is_read_from_a_snapshot_and_left_untouched() {
        let src = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let db = wal_db(src.path());
        let before = (listing(src.path()), std::fs::read(&db).unwrap(), stamp(&db));
        assert_eq!(before.0, vec!["opencode.db".to_string()]);

        let open = read_only_source(&db, Some(cache.path())).unwrap();
        assert_ne!(open, db);
        assert!(open.starts_with(cache.path()));
        let conn = crate::opencode::db::open_read_only(&open).unwrap();
        let n: i64 = conn.query_row("SELECT x FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 7);
        drop(conn);

        // Again, with the snapshot and its sidecars already in the cache.
        let again = read_only_source(&db, Some(cache.path())).unwrap();
        assert_eq!(again, open);
        drop(crate::opencode::db::open_read_only(&again).unwrap());

        let after = (listing(src.path()), std::fs::read(&db).unwrap(), stamp(&db));
        assert_eq!(after, before, "the source directory must not change");
    }

    #[test]
    fn a_live_or_rollback_database_is_opened_in_place() {
        let src = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let db = wal_db(src.path());
        // A writer holding the database open keeps both sidecars present.
        let writer = Connection::open(&db).unwrap();
        writer.execute_batch("INSERT INTO t VALUES (8);").unwrap();
        assert!(sibling(&db, "-wal").exists() && sibling(&db, "-shm").exists());
        assert_eq!(read_only_source(&db, Some(cache.path())).unwrap(), db);
        drop(writer);

        let rollback = src.path().join("rollback.db");
        Connection::open(&rollback)
            .unwrap()
            .execute_batch("CREATE TABLE t(x);")
            .unwrap();
        assert_eq!(
            read_only_source(&rollback, Some(cache.path())).unwrap(),
            rollback
        );
        assert!(listing(cache.path()).is_empty(), "no snapshot was needed");
    }
}
