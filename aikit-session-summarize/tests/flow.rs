//! The whole flow on the capture crate's fixtures: locate → scan → select →
//! summarize through a mock gateway → stored brief → no-op on re-run, with the
//! session files left untouched.
#![cfg(all(feature = "claudecode", feature = "codex"))]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use aikit_agent::llm::mock::{MockGateway, MockResponse};
use aikit_session_capture::{
    scan_adapter, EventStore, InMemoryCursorStore, InMemoryEventStore, ToolKind,
};
use aikit_session_summarize::{
    adapters_for, select_sessions, BriefStore, InMemoryBriefStore, LocationSpec, ModelConfig,
    Outcome, Selection, SummarizeOptions, Summarizer, TagList, TagSource,
};

/// Every file under `root` with its bytes and modification time: what a
/// read-only run must leave exactly as it found it.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (Vec<u8>, SystemTime)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            let meta = entry.metadata().unwrap();
            if meta.is_dir() {
                stack.push(path);
            } else {
                let bytes = std::fs::read(&path).unwrap();
                out.insert(path, (bytes, meta.modified().unwrap()));
            }
        }
    }
    out
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("aikit-session-capture")
        .join("tests")
        .join("fixtures")
}

/// Copy the two fixtures into scratch roots laid out the way `--path
/// <tool>=<dir>` expects, and scan them into an in-memory store.
async fn scanned() -> (Arc<InMemoryEventStore>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let claude_root = dir.path().join("claude");
    let codex_root = dir.path().join("codex");
    std::fs::create_dir_all(claude_root.join("-tmp-proj")).unwrap();
    std::fs::create_dir_all(&codex_root).unwrap();
    std::fs::copy(
        fixtures().join("claudecode").join("simple-session.jsonl"),
        claude_root.join("-tmp-proj").join("sess-001.jsonl"),
    )
    .unwrap();
    std::fs::copy(
        fixtures().join("codex").join("rollout-session.jsonl"),
        codex_root.join("rollout-session.jsonl"),
    )
    .unwrap();

    let locations = vec![
        LocationSpec {
            tool: Some(ToolKind::ClaudeCode),
            path: claude_root,
        },
        LocationSpec {
            tool: Some(ToolKind::Codex),
            path: codex_root,
        },
    ];
    let store = Arc::new(InMemoryEventStore::new());
    let cursors = InMemoryCursorStore::default();
    for adapter in adapters_for(None, &locations) {
        let outcome = scan_adapter(adapter.as_ref(), store.as_ref(), &cursors, false).await;
        assert_eq!(outcome.files_scanned, 1, "{:?}", adapter.kind());
        assert!(outcome.events_upserted > 0);
    }
    (store, dir)
}

async fn all_sessions(store: &InMemoryEventStore) -> Vec<aikit_session_capture::SessionSummary> {
    let mut out = Vec::new();
    for tool in [ToolKind::ClaudeCode, ToolKind::Codex] {
        out.extend(store.sessions_for(tool, None, 100, 0).await.unwrap());
    }
    out
}

