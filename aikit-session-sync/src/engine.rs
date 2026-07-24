use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aikit_session_capture::{
    Adapter, Registry, SecretScrubber, ToolKind, SCRUBBER_PATTERN_VERSION,
};
use bytes::Bytes;
use chrono::Utc;
use rand::Rng;
use sha2::{Digest, Sha256};
use tokio::time::sleep;

use crate::key::object_key;
use crate::state::{FileFingerprint, SyncStateEntry, SyncStateStore};
use crate::{Envelope, SyncError, SyncObject, SyncSink};

#[derive(Debug, Clone)]
pub struct SyncConfig {
    pub bucket: Option<String>,
    pub endpoint: Option<String>,
    pub region: String,
    pub allow_http: bool,
    pub endpoint_ca_bundle: Option<PathBuf>,
    pub path_style: bool,
    pub owner: Option<String>,
    pub credential_owner: Option<String>,
    pub key_prefix: String,
    pub tools: Option<Vec<ToolKind>>,
    pub watch: bool,
    pub dry_run: bool,
    pub format: OutputFormat,
    pub log_level: String,
    pub host: String,
    pub state_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Default,
    Json,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            bucket: None,
            endpoint: None,
            region: "us-east-1".to_string(),
            allow_http: false,
            endpoint_ca_bundle: None,
            path_style: true,
            owner: None,
            credential_owner: None,
            key_prefix: "sessions/".to_string(),
            tools: None,
            watch: false,
            dry_run: false,
            format: OutputFormat::Default,
            log_level: "info".to_string(),
            host: default_host(),
            state_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    Synced { key: String, bytes_uploaded: u64 },
    SkippedUnchanged,
    Failed { error: String },
}

#[derive(Debug, Default, Clone, serde::Serialize, PartialEq, Eq)]
pub struct SyncRunSummary {
    pub synced: u64,
    pub skipped_unchanged: u64,
    pub failed: u64,
    pub bytes_uploaded: u64,
}

