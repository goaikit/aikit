//! Watch driver: the trait seam that lets `POST /capture/scan` (manual) and
//! an automated watcher share the same parse pipeline (spec 010 §14.5).
//!
//! Behind the `watcher` feature so hosts that want only manual scans skip
//! the `notify` dep entirely. `aikit serve` enables `watcher` in its feature
//! set.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use notify::Watcher;
use tokio::sync::mpsc;

use crate::adapter::Adapter;

/// The watch driver trait. Returns one changed path at a time, or `None` on
/// shutdown (when the internal channel closes). Both impls call into the
/// same `parse_session_file → upsert` pipeline that `POST /capture/scan`
/// uses — they differ only in *when* to parse, not *how*.
#[async_trait]
pub trait WatchDriver: Send + Sync {
    /// Block until the next filesystem event in any adapter's watch paths,
    /// or return `None` on shutdown.
    async fn next_event(&mut self) -> Option<PathBuf>;
}

/// `notify`-backed default impl. Wraps `notify::RecommendedWatcher` over a
/// recursive watch of every adapter's `watch_paths()`. Events are debounced
/// 250ms (configurable) to coalesce editor save-storms.
pub struct NotifyWatchDriver {
    rx: mpsc::Receiver<PathBuf>,
    _watcher: notify::RecommendedWatcher,
}

impl NotifyWatchDriver {
    /// Create a watcher over the given adapters' watch paths.
    /// `debounce` controls how long to wait after the last event before
    /// surfacing it — coalesces rapid multi-event saves into one parse.
    pub fn new(adapters: Vec<&dyn Adapter>, debounce: Duration) -> Result<Self, notify::Error> {
        let (tx, rx) = mpsc::channel::<PathBuf>(256);

        // Debounce state: last-seen path + the instant we last saw any event
        // for it. When the debounce window elapses, the path is surfaced.
        let pending: Arc<Mutex<HashMap<PathBuf, Instant>>> = Arc::new(Mutex::new(HashMap::new()));

        let tx_for_cb = tx.clone();
        let pending_for_cb = Arc::clone(&pending);
        let debounce_cb = debounce;
        let runtime = tokio::runtime::Handle::current();

        let mut watcher =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    for path in &event.paths {
                        let mut map = pending_for_cb.lock().unwrap();
                        map.insert(path.clone(), Instant::now());
                    }
                    // Spawn a timer that checks if the debounce window has elapsed.
                    let tx2 = tx_for_cb.clone();
                    let p2 = Arc::clone(&pending_for_cb);
                    let db = debounce_cb;
                    runtime.spawn(async move {
                        tokio::time::sleep(db).await;
                        let mut map = p2.lock().unwrap();
                        let now = Instant::now();
                        // Surface any path whose debounce window has elapsed.
                        let ready: Vec<PathBuf> = map
                            .iter()
                            .filter(|(_, t)| now.duration_since(**t) >= db)
                            .map(|(p, _)| p.clone())
                            .collect();
                        for p in ready {
                            map.remove(&p);
                            let _ = tx2.try_send(p);
                        }
                    });
                }
            })?;

        for adapter in &adapters {
            for watch_path in adapter.watch_paths() {
                if watch_path.is_dir() {
                    watcher.watch(&watch_path, notify::RecursiveMode::Recursive)?;
                }
            }
        }

        Ok(Self {
            rx,
            _watcher: watcher,
        })
    }
}

#[async_trait]
impl WatchDriver for NotifyWatchDriver {
    async fn next_event(&mut self) -> Option<PathBuf> {
        self.rx.recv().await
    }
}

/// Polling fallback for hosts that don't want `notify` (or platforms where
/// `notify`'s native backend is unreliable). Polls mtimes on an interval.
pub struct PollingWatchDriver {
    adapters: Vec<Box<dyn Adapter>>,
    poll_interval: Duration,
    state: Mutex<HashMap<PathBuf, std::time::SystemTime>>,
}

