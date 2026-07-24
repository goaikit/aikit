use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait SyncSink: Send + Sync {
    async fn put(&self, object: SyncObject) -> Result<(), SyncError>;
}

#[derive(Debug, Clone)]
pub struct SyncObject {
    pub key: String,
    pub content: Bytes,
    pub envelope: Envelope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Envelope {
    pub schema_version: u32,
    pub owner: String,
    pub tool: String,
    pub session_id: String,
    pub source_file: String,
    pub host: String,
    pub captured_at_ms: i64,
    pub content_hash: String,
    pub byte_len: u64,
    pub scrubber_version: u32,
    pub sync_tool_version: String,
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("backend: {0}")]
    Backend(String),
    #[error("auth: {0}")]
    Auth(String),
}

#[derive(Clone, Default)]
pub struct InMemorySink {
    inner: Arc<Mutex<InMemoryInner>>,
}

#[derive(Default)]
struct InMemoryInner {
    objects: HashMap<String, Bytes>,
    envelopes: HashMap<String, Envelope>,
    put_calls: usize,
    fail_meta_once: bool,
    fail_all: bool,
}

impl InMemorySink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_meta_failure_once() -> Self {
        let sink = Self::new();
        sink.inner.lock().unwrap().fail_meta_once = true;
        sink
    }

    pub fn with_backend_failure() -> Self {
        let sink = Self::new();
        sink.inner.lock().unwrap().fail_all = true;
        sink
    }

    pub fn object_count(&self) -> usize {
        self.inner.lock().unwrap().objects.len()
    }

    pub fn meta_count(&self) -> usize {
        self.inner.lock().unwrap().envelopes.len()
    }

    pub fn put_calls(&self) -> usize {
        self.inner.lock().unwrap().put_calls
    }

    pub fn get_content(&self, key: &str) -> Option<Bytes> {
        self.inner.lock().unwrap().objects.get(key).cloned()
    }

    pub fn get_envelope(&self, key: &str) -> Option<Envelope> {
        self.inner.lock().unwrap().envelopes.get(key).cloned()
    }

    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<_> = self.inner.lock().unwrap().objects.keys().cloned().collect();
        keys.sort();
        keys
    }
}

#[async_trait]
impl SyncSink for InMemorySink {
    async fn put(&self, object: SyncObject) -> Result<(), SyncError> {
        let mut inner = self.inner.lock().unwrap();
        inner.put_calls += 1;
        if inner.fail_all {
            return Err(SyncError::Backend("injected failure".to_string()));
        }
        inner
            .objects
            .entry(object.key.clone())
            .or_insert(object.content);
        if inner.fail_meta_once {
            inner.fail_meta_once = false;
            return Err(SyncError::Backend("injected meta failure".to_string()));
        }
        inner
            .envelopes
            .insert(meta_key(&object.key), object.envelope);
        Ok(())
    }
}

pub(crate) fn meta_key(content_key: &str) -> String {
    content_key
        .strip_suffix(".jsonl")
        .map(|base| format!("{base}.meta.json"))
        .unwrap_or_else(|| format!("{content_key}.meta.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_sink_writes_content_and_meta() {
        let sink = InMemorySink::new();
        let env = Envelope {
            schema_version: 1,
            owner: "owner".into(),
            tool: "codex".into(),
            session_id: "s".into(),
            source_file: "/tmp/s.jsonl".into(),
            host: "h".into(),
            captured_at_ms: 1,
            content_hash: "abc".into(),
            byte_len: 4,
            scrubber_version: 1,
            sync_tool_version: "0.1.0".into(),
        };
        sink.put(SyncObject {
            key: "sessions/owner/codex/s/abc.jsonl".into(),
            content: Bytes::from_static(b"body"),
            envelope: env.clone(),
        })
        .await
        .unwrap();
        assert_eq!(sink.object_count(), 1);
        assert_eq!(sink.meta_count(), 1);
        assert_eq!(
            sink.get_envelope("sessions/owner/codex/s/abc.meta.json"),
            Some(env)
        );
    }
}
