//! Schema + migration for the capture SQLite DB (spec 010 §11.2).
//!
//! Three tables live in one DB file. Both event tables enforce the
//! `(source_file, source_event_id)` uniqueness invariant — the idempotency
//! contract that makes `scan --force` safe.

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

-- Query accelerators for the serve routes + MCP tools.
CREATE INDEX IF NOT EXISTS idx_tool_events_tool_session
    ON capture_tool_events(tool, session_id);
CREATE INDEX IF NOT EXISTS idx_tool_events_started
    ON capture_tool_events(started_at_ms);
CREATE INDEX IF NOT EXISTS idx_token_events_tool_session
    ON capture_token_events(tool, session_id);
CREATE INDEX IF NOT EXISTS idx_cache_observations_tool_session
    ON capture_cache_observations(tool, session_id);
"#;

/// Open or create the capture DB at `path`, run migrations, and return a
/// connection ready for [`crate::storage::SqliteEventStore`] /
/// [`crate::storage::SqliteCursorStore`] to share.
pub fn open(path: &std::path::Path) -> Result<Arc<std::sync::Mutex<Connection>>, rusqlite::Error> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
    conn.execute_batch(MIGRATION_SQL)?;
    Ok(Arc::new(std::sync::Mutex::new(conn)))
}

/// Open an in-memory DB. For tests.
#[cfg(test)]
pub fn open_in_memory() -> Result<Arc<std::sync::Mutex<Connection>>, rusqlite::Error> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(MIGRATION_SQL)?;
    Ok(Arc::new(std::sync::Mutex::new(conn)))
}