impl SyncRunSummary {
    pub fn record(&mut self, outcome: &SyncOutcome) {
        match outcome {
            SyncOutcome::Synced { bytes_uploaded, .. } => {
                self.synced += 1;
                self.bytes_uploaded += *bytes_uploaded;
            }
            SyncOutcome::SkippedUnchanged => self.skipped_unchanged += 1,
            SyncOutcome::Failed { .. } => self.failed += 1,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WatchRetryPolicy {
    pub base: Duration,
    pub cap: Duration,
}

impl Default for WatchRetryPolicy {
    fn default() -> Self {
        Self {
            base: Duration::from_secs(1),
            cap: Duration::from_secs(60),
        }
    }
}

pub struct SyncEngine {
    config: SyncConfig,
    owner: String,
    sink: Arc<dyn SyncSink>,
    state: Arc<dyn SyncStateStore>,
    scrubber: SecretScrubber,
}

impl SyncEngine {
    pub fn new(
        config: SyncConfig,
        sink: Arc<dyn SyncSink>,
        state: Arc<dyn SyncStateStore>,
    ) -> Result<Self, SyncError> {
        let owner = resolve_owner(config.owner.as_deref(), config.credential_owner.as_deref())?;
        Ok(Self {
            config,
            owner,
            sink,
            state,
            scrubber: SecretScrubber::default(),
        })
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub async fn sync_detected(&self, registry: &Registry) -> SyncRunSummary {
        let mut summary = SyncRunSummary::default();
        for adapter in jsonl_adapters(registry, self.config.tools.as_deref()) {
            for file in session_files(adapter) {
                let outcome = match self.sync_file(adapter, &file).await {
                    Ok(outcome) => outcome,
                    Err(error) => SyncOutcome::Failed {
                        error: error.to_string(),
                    },
                };
                summary.record(&outcome);
            }
        }
        summary
    }

    pub async fn sync_file(
        &self,
        adapter: &dyn Adapter,
        path: &Path,
    ) -> Result<SyncOutcome, SyncError> {
        if !is_jsonl_kind(adapter.kind()) || !adapter.is_session_file(path) {
            return Ok(SyncOutcome::SkippedUnchanged);
        }

        let metadata = tokio::fs::metadata(path).await?;
        let fingerprint = FileFingerprint::from_metadata(&metadata);
        if let Some(cached) = self.state.load(path).await {
            if cached.fingerprint() == fingerprint {
                return Ok(SyncOutcome::SkippedUnchanged);
            }
        }

        let raw = tokio::fs::read(path).await?;
        let raw_str = std::str::from_utf8(&raw).map_err(|e| {
            SyncError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("session file is not valid UTF-8: {e}"),
            ))
        })?;
        let scrubbed = self.scrubber.scrub(raw_str);
        let scrubbed_bytes = Bytes::from(scrubbed.into_bytes());
        let content_hash = hex::encode(Sha256::digest(&scrubbed_bytes));

        if let Some(cached) = self.state.load(path).await {
            if cached.last_synced_content_hash == content_hash {
                self.state
                    .save(
                        path,
                        SyncStateEntry {
                            last_synced_content_hash: content_hash,
                            mtime_ms: fingerprint.mtime_ms,
                            size: fingerprint.size,
                        },
                    )
                    .await?;
                return Ok(SyncOutcome::SkippedUnchanged);
            }
        }

        let session_id = session_id_for_path(path);
        let captured_at_ms = Utc::now().timestamp_millis();
        let key = object_key(
            &self.config.key_prefix,
            &self.owner,
            adapter.kind(),
            &session_id,
            &content_hash,
        );
        let envelope = Envelope {
            schema_version: 1,
            owner: self.owner.clone(),
            tool: adapter.kind().as_str().to_string(),
            session_id,
            source_file: path.to_string_lossy().into_owned(),
            host: self.config.host.clone(),
            captured_at_ms,
            content_hash: content_hash.clone(),
            byte_len: scrubbed_bytes.len() as u64,
            scrubber_version: SCRUBBER_PATTERN_VERSION,
            sync_tool_version: env!("CARGO_PKG_VERSION").to_string(),
        };

        if self.config.dry_run {
            return Ok(SyncOutcome::Synced {
                key,
                bytes_uploaded: scrubbed_bytes.len() as u64,
            });
        }

        self.sink
            .put(SyncObject {
                key: key.clone(),
                content: scrubbed_bytes.clone(),
                envelope,
            })
            .await?;
        self.state
            .save(
                path,
                SyncStateEntry {
                    last_synced_content_hash: content_hash,
                    mtime_ms: fingerprint.mtime_ms,
                    size: fingerprint.size,
                },
            )
            .await?;
        Ok(SyncOutcome::Synced {
            key,
            bytes_uploaded: scrubbed_bytes.len() as u64,
        })
    }

    pub async fn retry_with_backoff(
        &self,
        adapter: &dyn Adapter,
        path: &Path,
        attempts: usize,
        policy: WatchRetryPolicy,
    ) -> Result<SyncOutcome, SyncError> {
        let mut delay = policy.base;
        let mut last_error = None;
        for attempt in 0..attempts {
            match self.sync_file(adapter, path).await {
                Ok(outcome) => return Ok(outcome),
                Err(error) => {
                    last_error = Some(error);
                    if attempt + 1 < attempts {
                        let jitter = rand::thread_rng().gen_range(0..=delay.as_millis() / 4);
                        sleep(delay + Duration::from_millis(jitter as u64)).await;
                        delay = delay.saturating_mul(2).min(policy.cap);
                    }
                }
            }
        }
        Err(last_error.unwrap_or_else(|| SyncError::Backend("retry exhausted".into())))
    }
}

pub fn resolve_owner(
    explicit_owner: Option<&str>,
    credential_owner: Option<&str>,
) -> Result<String, SyncError> {
    match (explicit_owner, credential_owner) {
        (Some(explicit), Some(derived)) if explicit == derived => Ok(explicit.to_string()),
        (Some(explicit), Some(derived)) => Err(SyncError::Auth(format!(
            "configured owner '{explicit}' does not match credential-derived owner '{derived}'"
        ))),
        (Some(explicit), None) => Ok(explicit.to_string()),
        (None, Some(derived)) => Ok(derived.to_string()),
        (None, None) => Err(SyncError::Auth("owner is required".to_string())),
    }
}

pub fn default_host() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn jsonl_adapters<'a>(
    registry: &'a Registry,
    allow: Option<&[ToolKind]>,
) -> Vec<&'a dyn Adapter> {
    registry
        .detected(allow)
        .into_iter()
        .filter(|adapter| is_jsonl_kind(adapter.kind()))
        .collect()
}

fn is_jsonl_kind(kind: ToolKind) -> bool {
    matches!(kind, ToolKind::ClaudeCode | ToolKind::Codex)
}

fn session_files(adapter: &dyn Adapter) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in adapter.watch_paths() {
        if !root.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(root).into_iter().flatten() {
            if entry.file_type().is_file() && adapter.is_session_file(entry.path()) {
                files.push(entry.path().to_path_buf());
            }
        }
    }
    files.sort();
    files
}

fn session_id_for_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