impl PollingWatchDriver {
    pub fn new(adapters: Vec<Box<dyn Adapter>>, poll_interval: Duration) -> Self {
        Self {
            adapters,
            poll_interval,
            state: Mutex::new(HashMap::new()),
        }
    }

    /// Scan all watch paths once and return the first changed file found.
    fn scan_once(&self) -> Option<PathBuf> {
        let mut state = self.state.lock().unwrap();
        for adapter in &self.adapters {
            for watch_path in adapter.watch_paths() {
                if !watch_path.is_dir() {
                    continue;
                }
                for entry in walkdir::WalkDir::new(&watch_path).into_iter().flatten() {
                    if !entry.file_type().is_file() {
                        continue;
                    }
                    let path = entry.path();
                    if !adapter.is_session_file(path) {
                        continue;
                    }
                    let mtime = std::fs::metadata(path)
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::UNIX_EPOCH);
                    let prev = state.insert(path.to_path_buf(), mtime);
                    // If we've never seen this file, or its mtime changed.
                    if prev.is_none() || prev != Some(mtime) {
                        return Some(path.to_path_buf());
                    }
                }
            }
        }
        None
    }
}

#[async_trait]
impl WatchDriver for PollingWatchDriver {
    async fn next_event(&mut self) -> Option<PathBuf> {
        loop {
            if let Some(path) = self.scan_once() {
                return Some(path);
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }
}

/// Filter a changed path through the adapters' `is_session_file`. Returns
/// the matching adapter if any adapter claims the path.
pub fn find_adapter_for_path<'a>(
    adapters: &'a [&dyn Adapter],
    path: &Path,
) -> Option<&'a dyn Adapter> {
    adapters.iter().find(|a| a.is_session_file(path)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct FakeAdapter {
        kind: crate::ToolKind,
        paths: Vec<PathBuf>,
    }

    #[async_trait]
    impl Adapter for FakeAdapter {
        fn kind(&self) -> crate::ToolKind {
            self.kind
        }
        fn watch_paths(&self) -> Vec<PathBuf> {
            self.paths.clone()
        }
        fn is_session_file(&self, path: &Path) -> bool {
            path.extension().is_some_and(|e| e == "jsonl")
        }
        async fn parse_session_file(
            &self,
            _path: &Path,
            _from_offset: u64,
        ) -> Result<crate::ParseResult, crate::AdapterError> {
            Ok(crate::ParseResult::default())
        }
    }

    #[test]
    fn find_adapter_matches_jsonl() {
        let adapter = FakeAdapter {
            kind: crate::ToolKind::ClaudeCode,
            paths: vec![],
        };
        let adapters: Vec<&dyn Adapter> = vec![&adapter];
        assert!(find_adapter_for_path(&adapters, Path::new("/tmp/sess.jsonl")).is_some());
        assert!(find_adapter_for_path(&adapters, Path::new("/tmp/sess.txt")).is_none());
    }

    #[tokio::test]
    async fn polling_driver_detects_new_file() {
        let tmp = tempfile::tempdir().unwrap();
        let watch_path = tmp.path().to_path_buf();

        // Pre-create the file so the first scan picks it up as "new".
        let sess_file = watch_path.join("sess.jsonl");
        std::fs::write(&sess_file, "{\"type\":\"user\"}\n").unwrap();

        let adapter = FakeAdapter {
            kind: crate::ToolKind::ClaudeCode,
            paths: vec![watch_path.clone()],
        };

        let mut driver =
            PollingWatchDriver::new(vec![Box::new(adapter)], Duration::from_millis(50));

        let handle = tokio::time::timeout(Duration::from_secs(2), driver.next_event()).await;
        assert!(handle.is_ok(), "polling driver should detect the new file");
        let path = handle.unwrap().unwrap();
        assert!(path.extension().unwrap() == "jsonl");
    }
}
