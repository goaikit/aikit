//! `SqliteEventStore` — production `EventStore` impl backed by SQLite.
//!
//! See spec 010 §11.2. The `(source_file, source_event_id)` primary key on
//! both event tables is the idempotency invariant: `INSERT OR IGNORE` makes
//! a full re-walk from offset 0 safe (previously-seen rows deduplicate to
//! zero insertions, so `deduplicated_count == events_upserted` on the second
//! `POST /capture/scan?force=true`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rusqlite::{params, Connection};
use serde_json::Value;

use aikit_session_capture::{
    ActionKind, ActionStatus, CacheObservation, CaptureSource, EventBatch, EventStore, FileTouch,
    SessionSummary, StoreError, TokenEvent, ToolEvent, ToolKind,
};

/// Map a rusqlite error into the crate-agnostic [`StoreError::Backend`] variant.
/// (Can't impl `From<rusqlite::Error>` due to the orphan rule — `StoreError`
/// is defined in `aikit-session-capture`.)
fn sqlite_err(e: rusqlite::Error) -> StoreError {
    StoreError::Backend(format!("sqlite: {e}"))
}

/// Production [`EventStore`] backed by a SQLite DB shared with
/// [`crate::cli::serve::storage::SqliteCursorStore`].
pub struct SqliteEventStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteEventStore {
    /// Wrap an existing shared connection (typically from
    /// [`crate::cli::serve::storage::schema::open`]).
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl EventStore for SqliteEventStore {
    async fn upsert_events(&self, events: EventBatch) -> Result<u64, StoreError> {
        let conn = self.conn.clone();
        let inserted = tokio::task::spawn_blocking(move || -> Result<u64, rusqlite::Error> {
            let mut conn = conn.lock().unwrap();
            let mut inserted = 0u64;
            let tx = conn.transaction()?;

            {
                let mut stmt = tx.prepare(
                    r#"INSERT OR IGNORE INTO capture_tool_events (
                        source_file, source_event_id, session_id, tool, kind,
                        target, input, output, status, error_message,
                        started_at_ms, duration_ms, git_root, metadata
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
                )?;
                for ev in &events.tool_events {
                    inserted += stmt.execute(params![
                        ev.source_file.to_string_lossy(),
                        &ev.source_event_id,
                        &ev.session_id,
                        ev.tool.as_str(),
                        ev.kind.as_str(),
                        ev.target.as_deref(),
                        ev.input.as_deref(),
                        ev.output.as_deref(),
                        action_status_str(ev.status),
                        ev.error_message.as_deref(),
                        ev.started_at_ms,
                        ev.duration_ms.map(|d| d as i64),
                        ev.git_root
                            .as_deref()
                            .map(|s| s.to_string_lossy().into_owned()),
                        ev.metadata.to_string(),
                    ])? as u64;
                }
            }

            {
                let mut stmt = tx.prepare(
                    r#"INSERT OR IGNORE INTO capture_token_events (
                        source_file, source_event_id, session_id, tool,
                        model, request_id,
                        input_tokens, cache_read_tokens, cache_creation_tokens,
                        cache_creation_1h_tokens, output_tokens, reasoning_tokens,
                        captured_at_ms, captured_via
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
                )?;
                for ev in &events.token_events {
                    let source_file = derive_token_source_file(&events.tool_events, ev);
                    inserted += stmt.execute(params![
                        source_file,
                        &ev.source_event_id,
                        &ev.session_id,
                        ev.tool.as_str(),
                        ev.model.as_deref(),
                        ev.request_id.as_deref(),
                        ev.input_tokens.map(|v| v as i64),
                        ev.cache_read_tokens.map(|v| v as i64),
                        ev.cache_creation_tokens.map(|v| v as i64),
                        ev.cache_creation_1h_tokens.map(|v| v as i64),
                        ev.output_tokens.map(|v| v as i64),
                        ev.reasoning_tokens.map(|v| v as i64),
                        ev.captured_at_ms,
                        capture_source_str(ev.captured_via),
                    ])? as u64;
                }
            }

            {
                let mut stmt = tx.prepare(
                    r#"INSERT OR IGNORE INTO capture_cache_observations (
                        source_file, source_event_id, session_id, tool,
                        cache_read_input_tokens, cache_creation_input_tokens,
                        cache_creation_1h_input_tokens, assistant_blocks_hash,
                        tools_changed, observed_at_ms
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
                )?;
                for ev in &events.cache_observations {
                    let source_file =
                        derive_cache_source_file(&events.tool_events, &events.token_events, ev);
                    inserted += stmt.execute(params![
                        source_file,
                        &ev.source_event_id,
                        &ev.session_id,
                        ev.tool.as_str(),
                        ev.cache_read_input_tokens.map(|v| v as i64),
                        ev.cache_creation_input_tokens.map(|v| v as i64),
                        ev.cache_creation_1h_input_tokens.map(|v| v as i64),
                        ev.assistant_blocks_hash.as_deref(),
                        serde_json::to_string(&ev.tools_changed).unwrap_or_else(|_| "[]".into()),
                        ev.observed_at_ms,
                    ])? as u64;
                }
            }

            tx.commit()?;
            Ok(inserted)
        })
        .await
        .map_err(|e| StoreError::Backend(format!("join error: {e}")))?
        .map_err(sqlite_err)?;

        Ok(inserted)
    }

    async fn sessions_for(
        &self,
        tool: ToolKind,
        cwd: Option<&Path>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<SessionSummary>, StoreError> {
        let conn = self.conn.clone();
        let cwd = cwd.map(|p| p.to_path_buf());
        let rows =
            tokio::task::spawn_blocking(move || -> Result<Vec<SessionSummary>, rusqlite::Error> {
                let conn = conn.lock().unwrap();
                let (sql, has_cwd): (&str, bool) = match &cwd {
                    Some(_) => (
                        "SELECT session_id, source_file, MIN(started_at_ms), MAX(started_at_ms), \
                     COUNT(*) as action_count, git_root \
                     FROM capture_tool_events \
                     WHERE tool = ?1 AND git_root = ?2 \
                     GROUP BY session_id ORDER BY MAX(started_at_ms) DESC LIMIT ?3 OFFSET ?4",
                        true,
                    ),
                    None => (
                        "SELECT session_id, source_file, MIN(started_at_ms), MAX(started_at_ms), \
                     COUNT(*) as action_count, git_root \
                     FROM capture_tool_events \
                     WHERE tool = ?1 \
                     GROUP BY session_id ORDER BY MAX(started_at_ms) DESC LIMIT ?2 OFFSET ?3",
                        false,
                    ),
                };
                let mut stmt = conn.prepare(sql)?;
                let closure = |row: &rusqlite::Row| -> rusqlite::Result<SessionSummary> {
                    let session_id: String = row.get(0)?;
                    let source_file: String = row.get(1)?;
                    let first_event_at_ms: i64 = row.get(2)?;
                    let last_event_at_ms: i64 = row.get(3)?;
                    let action_count: i64 = row.get(4)?;
                    let git_root: Option<String> = row.get(5)?;
                    let tool_kinds = tool_kinds_for_session(&conn, tool, &session_id)?;
                    Ok(SessionSummary {
                        tool,
                        session_id,
                        source_file: PathBuf::from(source_file),
                        first_event_at_ms,
                        last_event_at_ms,
                        action_count: action_count as u64,
                        tool_kinds,
                        git_root: git_root.map(PathBuf::from),
                    })
                };
                let iter = match (&cwd, has_cwd) {
                    (Some(c), true) => stmt.query_map(
                        params![tool.as_str(), c.to_string_lossy(), limit, offset],
                        closure,
                    )?,
                    _ => stmt.query_map(params![tool.as_str(), limit, offset], closure)?,
                };
                iter.collect()
            })
            .await
            .map_err(|e| StoreError::Backend(format!("join error: {e}")))?
            .map_err(sqlite_err)?;
        Ok(rows)
    }

    async fn actions_for_session(
        &self,
        tool: ToolKind,
        session_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<ToolEvent>, StoreError> {
        let conn = self.conn.clone();
        let session_id = session_id.to_string();
        let rows =
            tokio::task::spawn_blocking(move || -> Result<Vec<ToolEvent>, rusqlite::Error> {
                let conn = conn.lock().unwrap();
                let mut stmt = conn.prepare(
                    r#"SELECT source_event_id, source_file, session_id, tool, kind,
                          target, input, output, status, error_message,
                          started_at_ms, duration_ms, git_root, metadata
                   FROM capture_tool_events
                   WHERE tool = ?1 AND session_id = ?2
                   ORDER BY started_at_ms ASC LIMIT ?3 OFFSET ?4"#,
                )?;
                let iter =
                    stmt.query_map(params![tool.as_str(), &session_id, limit, offset], |row| {
                        let metadata_str: String = row.get(13)?;
                        let metadata: Value =
                            serde_json::from_str(&metadata_str).unwrap_or(Value::Null);
                        let duration_ms: Option<i64> = row.get(11)?;
                        Ok(ToolEvent {
                            source_event_id: row.get(0)?,
                            source_file: PathBuf::from(row.get::<_, String>(1)?),
                            session_id: row.get(2)?,
                            tool,
                            kind: parse_action_kind(&row.get::<_, String>(3)?),
                            target: row.get::<_, Option<String>>(4)?.filter(|s| !s.is_empty()),
                            input: row.get::<_, Option<String>>(5)?.filter(|s| !s.is_empty()),
                            output: row.get::<_, Option<String>>(6)?.filter(|s| !s.is_empty()),
                            status: parse_action_status(&row.get::<_, String>(7)?),
                            error_message: row
                                .get::<_, Option<String>>(8)?
                                .filter(|s| !s.is_empty()),
                            started_at_ms: row.get(9)?,
                            duration_ms: duration_ms.map(|d| d as u64),
                            git_root: row
                                .get::<_, Option<String>>(12)?
                                .filter(|s| !s.is_empty())
                                .map(PathBuf::from),
                            metadata,
                        })
                    })?;
                iter.collect()
            })
            .await
            .map_err(|e| StoreError::Backend(format!("join error: {e}")))?
            .map_err(sqlite_err)?;
        Ok(rows)
    }

    async fn search_outputs(&self, query: &str, limit: u32) -> Result<Vec<ToolEvent>, StoreError> {
        let conn = self.conn.clone();
        let query = format!("%{query}%");
        let rows =
            tokio::task::spawn_blocking(move || -> Result<Vec<ToolEvent>, rusqlite::Error> {
                let conn = conn.lock().unwrap();
                let mut stmt = conn.prepare(
                    r#"SELECT source_event_id, source_file, session_id, tool, kind,
                          target, input, output, status, error_message,
                          started_at_ms, duration_ms, git_root, metadata
                   FROM capture_tool_events
                   WHERE output LIKE ?1
                   ORDER BY started_at_ms DESC LIMIT ?2"#,
                )?;
                let iter = stmt.query_map(params![&query, limit], |row| {
                    let tool_str: String = row.get(3)?;
                    let metadata_str: String = row.get(13)?;
                    let metadata: Value =
                        serde_json::from_str(&metadata_str).unwrap_or(Value::Null);
                    let duration_ms: Option<i64> = row.get(11)?;
                    Ok(ToolEvent {
                        source_event_id: row.get(0)?,
                        source_file: PathBuf::from(row.get::<_, String>(1)?),
                        session_id: row.get(2)?,
                        tool: parse_tool_kind(&tool_str),
                        kind: parse_action_kind(&row.get::<_, String>(4)?),
                        target: row.get::<_, Option<String>>(5)?.filter(|s| !s.is_empty()),
                        input: row.get::<_, Option<String>>(6)?.filter(|s| !s.is_empty()),
                        output: row.get::<_, Option<String>>(7)?.filter(|s| !s.is_empty()),
                        status: parse_action_status(&row.get::<_, String>(8)?),
                        error_message: row.get::<_, Option<String>>(9)?.filter(|s| !s.is_empty()),
                        started_at_ms: row.get(10)?,
                        duration_ms: duration_ms.map(|d| d as u64),
                        git_root: row
                            .get::<_, Option<String>>(12)?
                            .filter(|s| !s.is_empty())
                            .map(PathBuf::from),
                        metadata,
                    })
                })?;
                iter.collect()
            })
            .await
            .map_err(|e| StoreError::Backend(format!("join error: {e}")))?
            .map_err(sqlite_err)?;
        Ok(rows)
    }

    async fn token_events_for_session(
        &self,
        tool: ToolKind,
        session_id: &str,
    ) -> Result<Vec<TokenEvent>, StoreError> {
        let conn = self.conn.clone();
        let session_id = session_id.to_string();
        let rows =
            tokio::task::spawn_blocking(move || -> Result<Vec<TokenEvent>, rusqlite::Error> {
                let conn = conn.lock().unwrap();
                let mut stmt = conn.prepare(
                    r#"SELECT source_event_id, session_id, tool, model, request_id,
                          input_tokens, cache_read_tokens, cache_creation_tokens,
                          cache_creation_1h_tokens, output_tokens, reasoning_tokens,
                          captured_at_ms, captured_via
                   FROM capture_token_events
                   WHERE tool = ?1 AND session_id = ?2
                   ORDER BY captured_at_ms ASC"#,
                )?;
                let iter = stmt.query_map(params![tool.as_str(), &session_id], |row| {
                    let captured_via_str: String = row.get(12)?;
                    let input_tokens: Option<i64> = row.get(5)?;
                    let cache_read_tokens: Option<i64> = row.get(6)?;
                    let cache_creation_tokens: Option<i64> = row.get(7)?;
                    let cache_creation_1h_tokens: Option<i64> = row.get(8)?;
                    let output_tokens: Option<i64> = row.get(9)?;
                    let reasoning_tokens: Option<i64> = row.get(10)?;
                    Ok(TokenEvent {
                        source_event_id: row.get(0)?,
                        session_id: row.get(1)?,
                        tool,
                        model: row.get::<_, Option<String>>(3)?.filter(|s| !s.is_empty()),
                        request_id: row.get::<_, Option<String>>(4)?.filter(|s| !s.is_empty()),
                        input_tokens: input_tokens.map(|v| v as u64),
                        cache_read_tokens: cache_read_tokens.map(|v| v as u64),
                        cache_creation_tokens: cache_creation_tokens.map(|v| v as u64),
                        cache_creation_1h_tokens: cache_creation_1h_tokens.map(|v| v as u64),
                        output_tokens: output_tokens.map(|v| v as u64),
                        reasoning_tokens: reasoning_tokens.map(|v| v as u64),
                        captured_at_ms: row.get(11)?,
                        captured_via: parse_capture_source(&captured_via_str),
                    })
                })?;
                iter.collect()
            })
            .await
            .map_err(|e| StoreError::Backend(format!("join error: {e}")))?
            .map_err(sqlite_err)?;
        Ok(rows)
    }

    async fn last_file_touch(&self, path: &Path) -> Result<Option<FileTouch>, StoreError> {
        let conn = self.conn.clone();
        let path_str = path.to_string_lossy().into_owned();
        let path_buf = path.to_path_buf();
        let result =
            tokio::task::spawn_blocking(move || -> Result<Option<FileTouch>, rusqlite::Error> {
                let conn = conn.lock().unwrap();
                let mut stmt = conn.prepare(
                    r#"SELECT MAX(started_at_ms) FROM capture_tool_events
                   WHERE kind IN ('read', 'edit', 'write')
                     AND target = ?1"#,
                )?;
                let last_read: Option<i64> =
                    stmt.query_row(params![&path_str], |row| row.get(0))?;
                let last_modified = std::fs::metadata(&path_buf)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .ok()
                            .map(|d| d.as_millis() as i64)
                    });
                if last_read.is_none() && last_modified.is_none() {
                    return Ok(None);
                }
                Ok(Some(FileTouch {
                    path: path_buf.clone(),
                    last_read_at_ms: last_read,
                    last_modified_at_ms: last_modified,
                }))
            })
            .await
            .map_err(|e| StoreError::Backend(format!("join error: {e}")))?
            .map_err(sqlite_err)?;
        Ok(result)
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Token events don't carry their own `source_file`; derive it from the batch's
/// tool events (same session).
fn derive_token_source_file(tool_events: &[ToolEvent], token: &TokenEvent) -> String {
    if let Some(ev) = tool_events
        .iter()
        .find(|e| e.session_id == token.session_id)
    {
        return ev.source_file.to_string_lossy().into_owned();
    }
    // Fallback: use the token's session_id as a synthetic source key. This is
    // only hit when a batch has token events without corresponding tool events
    // (rare but valid — a turn with no tool calls still emits token counts).
    format!("session:{}", token.session_id)
}

fn derive_cache_source_file(
    tool_events: &[ToolEvent],
    token_events: &[TokenEvent],
    cache: &CacheObservation,
) -> String {
    if let Some(ev) = tool_events
        .iter()
        .find(|e| e.session_id == cache.session_id && e.tool == cache.tool)
    {
        return ev.source_file.to_string_lossy().into_owned();
    }
    if let Some(token) = token_events
        .iter()
        .find(|e| e.session_id == cache.session_id && e.tool == cache.tool)
    {
        return format!("session:{}", token.session_id);
    }
    format!("session:{}", cache.session_id)
}

fn tool_kinds_for_session(
    conn: &Connection,
    tool: ToolKind,
    session_id: &str,
) -> rusqlite::Result<Vec<ActionKind>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT kind FROM capture_tool_events WHERE tool = ?1 AND session_id = ?2 \
         ORDER BY kind ASC",
    )?;
    let iter = stmt.query_map(params![tool.as_str(), session_id], |row| {
        let s: String = row.get(0)?;
        Ok(parse_action_kind(&s))
    })?;
    iter.collect()
}

fn action_status_str(s: ActionStatus) -> &'static str {
    match s {
        ActionStatus::Success => "success",
        ActionStatus::Failure => "failure",
        ActionStatus::Cancelled => "cancelled",
        ActionStatus::Unknown => "unknown",
        _ => "unknown",
    }
}