pub fn credential_owner_from_env() -> Option<String> {
    std::env::var("AIKIT_SYNC_CREDENTIAL_OWNER")
        .ok()
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::InMemorySink;
    use crate::state::InMemorySyncStateStore;
    use aikit_session_capture::{AdapterError, ParseResult};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeAdapter {
        kind: ToolKind,
        root: PathBuf,
    }

    #[async_trait]
    impl Adapter for FakeAdapter {
        fn kind(&self) -> ToolKind {
            self.kind
        }

        fn watch_paths(&self) -> Vec<PathBuf> {
            vec![self.root.clone()]
        }

        fn is_session_file(&self, path: &Path) -> bool {
            path.starts_with(&self.root) && path.extension().is_some_and(|e| e == "jsonl")
        }

        async fn parse_session_file(
            &self,
            _path: &Path,
            _from_offset: u64,
        ) -> Result<ParseResult, AdapterError> {
            panic!("session sync must not call parse_session_file")
        }
    }

    fn engine(sink: InMemorySink, state: InMemorySyncStateStore) -> SyncEngine {
        SyncEngine::new(
            SyncConfig {
                owner: Some("owner".into()),
                host: "host".into(),
                ..SyncConfig::default()
            },
            Arc::new(sink) as Arc<dyn SyncSink>,
            Arc::new(state) as Arc<dyn SyncStateStore>,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn idempotency_uses_skip_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("session.jsonl");
        tokio::fs::write(&file, "{\"msg\":\"hello\"}\n")
            .await
            .unwrap();
        let adapter = FakeAdapter {
            kind: ToolKind::Codex,
            root: tmp.path().to_path_buf(),
        };
        let sink = InMemorySink::new();
        let engine = engine(sink.clone(), InMemorySyncStateStore::default());

        assert!(matches!(
            engine.sync_file(&adapter, &file).await.unwrap(),
            SyncOutcome::Synced { .. }
        ));
        assert_eq!(sink.put_calls(), 1);
        assert_eq!(
            engine.sync_file(&adapter, &file).await.unwrap(),
            SyncOutcome::SkippedUnchanged
        );
        assert_eq!(sink.put_calls(), 1);
    }

    #[tokio::test]
    async fn grown_file_versions_are_retained_and_ordered_by_capture_time() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("grow.jsonl");
        tokio::fs::write(&file, "{\"msg\":\"one\"}\n")
            .await
            .unwrap();
        let adapter = FakeAdapter {
            kind: ToolKind::ClaudeCode,
            root: tmp.path().to_path_buf(),
        };
        let sink = InMemorySink::new();
        let engine = engine(sink.clone(), InMemorySyncStateStore::default());

        engine.sync_file(&adapter, &file).await.unwrap();
        tokio::time::sleep(Duration::from_millis(2)).await;
        tokio::fs::write(&file, "{\"msg\":\"one\"}\n{\"msg\":\"two\"}\n")
            .await
            .unwrap();
        engine.sync_file(&adapter, &file).await.unwrap();

        assert_eq!(sink.object_count(), 2);
        let keys = sink.keys();
        let first = sink.get_envelope(&crate::sink::meta_key(&keys[0])).unwrap();
        let second = sink.get_envelope(&crate::sink::meta_key(&keys[1])).unwrap();
        assert_ne!(first.content_hash, second.content_hash);
        assert!(first.captured_at_ms <= second.captured_at_ms);
    }

    #[tokio::test]
    async fn sidecar_failure_does_not_advance_state_and_retry_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("partial.jsonl");
        tokio::fs::write(&file, "{\"msg\":\"hello\"}\n")
            .await
            .unwrap();
        let adapter = FakeAdapter {
            kind: ToolKind::Codex,
            root: tmp.path().to_path_buf(),
        };
        let sink = InMemorySink::with_meta_failure_once();
        let state = InMemorySyncStateStore::default();
        let engine = engine(sink.clone(), state);

        assert!(engine.sync_file(&adapter, &file).await.is_err());
        assert_eq!(sink.object_count(), 1);
        assert_eq!(sink.meta_count(), 0);
        engine.sync_file(&adapter, &file).await.unwrap();
        assert_eq!(sink.object_count(), 1);
        assert_eq!(sink.meta_count(), 1);
        assert_eq!(sink.put_calls(), 2);
    }

    #[test]
    fn owner_precedence_is_fail_closed() {
        assert_eq!(resolve_owner(Some("a"), Some("a")).unwrap(), "a");
        assert!(matches!(
            resolve_owner(Some("a"), Some("b")),
            Err(SyncError::Auth(_))
        ));
        assert!(matches!(resolve_owner(None, None), Err(SyncError::Auth(_))));
    }

    #[tokio::test]
    async fn non_utf8_file_fails_but_other_files_continue() {
        let tmp = tempfile::tempdir().unwrap();
        let bad = tmp.path().join("bad.jsonl");
        let good = tmp.path().join("good.jsonl");
        tokio::fs::write(&bad, b"\xff\xfe").await.unwrap();
        tokio::fs::write(&good, b"{\"msg\":\"ok\"}\n")
            .await
            .unwrap();
        let adapter = FakeAdapter {
            kind: ToolKind::Codex,
            root: tmp.path().to_path_buf(),
        };
        let sink = InMemorySink::new();
        let engine = engine(sink.clone(), InMemorySyncStateStore::default());
        assert!(matches!(
            engine.sync_file(&adapter, &bad).await,
            Err(SyncError::Io(_))
        ));
        engine.sync_file(&adapter, &good).await.unwrap();
        assert_eq!(sink.put_calls(), 1);
    }

    #[tokio::test]
    async fn dry_run_never_calls_put() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("dry.jsonl");
        tokio::fs::write(&file, "{\"msg\":\"hello\"}\n")
            .await
            .unwrap();
        let adapter = FakeAdapter {
            kind: ToolKind::Codex,
            root: tmp.path().to_path_buf(),
        };
        let sink = InMemorySink::new();
        let engine = SyncEngine::new(
            SyncConfig {
                owner: Some("owner".into()),
                host: "host".into(),
                dry_run: true,
                ..SyncConfig::default()
            },
            Arc::new(sink.clone()) as Arc<dyn SyncSink>,
            Arc::new(InMemorySyncStateStore::default()) as Arc<dyn SyncStateStore>,
        )
        .unwrap();
        assert!(matches!(
            engine.sync_file(&adapter, &file).await.unwrap(),
            SyncOutcome::Synced { .. }
        ));
        assert_eq!(sink.put_calls(), 0);
    }

    struct FlakySink {
        fail_count: AtomicUsize,
        writes: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl SyncSink for FlakySink {
        async fn put(&self, object: SyncObject) -> Result<(), SyncError> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            let remaining = self.fail_count.load(Ordering::SeqCst);
            if object.key.contains("/flaky/") && remaining > 0 {
                self.fail_count.fetch_sub(1, Ordering::SeqCst);
                return Err(SyncError::Backend("temporary".into()));
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn watch_retry_uses_backoff_without_blocking_other_files() {
        let tmp = tempfile::tempdir().unwrap();
        let flaky_file = tmp.path().join("flaky.jsonl");
        let other_file = tmp.path().join("other.jsonl");
        tokio::fs::write(&flaky_file, "{\"msg\":\"flaky\"}\n")
            .await
            .unwrap();
        tokio::fs::write(&other_file, "{\"msg\":\"other\"}\n")
            .await
            .unwrap();
        let adapter = FakeAdapter {
            kind: ToolKind::Codex,
            root: tmp.path().to_path_buf(),
        };
        let sink = Arc::new(FlakySink {
            fail_count: AtomicUsize::new(1),
            writes: AtomicUsize::new(0),
        });
        let engine = SyncEngine::new(
            SyncConfig {
                owner: Some("owner".into()),
                host: "host".into(),
                ..SyncConfig::default()
            },
            sink.clone() as Arc<dyn SyncSink>,
            Arc::new(InMemorySyncStateStore::default()) as Arc<dyn SyncStateStore>,
        )
        .unwrap();

        engine.sync_file(&adapter, &other_file).await.unwrap();
        engine
            .retry_with_backoff(
                &adapter,
                &flaky_file,
                2,
                WatchRetryPolicy {
                    base: Duration::from_millis(1),
                    cap: Duration::from_millis(2),
                },
            )
            .await
            .unwrap();
        assert_eq!(sink.writes.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn resume_offline_keeps_confirmed_state_only() {
        let tmp = tempfile::tempdir().unwrap();
        let one = tmp.path().join("one.jsonl");
        let two = tmp.path().join("two.jsonl");
        tokio::fs::write(&one, "{\"msg\":\"one\"}\n").await.unwrap();
        tokio::fs::write(&two, "{\"msg\":\"two\"}\n").await.unwrap();
        let adapter = FakeAdapter {
            kind: ToolKind::Codex,
            root: tmp.path().to_path_buf(),
        };
        let state = InMemorySyncStateStore::default();
        let failing = engine(InMemorySink::with_backend_failure(), state.clone());
        assert!(failing.sync_file(&adapter, &one).await.is_err());

        let sink = InMemorySink::new();
        let resumed = engine(sink.clone(), state);
        resumed.sync_file(&adapter, &one).await.unwrap();
        resumed.sync_file(&adapter, &two).await.unwrap();
        resumed.sync_file(&adapter, &one).await.unwrap();
        assert_eq!(sink.object_count(), 2);
    }
}
