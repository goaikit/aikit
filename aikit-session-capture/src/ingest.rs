//! The shared scan pipeline: parse one session file from its stored cursor,
//! upsert the events, save the new cursor.
//!
//! Every host path that ingests session files runs through
//! [`parse_and_store_file`]: `aikit serve`'s `POST /capture/scan`, its watch
//! driver, and `aikit session list` / `aikit session summarize`. One code
//! path is what makes their idempotency identical (spec 010 §14.3).

use std::path::{Path, PathBuf};

use crate::adapter::{Adapter, ParseWarning};
use crate::cursor_offset::{CursorStore, ParseCursor};
use crate::event_store::{EventBatch, EventStore};

/// Outcome of parsing one file, or the aggregate over a scan.
#[derive(Debug, Default, Clone)]
pub struct IngestOutcome {
    pub files_scanned: u64,
    pub files_skipped: u64,
    pub events_upserted: u64,
    pub deduplicated_count: u64,
    pub warnings: Vec<ParseWarning>,
}

impl IngestOutcome {
    /// Fold another outcome into this one.
    pub fn absorb(&mut self, other: IngestOutcome) {
        self.files_scanned += other.files_scanned;
        self.files_skipped += other.files_skipped;
        self.events_upserted += other.events_upserted;
        self.deduplicated_count += other.deduplicated_count;
        self.warnings.extend(other.warnings);
    }
}

/// Load the cursor for `path`, call the adapter, upsert the events, save the
/// new cursor. `force` re-reads from offset zero; the store's idempotent
/// upsert makes that safe.
///
/// The error is a plain string: every caller surfaces it as a warning on the
/// file and moves on to the next one.
pub async fn parse_and_store_file(
    adapter: &dyn Adapter,
    path: &Path,
    event_store: &dyn EventStore,
    cursor_store: &dyn CursorStore,
    force: bool,
) -> Result<IngestOutcome, String> {
    let cursor = cursor_store.load(path).await;
    let from_offset = if force {
        0
    } else {
        let stored = cursor.as_ref().map(|c| c.offset).unwrap_or(0);
        // Fast path for byte-offset adapters: skip if the file hasn't grown
        // past the stored cursor. SQLite-watermark adapters (OpenCode) always
        // proceed — their offset isn't a byte position.
        if stored > 0 {
            if let Ok(meta) = std::fs::metadata(path) {
                if meta.len() <= stored {
                    return Ok(IngestOutcome {
                        files_skipped: 1,
                        ..Default::default()
                    });
                }
            }
        }
        stored
    };

    let result = adapter
        .parse_session_file(path, from_offset)
        .await
        .map_err(|e| e.to_string())?;

    let total = (result.tool_events.len()
        + result.token_events.len()
        + result.cache_observations.len()) as u64;
    let batch = EventBatch {
        tool_events: result.tool_events,
        token_events: result.token_events,
        cache_observations: result.cache_observations,
    };
    let inserted = event_store
        .upsert_events(batch)
        .await
        .map_err(|e| e.to_string())?;
    let deduped = total.saturating_sub(inserted);

    cursor_store
        .save(ParseCursor {
            source_file: path.to_path_buf(),
            offset: result.new_offset,
            adapter_kind: adapter.kind(),
            updated_at: chrono::Utc::now(),
        })
        .await;

    Ok(IngestOutcome {
        files_scanned: 1,
        files_skipped: 0,
        events_upserted: inserted,
        deduplicated_count: deduped,
        warnings: result.warnings,
    })
}

/// Every file under the adapter's watch paths that it claims, sorted.
/// Missing roots are skipped; a root that is itself a file is offered to the
/// adapter directly.
pub fn session_files(adapter: &dyn Adapter) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in adapter.watch_paths() {
        if root.is_file() {
            if adapter.is_session_file(&root) {
                files.push(root);
            }
            continue;
        }
        if !root.is_dir() {
            continue;
        }
        walk_files(&root, &mut |p| {
            if adapter.is_session_file(p) {
                files.push(p.to_path_buf());
            }
        });
    }
    files.sort();
    files.dedup();
    files
}

/// Recursive file walk with `std::fs` so this module does not depend on the
/// `watcher` feature's `walkdir`. Symlinked directories are not followed.
fn walk_files(dir: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            walk_files(&path, visit);
        } else if meta.is_file() {
            visit(&path);
        }
    }
}