#[tokio::test]
async fn fixtures_scan_list_and_summarize_end_to_end() {
    let (store, dir) = scanned().await;
    let briefs = Arc::new(InMemoryBriefStore::new());
    // The session roots, as the scan left them.
    let before = snapshot(dir.path());

    let listed = all_sessions(&store).await;
    let mut ids: Vec<(ToolKind, &str)> = listed
        .iter()
        .map(|s| (s.tool, s.session_id.as_str()))
        .collect();
    ids.sort_by_key(|(t, id)| (t.as_str(), *id));
    assert_eq!(
        ids,
        vec![
            (ToolKind::ClaudeCode, "sess-001"),
            (ToolKind::Codex, "cx-001")
        ]
    );

    let selected = select_sessions(
        listed.clone(),
        &Selection {
            all: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(selected.len(), 2);

    // Dry run first: the digest is built from the events and nothing is
    // stored.
    let dry = Arc::new(Summarizer::new(
        Arc::new(MockGateway::new(vec![])),
        ModelConfig::new("mock", "http://mock", "key"),
        SummarizeOptions {
            dry_run: true,
            ..Default::default()
        },
    ));
    let outcomes = dry
        .summarize_many(store.clone(), briefs.clone(), selected.clone())
        .await;
    for o in &outcomes {
        match &o.outcome {
            Outcome::DryRun { user_message, .. } => {
                assert!(user_message.contains("## Files touched"));
                assert!(user_message.contains("main.go"), "{user_message}");
                assert!(user_message.contains("## Allowed tags"));
                if o.tool == ToolKind::Codex {
                    // Codex stores the prompt as an event.
                    assert!(user_message.contains("1. Show me main.go and run the tests"));
                }
            }
            other => panic!("expected DryRun for {}: {other:?}", o.session_id),
        }
    }
    assert!(briefs
        .brief_for(ToolKind::Codex, "cx-001")
        .await
        .unwrap()
        .is_none());

    // Real run through the mock: one reply per session, in batch order.
    let replies: Vec<MockResponse> = selected
        .iter()
        .map(|s| {
            MockResponse::text(format!(
                r#"{{"summary": "Looked at main.go in {}.", "tags": [{{"name": "research", "why": "only reads"}}, {{"name": "bogus", "why": "not allowed"}}]}}"#,
                s.session_id
            ))
        })
        .collect();
    // The rejected `bogus` triggers one corrective retry per session.
    let mut queue = Vec::new();
    for r in replies {
        queue.push(r.clone());
        queue.push(r);
    }
    let real = Arc::new(Summarizer::new(
        Arc::new(MockGateway::new(queue)),
        ModelConfig::new("mock", "http://mock", "key"),
        SummarizeOptions {
            parallel: 1,
            tags: TagList::from_names(["feature", "research"]).unwrap(),
            ..Default::default()
        },
    ));
    let outcomes = real
        .summarize_many(store.clone(), briefs.clone(), selected.clone())
        .await;
    assert_eq!(outcomes.len(), 2);
    for o in &outcomes {
        match &o.outcome {
            Outcome::Generated { brief } => {
                assert!(brief.summary.starts_with("Looked at main.go"));
                assert_eq!(brief.tags.len(), 1);
                assert_eq!(brief.tags[0].name, "research");
                assert_eq!(brief.tags[0].source, TagSource::Model);
                assert_eq!(brief.rejected_tags, vec!["bogus"]);
                assert!(!brief.areas.is_empty(), "areas from the fixture's reads");
                assert_eq!(brief.model, "mock");
                let stored = briefs.brief_for(o.tool, &o.session_id).await.unwrap();
                assert_eq!(stored.as_ref(), Some(brief));
            }
            other => panic!("expected Generated for {}: {other:?}", o.session_id),
        }
    }

    // Unchanged sessions are a no-op: the queue is empty and never touched.
    let again = real
        .summarize_many(store.clone(), briefs.clone(), selected.clone())
        .await;
    assert!(again
        .iter()
        .all(|o| matches!(o.outcome, Outcome::Unchanged { .. })));

    // `--force` asks again; an empty queue then fails, proving the call.
    let forced = Arc::new(Summarizer::new(
        Arc::new(MockGateway::new(vec![])),
        ModelConfig::new("mock", "http://mock", "key"),
        SummarizeOptions {
            force: true,
            tags: TagList::from_names(["feature", "research"]).unwrap(),
            ..Default::default()
        },
    ));
    let f = forced
        .summarize_one(store.as_ref(), briefs.as_ref(), &selected[0])
        .await;
    assert!(matches!(f.outcome, Outcome::Failed { .. }));
    // The stored brief survives a failed regeneration.
    assert!(briefs
        .brief_for(selected[0].tool, &selected[0].session_id)
        .await
        .unwrap()
        .is_some());

    // Read-only toward the tools: dry run, generation, re-run and a forced
    // regeneration left every session file byte-for-byte and mtime-for-mtime
    // as it was.
    assert_eq!(snapshot(dir.path()), before);

    // Briefs list newest first per tool.
    let codex = briefs.briefs_for(ToolKind::Codex, 10, 0).await.unwrap();
    assert_eq!(codex.len(), 1);
    assert_eq!(codex[0].session_id, "cx-001");
}
