//! Offset cursors: persist where to resume parsing from on the next call.
//!
//! See spec 010 §10. Two reference impls ship in this crate; production
//! SQLite impls live in the host (aikit-serve).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::models::ToolKind;

/// One persisted cursor row.
#[derive(Debug, Clone)]
pub struct ParseCursor {
    pub source_file: PathBuf,
    /// Offset semantics are adapter-defined: byte offset for JSONL adapters,
    /// `time_updated` watermark for the SQLite-based OpenCode adapter.
    pub offset: u64,
    pub adapter_kind: ToolKind,
    pub updated_at: DateTime<Utc>,
}

/// On-disk JSON shape. `source_file` is the map key, not a field here.
#[derive(Serialize, Deserialize)]
struct StoredCursor {
    offset: u64,
    adapter_kind: ToolKind,
    updated_at: DateTime<Utc>,
}

/// Persists parse offsets so the next call resumes from the right place.
#[async_trait]
pub trait CursorStore: Send + Sync {
    async fn load(&self, source_file: &Path) -> Option<ParseCursor>;
    async fn save(&self, cursor: ParseCursor);
}

/// In-memory cursor store. For tests and ephemeral hosts.
#[derive(Default)]
pub struct InMemoryCursorStore {
    inner: Mutex<HashMap<PathBuf, ParseCursor>>,
}

#[async_trait]
impl CursorStore for InMemoryCursorStore {
    async fn load(&self, source_file: &Path) -> Option<ParseCursor> {
        self.inner.lock().unwrap().get(source_file).cloned()
    }
    async fn save(&self, cursor: ParseCursor) {
        self.inner
            .lock()
            .unwrap()
            .insert(cursor.source_file.clone(), cursor);
    }
}

/// JSON-on-disk cursor store. Atomic writes via temp-file + rename.
pub struct JsonSidecarCursorStore {
    path: PathBuf,
}

impl JsonSidecarCursorStore {
    pub fn open() -> std::io::Result<Self> {
        let root = dirs::config_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let dir = root.join(".aikit").join("adapters");
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            path: dir.join("cursors.json"),
        })
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    fn load_all(&self) -> HashMap<String, StoredCursor> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    fn store_all(&self, map: &HashMap<String, StoredCursor>) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(map)?;
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[async_trait]
impl CursorStore for JsonSidecarCursorStore {
    async fn load(&self, source_file: &Path) -> Option<ParseCursor> {
        let map = self.load_all();
        let key = source_file.to_string_lossy();
        let stored = map.get(key.as_ref())?;
        Some(ParseCursor {
            source_file: source_file.to_path_buf(),
            offset: stored.offset,
            adapter_kind: stored.adapter_kind,
            updated_at: stored.updated_at,
        })
    }

    async fn save(&self, cursor: ParseCursor) {
        let key = cursor.source_file.to_string_lossy().into_owned();
        let stored = StoredCursor {
            offset: cursor.offset,
            adapter_kind: cursor.adapter_kind,
            updated_at: cursor.updated_at,
        };
        let mut map = self.load_all();
        map.insert(key, stored);
        if let Err(e) = self.store_all(&map) {
            tracing::warn!(target: "aikit_session_capture::cursor", "cursor sidecar write failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_roundtrip() {
        let store = InMemoryCursorStore::default();
        let path = PathBuf::from("/tmp/sess.jsonl");
        let cursor = ParseCursor {
            source_file: path.clone(),
            offset: 4096,
            adapter_kind: ToolKind::ClaudeCode,
            updated_at: Utc::now(),
        };
        store.save(cursor.clone()).await;
        let loaded = store.load(&path).await.unwrap();
        assert_eq!(loaded.offset, 4096);
        assert_eq!(loaded.adapter_kind, ToolKind::ClaudeCode);
    }

    #[tokio::test]
    async fn json_sidecar_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = JsonSidecarCursorStore::at(tmp.path().join("cursors.json"));
        let path = PathBuf::from("/tmp/sess.jsonl");
        let cursor = ParseCursor {
            source_file: path.clone(),
            offset: 8192,
            adapter_kind: ToolKind::Codex,
            updated_at: Utc::now(),
        };
        store.save(cursor.clone()).await;
        let loaded = store.load(&path).await.unwrap();
        assert_eq!(loaded.offset, 8192);
        assert_eq!(loaded.adapter_kind, ToolKind::Codex);
    }
}
