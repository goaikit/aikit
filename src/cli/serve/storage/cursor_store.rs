//! `SqliteCursorStore` — production `CursorStore` impl backed by SQLite.
//!
//! One row per source file. The `adapter_kind` column disambiguates offset
//! semantics on re-load (byte offset for JSONL adapters, `time_updated`
//! watermark for the OpenCode SQLite adapter — spec 010 §10).

use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use rusqlite::{params, Connection};

use aikit_session_capture::{CursorStore, ParseCursor, ToolKind};

pub struct SqliteCursorStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteCursorStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl CursorStore for SqliteCursorStore {
    async fn load(&self, source_file: &Path) -> Option<ParseCursor> {
        let conn = self.conn.clone();
        let key = source_file.to_string_lossy().into_owned();
        let path_owned = source_file.to_path_buf();
        let result =
            tokio::task::spawn_blocking(move || -> Result<Option<ParseCursor>, rusqlite::Error> {
                let conn = conn.lock().unwrap();
                let mut stmt = conn.prepare(
                    "SELECT offset, adapter_kind, updated_at_ms \
                     FROM capture_cursors WHERE source_file = ?1",
                )?;
                let result = stmt.query_row(params![key], |row| {
                    let offset: i64 = row.get(0)?;
                    let kind_str: String = row.get(1)?;
                    let updated_at_ms: i64 = row.get(2)?;
                    let adapter_kind = parse_tool_kind(&kind_str);
                    let updated_at = chrono::DateTime::from_timestamp_millis(updated_at_ms)
                        .unwrap_or_else(Utc::now);
                    Ok::<_, rusqlite::Error>(ParseCursor {
                        source_file: path_owned.clone(),
                        offset: offset as u64,
                        adapter_kind,
                        updated_at,
                    })
                });
                match result {
                    Ok(c) => Ok(Some(c)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(e),
                }
            })
            .await;
        result.ok().and_then(|r| r.ok()).flatten()
    }

    async fn save(&self, cursor: ParseCursor) {
        let conn = self.conn.clone();
        let key = cursor.source_file.to_string_lossy().into_owned();
        let offset = cursor.offset as i64;
        let kind_str = cursor.adapter_kind.as_str().to_string();
        let updated_at_ms = cursor.updated_at.timestamp_millis();
        let _ = tokio::task::spawn_blocking(move || -> Result<(), rusqlite::Error> {
            let conn = conn.lock().unwrap();
            conn.execute(
                r#"INSERT INTO capture_cursors (source_file, offset, adapter_kind, updated_at_ms)
                   VALUES (?1, ?2, ?3, ?4)
                   ON CONFLICT(source_file) DO UPDATE SET
                       offset = excluded.offset,
                       adapter_kind = excluded.adapter_kind,
                       updated_at_ms = excluded.updated_at_ms"#,
                params![key, offset, kind_str, updated_at_ms],
            )?;
            Ok(())
        })
        .await;
    }
}

fn parse_tool_kind(s: &str) -> ToolKind {
    match s {
        "claude_code" => ToolKind::ClaudeCode,
        "codex" => ToolKind::Codex,
        "open_code" => ToolKind::OpenCode,
        "cursor" => ToolKind::Cursor,
        "gemini" => ToolKind::Gemini,
        _ => ToolKind::ClaudeCode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn roundtrip() {
        let store =
            SqliteCursorStore::new(crate::cli::serve::storage::schema::open_in_memory().unwrap());
        let path = PathBuf::from("/tmp/sess.jsonl");
        let cursor = ParseCursor {
            source_file: path.clone(),
            offset: 4096,
            adapter_kind: ToolKind::ClaudeCode,
            updated_at: Utc::now(),
        };
        store.save(cursor).await;
        let loaded = store.load(&path).await;
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.offset, 4096);
        assert_eq!(loaded.adapter_kind, ToolKind::ClaudeCode);
    }

    #[tokio::test]
    async fn save_updates_existing() {
        let store =
            SqliteCursorStore::new(crate::cli::serve::storage::schema::open_in_memory().unwrap());
        let path = PathBuf::from("/tmp/sess.jsonl");
        store
            .save(ParseCursor {
                source_file: path.clone(),
                offset: 100,
                adapter_kind: ToolKind::ClaudeCode,
                updated_at: Utc::now(),
            })
            .await;
        store
            .save(ParseCursor {
                source_file: path.clone(),
                offset: 200,
                adapter_kind: ToolKind::ClaudeCode,
                updated_at: Utc::now(),
            })
            .await;
        let loaded = store.load(&path).await.unwrap();
        assert_eq!(loaded.offset, 200);
    }
}