/// Scan every session file the adapter claims into the stores. A file that
/// fails to parse becomes an `Other` warning and counts as skipped; the scan
/// continues.
pub async fn scan_adapter(
    adapter: &dyn Adapter,
    event_store: &dyn EventStore,
    cursor_store: &dyn CursorStore,
    force: bool,
) -> IngestOutcome {
    let mut outcome = IngestOutcome::default();
    for path in session_files(adapter) {
        match parse_and_store_file(adapter, &path, event_store, cursor_store, force).await {
            Ok(o) => outcome.absorb(o),
            Err(e) => {
                outcome.warnings.push(ParseWarning::Other {
                    message: format!("{}: {e}", path.display()),
                });
                outcome.files_skipped += 1;
            }
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{AdapterError, ParseResult};
    use crate::cursor_offset::InMemoryCursorStore;
    use crate::event_store::InMemoryEventStore;
    use crate::models::{ActionKind, ActionStatus, ToolEvent, ToolKind};
    use async_trait::async_trait;

    /// Claims every `.txt` under its root and emits one event per line.
    struct LineAdapter {
        root: PathBuf,
    }

    #[async_trait]
    impl Adapter for LineAdapter {
        fn kind(&self) -> ToolKind {
            ToolKind::Codex
        }
        fn watch_paths(&self) -> Vec<PathBuf> {
            vec![self.root.clone()]
        }
        fn is_session_file(&self, path: &Path) -> bool {
            path.extension().is_some_and(|e| e == "txt")
        }
        async fn parse_session_file(
            &self,
            path: &Path,
            from_offset: u64,
        ) -> Result<ParseResult, AdapterError> {
            let bytes = std::fs::read(path).unwrap();
            let text = String::from_utf8_lossy(&bytes[from_offset as usize..]).into_owned();
            let session = path.file_stem().unwrap().to_string_lossy().into_owned();
            let events = text
                .lines()
                .map(|line| ToolEvent {
                    source_event_id: format!("{session}:{line}"),
                    source_file: path.to_path_buf(),
                    session_id: session.clone(),
                    tool: ToolKind::Codex,
                    kind: ActionKind::Read,
                    target: Some(line.to_string()),
                    input: None,
                    output: None,
                    status: ActionStatus::Success,
                    error_message: None,
                    started_at_ms: Some(1),
                    duration_ms: None,
                    git_root: None,
                    metadata: serde_json::Value::Null,
                })
                .collect();
            Ok(ParseResult {
                tool_events: events,
                new_offset: bytes.len() as u64,
                ..Default::default()
            })
        }
    }

    #[tokio::test]
    async fn scan_is_cursor_resumed_and_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("s1.txt");
        std::fs::write(&file, "one\ntwo\n").unwrap();
        std::fs::write(dir.path().join("ignored.jsonl"), "x").unwrap();

        let adapter = LineAdapter {
            root: dir.path().to_path_buf(),
        };
        let events = InMemoryEventStore::new();
        let cursors = InMemoryCursorStore::default();

        let first = scan_adapter(&adapter, &events, &cursors, false).await;
        assert_eq!(first.files_scanned, 1);
        assert_eq!(first.events_upserted, 2);

        // Unchanged file: the cursor fast path skips it.
        let second = scan_adapter(&adapter, &events, &cursors, false).await;
        assert_eq!(second.files_skipped, 1);
        assert_eq!(second.events_upserted, 0);

        // Grown file: only the new line is parsed and inserted.
        std::fs::write(&file, "one\ntwo\nthree\n").unwrap();
        let third = scan_adapter(&adapter, &events, &cursors, false).await;
        assert_eq!(third.events_upserted, 1);

        // Forced re-walk from zero dedupes to zero insertions.
        let forced = scan_adapter(&adapter, &events, &cursors, true).await;
        assert_eq!(forced.events_upserted, 0);
        assert_eq!(forced.deduplicated_count, 3);
    }

    #[test]
    fn session_files_accepts_a_file_root() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("only.txt");
        std::fs::write(&file, "").unwrap();
        let adapter = LineAdapter { root: file.clone() };
        assert_eq!(session_files(&adapter), vec![file]);
    }
}
