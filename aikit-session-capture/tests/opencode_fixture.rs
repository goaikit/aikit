//! Test helper: seed a synthetic `opencode.db` with the schema OpenCode
//! uses (session/message/part tables) and a small set of rows with
//! monotonic `time_updated` values.
//!
//! Used by the opencode adapter tests. Mirrors the schema documented in
//! `superbased-observer/internal/adapter/opencode/transcript.go:18`.

#![cfg(test)]
#![cfg(feature = "opencode")]
#![allow(dead_code)]

use rusqlite::Connection;
use std::path::Path;

/// Create the schema. Idempotent — safe to call on an existing DB.
pub(crate) fn create_schema(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS session (
            id TEXT PRIMARY KEY,
            directory TEXT,
            time_created INTEGER,
            time_updated INTEGER
        );
        CREATE TABLE IF NOT EXISTS message (
            id TEXT PRIMARY KEY,
            session_id TEXT,
            time_created INTEGER,
            time_updated INTEGER,
            data TEXT
        );
        CREATE TABLE IF NOT EXISTS part (
            id TEXT PRIMARY KEY,
            message_id TEXT,
            session_id TEXT,
            time_created INTEGER,
            time_updated INTEGER,
            data TEXT
        );",
    )
}

/// Seed the DB with a known fixture: one session, two messages (user +
/// assistant), and a tool part (bash) + a token-bearing assistant message.
///
/// All rows are inserted with explicit `time_updated` values so the watermark
/// parser can be tested deterministically.
pub(crate) fn seed_fixture(db: &Connection) -> rusqlite::Result<()> {
    create_schema(db)?;
    // Session at t=1000.
    db.execute(
        "INSERT INTO session (id, directory, time_created, time_updated) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params!["sess-1", "/tmp/opencode-fixture", 1000i64, 1000i64],
    )?;
    // User message at t=1100.
    db.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            "msg-user-1",
            "sess-1",
            1100i64,
            1100i64,
            r#"{"role":"user","path":{"cwd":"/tmp/opencode-fixture"},"time":{"created":1100}}"#,
        ],
    )?;
    // Text part attached to the user message at t=1150.
    db.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            "part-text-1",
            "msg-user-1",
            "sess-1",
            1150i64,
            1150i64,
            r#"{"type":"text","text":"Run the tests"}"#,
        ],
    )?;
    // Assistant message at t=1200 with token data.
    db.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            "msg-asst-1",
            "sess-1",
            1200i64,
            1200i64,
            r#"{"role":"assistant","modelID":"claude-sonnet-4-20250514","path":{"cwd":"/tmp/opencode-fixture"},"time":{"created":1200,"completed":1250},"tokens":{"input":100,"output":50,"reasoning":10,"cache":{"read":80,"write":20}}}"#,
        ],
    )?;
    // Tool part (bash) at t=1300 with success.
    db.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            "part-tool-1",
            "msg-asst-1",
            "sess-1",
            1300i64,
            1300i64,
            r#"{"type":"tool","tool":"bash","callID":"call-1","state":{"status":"completed","input":{"command":"go test ./..."},"output":"ok all tests pass","metadata":{"exit":0,"output":""},"time":{"start":1250,"end":1300}}}"#,
        ],
    )?;
    Ok(())
}

/// Seed a fixture with a failing bash + a tool with secrets.
pub(crate) fn seed_secrets_fixture(db: &Connection) -> rusqlite::Result<()> {
    create_schema(db)?;
    db.execute(
        "INSERT INTO session (id, directory, time_created, time_updated) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params!["sess-sec", "/tmp/opencode-sec", 1000i64, 1000i64],
    )?;
    db.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            "msg-sec-1",
            "sess-sec",
            1100i64,
            1100i64,
            r#"{"role":"assistant","path":{"cwd":"/tmp/opencode-sec"},"time":{"created":1100}}"#,
        ],
    )?;
    db.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            "part-sec-1",
            "msg-sec-1",
            "sess-sec",
            1200i64,
            1200i64,
            r#"{"type":"tool","tool":"bash","callID":"call-sec","state":{"status":"completed","input":{"command":"export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE GITHUB_TOKEN=ghp_0123456789012345678901234567890abcdefgh JWT=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.Gfx6sVE3J0nS12t5mU9gZ2p ANTHROPIC=sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ OPENAI=sk-proj-abcdef1234567890ABCDEFGHIJabcdefghij"},"output":"connected to postgres://deploy:secretpass123@db.internal:5432/prod","metadata":{"exit":0,"output":""},"time":{"start":1150,"end":1200}}}"#,
        ],
    )?;
    Ok(())
}

/// Open an in-memory SQLite DB with the fixture schema + rows. Used by
/// tests that want a real DB without writing to disk.
pub(crate) fn open_fixture_memory() -> rusqlite::Result<Connection> {
    let db = Connection::open_in_memory()?;
    seed_fixture(&db)?;
    Ok(db)
}

/// Open a temp-file-backed `opencode.db` with the fixture. Returns the path
/// the adapter should parse.
pub(crate) fn open_fixture_file(dir: &Path) -> rusqlite::Result<std::path::PathBuf> {
    let path = dir.join("opencode.db");
    let db = Connection::open(&path)?;
    seed_fixture(&db)?;
    // Drop `db` so the file isn't locked when the adapter opens it.
    drop(db);
    Ok(path)
}
