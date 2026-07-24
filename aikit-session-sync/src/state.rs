use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::SyncError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileFingerprint {
    pub mtime_ms: u128,
    pub size: u64,
}

impl FileFingerprint {
    pub fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        let mtime_ms = modified
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        Self {
            mtime_ms,
            size: metadata.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncStateEntry {
    pub last_synced_content_hash: String,
    pub mtime_ms: u128,
    pub size: u64,
}

impl SyncStateEntry {
    pub fn fingerprint(&self) -> FileFingerprint {
        FileFingerprint {
            mtime_ms: self.mtime_ms,
            size: self.size,
        }
    }
}

#[async_trait]
pub trait SyncStateStore: Send + Sync {
    async fn load(&self, source_file: &Path) -> Option<SyncStateEntry>;
    async fn save(&self, source_file: &Path, entry: SyncStateEntry) -> Result<(), SyncError>;
}

#[derive(Default, Clone)]
pub struct InMemorySyncStateStore {
    inner: Arc<Mutex<HashMap<PathBuf, SyncStateEntry>>>,
}

#[async_trait]
impl SyncStateStore for InMemorySyncStateStore {
    async fn load(&self, source_file: &Path) -> Option<SyncStateEntry> {
        self.inner.lock().unwrap().get(source_file).cloned()
    }

    async fn save(&self, source_file: &Path, entry: SyncStateEntry) -> Result<(), SyncError> {
        self.inner
            .lock()
            .unwrap()
            .insert(source_file.to_path_buf(), entry);
        Ok(())
    }
}

pub struct JsonSyncStateStore {
    path: PathBuf,
}

impl JsonSyncStateStore {
    pub fn open() -> std::io::Result<Self> {
        let root = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let dir = root.join(".aikit").join("session-sync");
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            path: dir.join("state.json"),
        })
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    fn load_all(&self) -> HashMap<String, SyncStateEntry> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    fn store_all(&self, map: &HashMap<String, SyncStateEntry>) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(map)?;
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(tmp, &self.path)?;
        Ok(())
    }
}

#[async_trait]
impl SyncStateStore for JsonSyncStateStore {
    async fn load(&self, source_file: &Path) -> Option<SyncStateEntry> {
        let map = self.load_all();
        map.get(source_file.to_string_lossy().as_ref()).cloned()
    }

    async fn save(&self, source_file: &Path, entry: SyncStateEntry) -> Result<(), SyncError> {
        let mut map = self.load_all();
        map.insert(source_file.to_string_lossy().into_owned(), entry);
        self.store_all(&map)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn json_state_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state.json");
        let store = JsonSyncStateStore::at(path);
        let source = tmp.path().join("s.jsonl");
        let entry = SyncStateEntry {
            last_synced_content_hash: "abc".into(),
            mtime_ms: 10,
            size: 20,
        };
        store.save(&source, entry.clone()).await.unwrap();
        assert_eq!(store.load(&source).await, Some(entry));
    }

    #[test]
    fn open_creates_state_dir_under_home() {
        let tmp = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("HOME");
        std::env::set_var("HOME", tmp.path());
        let store = JsonSyncStateStore::open().unwrap();
        assert!(store.path.ends_with("state.json"));
        assert!(tmp.path().join(".aikit").join("session-sync").is_dir());
        match prev {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    #[tokio::test]
    async fn load_missing_source_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = JsonSyncStateStore::at(tmp.path().join("state.json"));
        assert_eq!(store.load(&tmp.path().join("absent.jsonl")).await, None);
    }
}
