//! `EventStore`: the trait the host implements to persist and query parsed
//! events. Distinct from [`CursorStore`][crate::CursorStore] — see spec 010 §11.
//!
//! The crate ships [`InMemoryEventStore`] for tests. Production SQLite impl
//! lives in `aikit-serve/src/storage/`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use async_trait::async_trait;

use crate::models::{ActionKind, CacheObservation, TokenEvent, ToolEvent, ToolKind};

/// Argument to [`EventStore::upsert_events`]. Each field may be empty.
#[derive(Debug, Clone, Default)]
pub struct EventBatch {
    pub tool_events: Vec<ToolEvent>,
    pub token_events: Vec<TokenEvent>,
    pub cache_observations: Vec<CacheObservation>,
}

/// One row returned by [`EventStore::sessions_for`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionSummary {
    pub tool: ToolKind,
    pub session_id: String,
    pub source_file: PathBuf,
    pub first_event_at_ms: i64,
    pub last_event_at_ms: i64,
    pub action_count: u64,
    pub tool_kinds: Vec<ActionKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_root: Option<PathBuf>,
}

/// Return value of [`EventStore::last_file_touch`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileTouch {
    pub path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_read_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified_at_ms: Option<i64>,
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// Backend-specific failure (SQLite locked, rusqlite error, etc.). The
    /// string is for diagnostics; callers do not pattern-match on it.
    #[error("backend: {0}")]
    Backend(String),
}

/// Persists parsed events and answers queries over them. Host-implemented;
/// the trait is the contract.
#[async_trait]
pub trait EventStore: Send + Sync {
    /// Upsert events. `(source_file, source_event_id)` is the unique key;
    /// re-inserting the same key MUST be a no-op (idempotent upsert).
    /// Returns the number of rows actually inserted (excluding deduplicated
    /// upserts). Callers compare against batch size to compute
    /// `deduplicated_count`.
    async fn upsert_events(&self, events: EventBatch) -> Result<u64, StoreError>;