fn parse_action_status(s: &str) -> ActionStatus {
    match s {
        "success" => ActionStatus::Success,
        "failure" => ActionStatus::Failure,
        "cancelled" => ActionStatus::Cancelled,
        _ => ActionStatus::Unknown,
    }
}

fn parse_action_kind(s: &str) -> ActionKind {
    match s {
        "read" => ActionKind::Read,
        "write" => ActionKind::Write,
        "edit" => ActionKind::Edit,
        "delete" => ActionKind::Delete,
        "bash" => ActionKind::Bash,
        "glob" => ActionKind::Glob,
        "grep" => ActionKind::Grep,
        "search" => ActionKind::Search,
        "web_fetch" => ActionKind::WebFetch,
        "web_search" => ActionKind::WebSearch,
        "mcp" => ActionKind::Mcp,
        "think" => ActionKind::Think,
        "plan" => ActionKind::Plan,
        "subagent" => ActionKind::Subagent,
        _ => ActionKind::Other,
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

fn capture_source_str(s: CaptureSource) -> &'static str {
    match s {
        CaptureSource::Transcript => "transcript",
        CaptureSource::Proxy => "proxy",
        _ => "transcript",
    }
}

fn parse_capture_source(s: &str) -> CaptureSource {
    match s {
        "proxy" => CaptureSource::Proxy,
        _ => CaptureSource::Transcript,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_session_capture::EventBatch;

    fn sample_tool_event(id: &str, sess: &str) -> ToolEvent {
        ToolEvent {
            source_event_id: id.into(),
            source_file: PathBuf::from("/tmp/sess.jsonl"),
            session_id: sess.into(),
            tool: ToolKind::ClaudeCode,
            kind: ActionKind::Read,
            target: Some("/tmp/file.go".into()),
            input: None,
            output: Some("package main".into()),
            status: ActionStatus::Success,
            error_message: None,
            started_at_ms: Some(1000),
            duration_ms: Some(5),
            git_root: Some(PathBuf::from("/tmp")),
            metadata: Value::Null,
        }
    }

    #[tokio::test]
    async fn upsert_is_idempotent() {
        let store =
            SqliteEventStore::new(crate::cli::serve::storage::schema::open_in_memory().unwrap());
        let batch = EventBatch {
            tool_events: vec![sample_tool_event("1", "s1")],
            token_events: vec![],
            cache_observations: vec![],
        };
        let n1 = store.upsert_events(batch.clone()).await.unwrap();
        let n2 = store.upsert_events(batch).await.unwrap();
        assert_eq!(n1, 1);
        assert_eq!(n2, 0, "second upsert of same batch must dedupe to zero");
    }

    #[tokio::test]
    async fn sessions_for_returns_one_per_session() {
        let store =
            SqliteEventStore::new(crate::cli::serve::storage::schema::open_in_memory().unwrap());
        store
            .upsert_events(EventBatch {
                tool_events: vec![
                    sample_tool_event("1", "s1"),
                    sample_tool_event("2", "s1"),
                    sample_tool_event("3", "s2"),
                ],
                token_events: vec![],
                cache_observations: vec![],
            })
            .await
            .unwrap();
        let got = store
            .sessions_for(ToolKind::ClaudeCode, None, 10, 0)
            .await
            .unwrap();
        assert_eq!(got.len(), 2);
    }

    #[tokio::test]
    async fn search_outputs_matches_substring() {
        let store =
            SqliteEventStore::new(crate::cli::serve::storage::schema::open_in_memory().unwrap());
        store
            .upsert_events(EventBatch {
                tool_events: vec![sample_tool_event("1", "s1")],
                token_events: vec![],
                cache_observations: vec![],
            })
            .await
            .unwrap();
        let got = store.search_outputs("package", 10).await.unwrap();
        assert_eq!(got.len(), 1);
    }
}
