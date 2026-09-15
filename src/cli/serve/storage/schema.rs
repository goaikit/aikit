//! Schema + migration for the capture SQLite DB (spec 010 §11.2).
//!
//! The capture tables live in one DB file. Both event tables enforce the
//! `(source_file, source_event_id)` uniqueness invariant — the idempotency
//! contract that makes `scan --force` safe. `capture_session_briefs` (ADR
//! 0022) is keyed by `(tool, session_id)` and replaced on write.

use std::sync::Arc;

use rusqlite::Connection;

/// DDL for all capture tables + indices. `IF NOT EXISTS` makes this safe to
/// run on every open (cheap — SQLite short-circuits existing objects).
pub const MIGRATION_SQL: &str = r#"
-- Normalized tool-call events. One row per parsed tool_use/tool_result.
CREATE TABLE IF NOT EXISTS capture_tool_events (
    source_file     TEXT    NOT NULL,
    source_event_id TEXT    NOT NULL,
    session_id      TEXT    NOT NULL,
    tool            TEXT    NOT NULL,
    kind            TEXT    NOT NULL,
    target          TEXT,
    input           TEXT,
    output          TEXT,
    status          TEXT    NOT NULL,
    error_message   TEXT,
    started_at_ms   INTEGER,
    duration_ms     INTEGER,
    git_root        TEXT,
    metadata        TEXT    NOT NULL DEFAULT '{}',
    PRIMARY KEY (source_file, source_event_id)
);

-- Per-turn token usage. Deduped by (source_file, source_event_id).
CREATE TABLE IF NOT EXISTS capture_token_events (
    source_file              TEXT    NOT NULL,
    source_event_id          TEXT    NOT NULL,
    session_id               TEXT    NOT NULL,
    tool                     TEXT    NOT NULL,
    model                    TEXT,
    request_id               TEXT,
    input_tokens             INTEGER,
    cache_read_tokens        INTEGER,
    cache_creation_tokens    INTEGER,
    cache_creation_1h_tokens INTEGER,
    output_tokens            INTEGER,
    reasoning_tokens         INTEGER,
    captured_at_ms           INTEGER NOT NULL,
    captured_via             TEXT    NOT NULL,
    PRIMARY KEY (source_file, source_event_id)
);

-- Cache behavior observations emitted by transcript adapters.
CREATE TABLE IF NOT EXISTS capture_cache_observations (
    source_file                       TEXT    NOT NULL,
    source_event_id                   TEXT    NOT NULL,
    session_id                        TEXT    NOT NULL,
    tool                              TEXT    NOT NULL,
    cache_read_input_tokens           INTEGER,
    cache_creation_input_tokens       INTEGER,
    cache_creation_1h_input_tokens    INTEGER,
    assistant_blocks_hash             TEXT,
    tools_changed                     TEXT    NOT NULL DEFAULT '[]',
    observed_at_ms                    INTEGER NOT NULL,
    PRIMARY KEY (source_file, source_event_id)
);

-- Offset cursors: one row per source file. Keyed by the canonical path so
-- both the manual scan route and the watch driver (Phase 4.5) resume from
-- the same place.
CREATE TABLE IF NOT EXISTS capture_cursors (
    source_file   TEXT    PRIMARY KEY,
    offset        INTEGER NOT NULL,
    adapter_kind  TEXT    NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

-- Session briefs (ADR 0023): one per (tool, session). `areas` and `tags`
-- are JSON arrays; `extra` is a JSON object holding every field added
-- after the first version, so the record grows without a column migration.
CREATE TABLE IF NOT EXISTS capture_session_briefs (
    tool            TEXT    NOT NULL,
    session_id      TEXT    NOT NULL,
    summary         TEXT    NOT NULL,
    areas           TEXT    NOT NULL DEFAULT '[]',
    tags            TEXT    NOT NULL DEFAULT '[]',
    model           TEXT    NOT NULL,
    digest_hash     TEXT    NOT NULL,
    generated_at_ms INTEGER NOT NULL,
    extra           TEXT    NOT NULL DEFAULT '{}',
    PRIMARY KEY (tool, session_id)
);

-- Query accelerators for the serve routes + MCP tools.
CREATE INDEX IF NOT EXISTS idx_tool_events_tool_session
    ON capture_tool_events(tool, session_id);
CREATE INDEX IF NOT EXISTS idx_tool_events_started
    ON capture_tool_events(started_at_ms);
CREATE INDEX IF NOT EXISTS idx_token_events_tool_session
    ON capture_token_events(tool, session_id);
CREATE INDEX IF NOT EXISTS idx_cache_observations_tool_session
    ON capture_cache_observations(tool, session_id);
CREATE INDEX IF NOT EXISTS idx_session_briefs_generated
    ON capture_session_briefs(tool, generated_at_ms);
"#;

/// Open or create the capture DB at `path`, run migrations, and return a
/// connection ready for [`crate::storage::SqliteEventStore`] /
/// [`crate::storage::SqliteCursorStore`] to share.
pub fn open(path: &std::path::Path) -> Result<Arc<std::sync::Mutex<Connection>>, rusqlite::Error> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(path)?;
    // `aikit serve` and the `aikit session` commands can share this file. A
    // writer waits up to 5 s for another's lock instead of failing at once
    // with "database is locked".
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    init_with_retry(&conn)?;
    Ok(Arc::new(std::sync::Mutex::new(conn)))
}

/// WAL mode, then the migration. Switching a new file to WAL needs an
/// exclusive lock, and SQLite can report that lock as busy without waiting on
/// the busy timeout, so two processes creating the same file at once would
/// fail with "database is locked". Retry that for up to 5 s.
fn init_with_retry(conn: &Connection) -> Result<(), rusqlite::Error> {
    use rusqlite::ErrorCode;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let attempt = (|| {
            let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
            if !mode.eq_ignore_ascii_case("wal") {
                conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0))?;
            }
            conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
            conn.execute_batch(MIGRATION_SQL)
        })();
        match attempt {
            Err(rusqlite::Error::SqliteFailure(e, _))
                if matches!(e.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            other => return other,
        }
    }
}

/// Open an in-memory DB. For tests.
#[cfg(test)]
pub fn open_in_memory() -> Result<Arc<std::sync::Mutex<Connection>>, rusqlite::Error> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(MIGRATION_SQL)?;
    Ok(Arc::new(std::sync::Mutex::new(conn)))
}

#[cfg(test)]
mod open_tests {
    use super::*;

    #[test]
    fn processes_creating_the_same_new_file_at_once_all_open_it() {
        for _round in 0..10 {
            let dir = tempfile::tempdir().unwrap();
            let path = Arc::new(dir.path().join("capture.db"));
            let barrier = Arc::new(std::sync::Barrier::new(8));
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let path = Arc::clone(&path);
                    let barrier = Arc::clone(&barrier);
                    std::thread::spawn(move || {
                        barrier.wait();
                        open(&path).map(|_| ()).map_err(|e| e.to_string())
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap().expect("every opener succeeds");
            }
            let conn = open(&path).unwrap();
            let mode: String = conn
                .lock()
                .unwrap()
                .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                .unwrap();
            assert_eq!(mode, "wal");
        }
    }
}