    /// List parsed sessions for one Backend, optionally filtered by cwd.
    async fn sessions_for(
        &self,
        tool: ToolKind,
        cwd: Option<&Path>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<SessionSummary>, StoreError>;

    /// Full action stream for one session, paginated.
    async fn actions_for_session(
        &self,
        tool: ToolKind,
        session_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<ToolEvent>, StoreError>;

    /// Substring/regex search over `ToolEvent.output`. Used by MCP
    /// `search_past_outputs`. Hosts with FTS5 should override.
    async fn search_outputs(&self, query: &str, limit: u32) -> Result<Vec<ToolEvent>, StoreError>;

    /// Cost-engine pull accessor (spec 009 integration, spec 010 §16).
    async fn token_events_for_session(
        &self,
        tool: ToolKind,
        session_id: &str,
    ) -> Result<Vec<TokenEvent>, StoreError>;

    /// Freshness join: most recent `Read`/`Edit`/`Write` of `path` across
    /// every session. Used by MCP `check_file_freshness`.
    async fn last_file_touch(&self, path: &Path) -> Result<Option<FileTouch>, StoreError>;
}

/// In-memory event store. For tests and the `CliTestHarness` MCP integration
/// tests (spec 010 Phase 5). Production hosts use SQLite (lives in
/// aikit-serve, not this crate).
#[derive(Default)]
pub struct InMemoryEventStore {
    tool_events: RwLock<Vec<ToolEvent>>,
    token_events: RwLock<Vec<TokenEvent>>,
    cache_observations: RwLock<Vec<CacheObservation>>,
    // Index: source_event_id → position in tool_events. Enforces idempotency.
    seen: RwLock<HashMap<String, usize>>,
    seen_cache: RwLock<HashMap<String, usize>>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl EventStore for InMemoryEventStore {
    async fn upsert_events(&self, events: EventBatch) -> Result<u64, StoreError> {
        let mut seen = self.seen.write().unwrap();
        let mut te = self.tool_events.write().unwrap();
        let mut tk = self.token_events.write().unwrap();
        let mut inserted = 0u64;

        for ev in events.tool_events {
            let key = format!("{}:{}", ev.source_file.display(), ev.source_event_id);
            if seen.contains_key(&key) {
                continue;
            }
            seen.insert(key, te.len());
            te.push(ev);
            inserted += 1;
        }
        // Token events dedupe by their own source_event_id within the session.
        let mut tk_seen: HashMap<String, usize> = HashMap::new();
        for (i, e) in tk.iter().enumerate() {
            tk_seen.insert(format!("{}:{}", e.session_id, e.source_event_id), i);
        }
        for ev in events.token_events {
            let key = format!("{}:{}", ev.session_id, ev.source_event_id);
            if tk_seen.contains_key(&key) {
                continue;
            }
            tk_seen.insert(key, tk.len());
            tk.push(ev);
            inserted += 1;
        }
        let mut cache_seen = self.seen_cache.write().unwrap();
        let mut cache = self.cache_observations.write().unwrap();
        for ev in events.cache_observations {
            let key = format!("{}:{}", ev.session_id, ev.source_event_id);
            if cache_seen.contains_key(&key) {
                continue;
            }
            cache_seen.insert(key, cache.len());
            cache.push(ev);
            inserted += 1;
        }
        Ok(inserted)
    }

    async fn sessions_for(
        &self,
        tool: ToolKind,
        cwd: Option<&Path>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<SessionSummary>, StoreError> {
        let te = self.tool_events.read().unwrap();
        let mut by_session: HashMap<(ToolKind, String), Vec<&ToolEvent>> = HashMap::new();
        for ev in te.iter() {
            if ev.tool != tool {
                continue;
            }
            if let Some(cwd) = cwd {
                let root = ev
                    .git_root
                    .as_deref()
                    .unwrap_or_else(|| ev.source_file.parent().unwrap_or(Path::new("")));
                if root != cwd {
                    continue;
                }
            }
            by_session
                .entry((ev.tool, ev.session_id.clone()))
                .or_default()
                .push(ev);
        }
        let mut summaries: Vec<SessionSummary> = by_session
            .into_iter()
            .map(|((tool, session_id), evs)| {
                let action_count = evs.len() as u64;
                let first = evs
                    .iter()
                    .filter_map(|e| e.started_at_ms)
                    .min()
                    .unwrap_or(0);
                let last = evs
                    .iter()
                    .filter_map(|e| e.started_at_ms)
                    .max()
                    .unwrap_or(0);
                let mut kinds: Vec<ActionKind> = evs.iter().map(|e| e.kind).collect();
                // `Discriminant<ActionKind>` is not `Ord`; sort by variant
                // tag string for a stable, deterministic order.
                kinds.sort_by_key(|k| k.as_str());
                kinds.dedup_by(|a, b| a.as_str() == b.as_str());
                SessionSummary {
                    tool,
                    session_id,
                    source_file: evs[0].source_file.clone(),
                    first_event_at_ms: first,
                    last_event_at_ms: last,
                    action_count,
                    tool_kinds: kinds,
                    git_root: evs[0].git_root.clone(),
                }
            })
            .collect();
        summaries.sort_by_key(|s| s.last_event_at_ms);
        let off = (offset as usize).min(summaries.len());
        let end = (off + limit as usize).min(summaries.len());
        Ok(summaries[off..end].to_vec())
    }

    async fn actions_for_session(
        &self,
        tool: ToolKind,
        session_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<ToolEvent>, StoreError> {
        let te = self.tool_events.read().unwrap();
        let mut matches: Vec<&ToolEvent> = te
            .iter()
            .filter(|e| e.tool == tool && e.session_id == session_id)
            .collect();
        matches.sort_by_key(|e| e.started_at_ms.unwrap_or(0));
        let off = (offset as usize).min(matches.len());
        let end = (off + limit as usize).min(matches.len());
        Ok(matches[off..end].iter().map(|e| (*e).clone()).collect())
    }

    async fn search_outputs(&self, query: &str, limit: u32) -> Result<Vec<ToolEvent>, StoreError> {
        let te = self.tool_events.read().unwrap();
        let q = query.to_lowercase();
        let matches: Vec<ToolEvent> = te
            .iter()
            .filter(|e| {
                e.output
                    .as_deref()
                    .map(|o| o.to_lowercase().contains(&q))
                    .unwrap_or(false)
            })
            .take(limit as usize)
            .cloned()
            .collect();
        Ok(matches)
    }

    async fn token_events_for_session(
        &self,
        tool: ToolKind,
        session_id: &str,
    ) -> Result<Vec<TokenEvent>, StoreError> {
        let tk = self.token_events.read().unwrap();
        Ok(tk
            .iter()
            .filter(|e| e.tool == tool && e.session_id == session_id)
            .cloned()
            .collect())
    }

    async fn last_file_touch(&self, path: &Path) -> Result<Option<FileTouch>, StoreError> {
        let te = self.tool_events.read().unwrap();
        let last_read = te
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    ActionKind::Read | ActionKind::Edit | ActionKind::Write
                ) && e.target.as_deref() == Some(path.to_string_lossy().as_ref())
            })
            .filter_map(|e| e.started_at_ms)
            .max();
        let last_modified = std::fs::metadata(path)
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
            path: path.to_path_buf(),
            last_read_at_ms: last_read,
            last_modified_at_ms: last_modified,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ActionStatus, CaptureSource};

    fn ev(id: &str, sess: &str, kind: ActionKind, started: i64) -> ToolEvent {
        ToolEvent {
            source_event_id: id.into(),
            source_file: PathBuf::from("/tmp/sess.jsonl"),
            session_id: sess.into(),
            tool: ToolKind::ClaudeCode,
            kind,
            target: None,
            input: None,
            output: None,
            status: ActionStatus::Success,
            error_message: None,
            started_at_ms: Some(started),
            duration_ms: None,
            git_root: None,
            metadata: serde_json::Value::Null,
        }
    }

    #[tokio::test]
    async fn upsert_is_idempotent() {
        let store = InMemoryEventStore::new();
        let batch = EventBatch {
            tool_events: vec![ev("1", "s1", ActionKind::Read, 100)],
            token_events: vec![],
            cache_observations: vec![],
        };
        let n1 = store.upsert_events(batch.clone()).await.unwrap();
        let n2 = store.upsert_events(batch).await.unwrap();
        assert_eq!(n1, 1);
        assert_eq!(n2, 0, "second upsert of same batch must dedupe to zero");
    }

    #[tokio::test]
    async fn sessions_for_filters_by_tool() {
        let store = InMemoryEventStore::new();
        store
            .upsert_events(EventBatch {
                tool_events: vec![
                    ev("1", "s1", ActionKind::Read, 100),
                    ev("2", "s1", ActionKind::Edit, 200),
                    ev("3", "s2", ActionKind::Read, 50),
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
        let store = InMemoryEventStore::new();
        let mut e = ev("1", "s1", ActionKind::Bash, 100);
        e.output = Some("go test ./...\nok main 0.1s".into());
        store
            .upsert_events(EventBatch {
                tool_events: vec![e],
                token_events: vec![],
                cache_observations: vec![],
            })
            .await
            .unwrap();
        let got = store.search_outputs("main", 10).await.unwrap();
        assert_eq!(got.len(), 1);
    }

    #[tokio::test]
    async fn token_events_absence_is_none() {
        // Sanity-check the CaptureSource variant deserializes cleanly.
        let s = serde_json::to_string(&CaptureSource::Transcript).unwrap();
        assert_eq!(s, "\"transcript\"");
    }
}
