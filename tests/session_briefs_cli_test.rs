//! CLI-level tests for `aikit session list` and `aikit session summarize`
//! (`execute_list` / `execute_summarize`) against a scratch `--path` and a
//! scratch `--db`, so nothing touches the user's homes or capture store.
//!
//! The model-calling path is covered in `aikit-session-summarize` with the
//! mock gateway; here the glue is exercised: location parsing, the scan into
//! SQLite, selection, `--dry-run`, `--format json`, and exit codes.
#![cfg(all(feature = "agent-adapters", feature = "codex"))]

use std::path::{Path, PathBuf};

use aikit::cli::session::{
    execute_list, execute_summarize, ListSessionsArgs, SummarizeSessionsArgs,
};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("aikit-session-capture")
        .join("tests")
        .join("fixtures")
        .join("codex")
        .join("rollout-session.jsonl")
}

/// A scratch root holding the Codex fixture, and a scratch DB path.
fn scratch() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("codex");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(fixture(), root.join("rollout-session.jsonl")).unwrap();
    let db = dir.path().join("capture.db").display().to_string();
    (dir, format!("codex={}", root.display()), db)
}

#[tokio::test]
async fn list_scans_a_scratch_path_into_a_scratch_db() {
    let (_dir, path, db) = scratch();
    let code = execute_list(ListSessionsArgs {
        paths: vec![path.clone()],
        db: Some(db.clone()),
        format: "json".into(),
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(code, 0);

    // A tool filter that leaves no adapter for the given root is a
    // configuration error (2), as is a bad --format.
    let code = execute_list(ListSessionsArgs {
        paths: vec![path.clone()],
        tools: vec!["claude_code".into()],
        db: Some(db.clone()),
        format: "default".into(),
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(code, 2);
    // Listing the scratch DB again without --path shows the stored session.
    let code = execute_list(ListSessionsArgs {
        tools: vec!["codex".into()],
        db: Some(db.clone()),
        format: "default".into(),
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(code, 0);
    let code = execute_list(ListSessionsArgs {
        db: Some(db.clone()),
        format: "yaml".into(),
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(code, 2);
    let code = execute_list(ListSessionsArgs {
        paths: vec!["nope=/tmp".into()],
        db: Some(db),
        format: "default".into(),
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(code, 2, "unknown tool prefix in --path");
}

#[tokio::test]
async fn summarize_dry_run_needs_no_model_and_validates_selection() {
    let (_dir, path, db) = scratch();
    let base = || SummarizeSessionsArgs {
        paths: vec![path.clone()],
        db: Some(db.clone()),
        dry_run: true,
        format: "json".into(),
        ..Default::default()
    };

    // No selection → 2.
    assert_eq!(execute_summarize(base()).await.unwrap(), 2);

    // --all with --dry-run: no model, no key, exit 0.
    let code = execute_summarize(SummarizeSessionsArgs {
        all: true,
        ..base()
    })
    .await
    .unwrap();
    assert_eq!(code, 0);

    // A unique id prefix selects the fixture session; an unknown one exits 2.
    let code = execute_summarize(SummarizeSessionsArgs {
        sessions: vec!["cx-001".into()],
        ..base()
    })
    .await
    .unwrap();
    assert_eq!(code, 0);
    let code = execute_summarize(SummarizeSessionsArgs {
        sessions: vec!["zz-404".into()],
        ..base()
    })
    .await
    .unwrap();
    assert_eq!(code, 2);

    // Tag flags: both at once, or an unreadable file, exit 2 before any scan.
    let code = execute_summarize(SummarizeSessionsArgs {
        all: true,
        tags: Some("a,b".into()),
        tags_file: Some("/nonexistent/tags.toml".into()),
        ..base()
    })
    .await
    .unwrap();
    assert_eq!(code, 2);
    let code = execute_summarize(SummarizeSessionsArgs {
        all: true,
        tags_file: Some("/nonexistent/tags.toml".into()),
        ..base()
    })
    .await
    .unwrap();
    assert_eq!(code, 2);

    // Without --dry-run a model is required.
    let code = execute_summarize(SummarizeSessionsArgs {
        all: true,
        dry_run: false,
        model: None,
        ..base()
    })
    .await
    .unwrap();
    assert!(
        code == 2 || std::env::var("AIKIT_MODEL").is_ok(),
        "no --model and no AIKIT_MODEL must exit 2"
    );
}
