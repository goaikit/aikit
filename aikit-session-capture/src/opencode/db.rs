//! SQLite read-only open + table-exists guard for the OpenCode adapter.
//!
//! See spec 010 §12.3. OpenCode persists state in `opencode.db` (plus
//! `-wal`/`-shm` siblings). The adapter opens it read-only with
//! `query_only(1)` so even a bug in the parser cannot corrupt the live DB.
//!
//! Reference: `superbased-observer/internal/adapter/opencode/adapter.go`
//! (`openReadOnlyDB`, `tableExists`, `latestWatermark`).

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::adapter::AdapterError;

/// Open `opencode.db` read-only with `query_only(1)` and a 2s busy timeout.
///
/// `busy_timeout(2000)` matters because OpenCode may be actively writing the
/// WAL while we read. The read-only + query_only combo is belt-and-braces:
/// even a stray `UPDATE` from the parser would be rejected at the SQLite
/// layer, never touching the underlying file.
///
/// `SQLITE_OPEN_URI` enables the URL-form pragma directives
/// (`?mode=ro&query_only=1&busy_timeout=2000`).
pub(crate) fn open_read_only(path: &Path) -> Result<Connection, AdapterError> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let url = format!(
        "file:{}?mode=ro&query_only=1&busy_timeout=2000",
        path.to_string_lossy()
    );
    Connection::open_with_flags(&url, flags).map_err(|source| AdapterError::SqliteOpen {
        path: path.to_path_buf(),
        source,
    })
}

/// `true` when `table_name` exists in the DB. Used to gracefully degrade on
/// schemas that don't have every table (e.g. older builds without `todo`).
#[allow(dead_code)] // Reserved for future phases (todo / subtask tables).
pub(crate) fn table_exists(db: &Connection, table_name: &str) -> bool {
    match db.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
        [table_name],
        |row| row.get::<_, i64>(0),
    ) {
        Ok(n) => n > 0,
        Err(_) => false,
    }
}

/// The high-water `time_updated` across `message`, `part`, and `session` —
/// the adapter's resumable offset. Mirrors observer's `latestWatermark`
/// (`opencode/adapter.go:1125`).
pub(crate) fn latest_watermark(db: &Connection) -> i64 {
    let q = "SELECT MAX(v) FROM (
        SELECT COALESCE(MAX(time_updated), 0) AS v FROM message
        UNION ALL
        SELECT COALESCE(MAX(time_updated), 0) AS v FROM part
        UNION ALL
        SELECT COALESCE(MAX(time_updated), 0) AS v FROM session
    )";
    db.query_row(q, [], |row| row.get::<_, i64>(0)).unwrap_or(0)
}
