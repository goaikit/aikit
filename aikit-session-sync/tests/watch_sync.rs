//! Watch → sync integration: prove that a session file *arriving* and later
//! *growing* on disk is picked up by a watch driver and synced to the blob
//! sink, scrubbed and content-addressed, exactly as the `aikit session sync
//! --watch` loop does.
//!
//! Uses `PollingWatchDriver` (not `NotifyWatchDriver`) on purpose: polling is
//! deterministic — it scans on an interval and reports new/mtime-changed files
//! — so this test is not subject to the timing/coalescing quirks of OS file
//! notifications. The notify driver itself is smoke-tested separately in
//! aikit-session-capture (`watch.rs`). The sync side here is the real thing:
//! the same `SyncEngine::retry_with_backoff` call the CLI watch loop makes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aikit_session_capture::watch::{find_adapter_for_path, PollingWatchDriver, WatchDriver};
use aikit_session_capture::{Adapter, AdapterError, ParseResult, ToolKind};
use aikit_session_sync::state::InMemorySyncStateStore;
use aikit_session_sync::{
    InMemorySink, SyncConfig, SyncEngine, SyncOutcome, SyncSink, SyncStateStore, WatchRetryPolicy,
};
use async_trait::async_trait;

/// A watch target rooted at a temp dir — no dependency on a real `~/.claude`
/// layout. `parse_session_file` must never be called by sync (asserted).
struct TempDirAdapter {
    kind: ToolKind,
    root: PathBuf,
}

#[async_trait]
impl Adapter for TempDirAdapter {
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

fn engine(sink: Arc<InMemorySink>) -> SyncEngine {
    SyncEngine::new(
        SyncConfig {
            owner: Some("alice".into()),
            host: "watch-host".into(),
            key_prefix: "sessions/".into(),
            ..SyncConfig::default()
        },
        sink as Arc<dyn SyncSink>,
        Arc::new(InMemorySyncStateStore::default()) as Arc<dyn SyncStateStore>,
    )
    .expect("engine")
}

/// Drive the polling watcher until it reports an event, then sync it the way
/// the CLI `--watch` loop does. Bounded so a bug can't hang the suite.
async fn watch_one_and_sync(
    driver: &mut PollingWatchDriver,
    adapter: &dyn Adapter,
    engine: &SyncEngine,
) -> SyncOutcome {
    let path = tokio::time::timeout(Duration::from_secs(5), driver.next_event())
        .await
        .expect("watcher reported no event within 5s")
        .expect("watcher stream ended");
    let adapters: [&dyn Adapter; 1] = [adapter];
    let matched = find_adapter_for_path(&adapters, &path).expect("adapter claims the path");
    engine
        .retry_with_backoff(matched, &path, 6, WatchRetryPolicy::default())
        .await
        .expect("watch sync failed")
}

#[tokio::test]
async fn new_and_grown_session_files_are_synced_via_watch() {
    let tmp = tempfile::tempdir().unwrap();
    let session_id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let file = tmp.path().join(format!("{session_id}.jsonl"));

    let sink = Arc::new(InMemorySink::new());
    let engine = engine(sink.clone());
    let adapter = TempDirAdapter {
        kind: ToolKind::ClaudeCode,
        root: tmp.path().to_path_buf(),
    };
    let mut driver = PollingWatchDriver::new(
        vec![Box::new(TempDirAdapter {
            kind: ToolKind::ClaudeCode,
            root: tmp.path().to_path_buf(),
        })],
        Duration::from_millis(25),
    );

    // 1. A new session file arrives with a live-looking secret.
    tokio::fs::write(
        &file,
        "{\"role\":\"user\",\"text\":\"key=AKIAIOSFODNN7EXAMPLE\"}\n",
    )
    .await
    .unwrap();

    let first = watch_one_and_sync(&mut driver, &adapter, &engine).await;
    let key1 = match first {
        SyncOutcome::Synced { key, .. } => key,
        other => panic!("expected Synced on new file, got {other:?}"),
    };

    // The secret was scrubbed on the watch path, not just in one-shot sync.
    let stored = sink.get_content(&key1).expect("content object present");
    let stored = String::from_utf8(stored.to_vec()).unwrap();
    assert!(
        !stored.contains("AKIAIOSFODNN7EXAMPLE"),
        "raw secret must not reach the sink"
    );
    assert!(stored.contains("[REDACTED:aws_access_key]"));
    assert!(key1.starts_with("sessions/alice/claude_code/"));
    let env = sink
        .get_envelope(&key1.replace(".jsonl", ".meta.json"))
        .expect("envelope present");
    assert_eq!(env.owner, "alice");
    assert_eq!(env.session_id, session_id);
    assert_eq!(sink.object_count(), 1);

    // 2. The same transcript grows (a new turn is appended). mtime advances,
    //    so the poller re-reports it; a second, distinct version is synced.
    tokio::time::sleep(Duration::from_millis(20)).await;
    tokio::fs::write(
        &file,
        "{\"role\":\"user\",\"text\":\"key=AKIAIOSFODNN7EXAMPLE\"}\n{\"role\":\"assistant\",\"text\":\"more\"}\n",
    )
    .await
    .unwrap();

    let key2 = match watch_one_and_sync(&mut driver, &adapter, &engine).await {
        SyncOutcome::Synced { key, .. } => key,
        other => panic!("expected Synced on grown file, got {other:?}"),
    };
    assert_ne!(
        key1, key2,
        "grown transcript must produce a new version key"
    );
    assert_eq!(sink.object_count(), 2, "both versions retained");
}
