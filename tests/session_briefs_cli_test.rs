//! CLI-level tests for `aikit session list`, `summarize` and `briefs`
//! (`execute_list` / `execute_summarize` / `execute_briefs`) against a
//! scratch `--path` and a scratch `--db`, so nothing touches the user's homes
//! or capture store.
//!
//! The model is a local mockito server speaking the OpenAI chat-completions
//! shape, so the real HTTP gateway, the preflight call and the exit codes are
//! exercised end to end. The engine's retry and reply handling are covered
//! in `aikit-session-summarize` with the mock gateway.
#![cfg(all(feature = "agent-adapters", feature = "codex"))]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use aikit::cli::session::{
    execute_briefs, execute_list, execute_summarize, BriefsArgs, ListSessionsArgs,
    SummarizeSessionsArgs,
};

/// Environment variable the tests put their fake key in. Unique to this
/// binary, so setting it cannot race another test's environment.
const KEY_ENV: &str = "AIKIT_SESSION_BRIEFS_TEST_KEY";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("aikit-session-capture")
        .join("tests")
        .join("fixtures")
        .join("codex")
        .join("rollout-session.jsonl")
}

/// A scratch dir holding a `codex` session root with the Codex fixture, the
/// `--path` value for it, and a scratch DB path beside it.
fn scratch() -> (tempfile::TempDir, PathBuf, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("codex");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(fixture(), root.join("rollout-session.jsonl")).unwrap();
    let db = dir.path().join("capture.db").display().to_string();
    let path = format!("codex={}", root.display());
    (dir, root, path, db)
}

/// Every file under `root` with its bytes and modification time.
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
                out.insert(
                    path.clone(),
                    (std::fs::read(&path).unwrap(), meta.modified().unwrap()),
                );
            }
        }
    }
    out
}

/// An OpenAI chat-completions response whose content is the brief JSON.
fn completion_body() -> String {
    let content = r#"{"summary": "Read main.go and ran the tests.", "tags": [{"name": "research", "why": "files were read and a command was run"}]}"#;
    serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "model": "mock-model",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 10, "total_tokens": 20}
    })
    .to_string()
}

fn summarize_args(path: &str, db: &str, base_url: &str) -> SummarizeSessionsArgs {
    SummarizeSessionsArgs {
        all: true,
        paths: vec![path.to_string()],
        db: Some(db.to_string()),
        model: Some("mock-model".into()),
        base_url: Some(base_url.to_string()),
        api_key_env: Some(KEY_ENV.into()),
        timeout: Some("30".into()),
        format: "json".into(),
        quiet: true,
        ..Default::default()
    }
}

fn stored_briefs(db: &str) -> Vec<(String, String)> {
    let conn = rusqlite::Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare("SELECT session_id, summary FROM capture_session_briefs ORDER BY session_id")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[tokio::test]
async fn list_scans_a_scratch_path_into_a_scratch_db() {
    let (_dir, _root, path, db) = scratch();
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
    let (_dir, _root, path, db) = scratch();
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generates_reads_back_and_never_touches_session_files() {
    std::env::set_var(KEY_ENV, "test-key");
    let (_dir, root, path, db) = scratch();
    let before = snapshot(&root);

    let mut server = mockito::Server::new_async().await;
    let completions = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(completion_body())
        // Preflight + one brief on the first run; preflight on each rerun.
        .expect_at_least(2)
        .create_async()
        .await;
    let url = server.url();

    // Generate: preflight passes, the brief is stored.
    assert_eq!(
        execute_summarize(summarize_args(&path, &db, &url))
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        stored_briefs(&db),
        vec![(
            "cx-001".to_string(),
            "Read main.go and ran the tests.".to_string()
        )]
    );

    // Unchanged on a rerun; `--no-mirror` from 0.1.196 scripts is accepted.
    let rerun = SummarizeSessionsArgs {
        no_mirror: true,
        quiet: false,
        format: "default".into(),
        ..summarize_args(&path, &db, &url)
    };
    assert_eq!(execute_summarize(rerun).await.unwrap(), 0);
    assert_eq!(stored_briefs(&db).len(), 1);

    // Read back without a model: all, by prefix, text and JSON.
    let briefs = |sessions: Vec<&str>, format: &str| BriefsArgs {
        sessions: sessions.into_iter().map(String::from).collect(),
        db: Some(db.clone()),
        format: format.into(),
        ..Default::default()
    };
    assert_eq!(execute_briefs(briefs(vec![], "json")).await.unwrap(), 0);
    assert_eq!(
        execute_briefs(briefs(vec!["cx-001"], "default"))
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        execute_briefs(briefs(vec!["no-such-session"], "json"))
            .await
            .unwrap(),
        2,
        "a --session that matches no stored brief"
    );
    assert_eq!(
        execute_briefs(briefs(vec!["cx"], "json")).await.unwrap(),
        2,
        "a prefix shorter than 8 characters"
    );
    assert_eq!(
        execute_briefs(BriefsArgs {
            db: Some("/nonexistent/capture.db".into()),
            format: "json".into(),
            ..Default::default()
        })
        .await
        .unwrap(),
        0,
        "no database yet is an empty list, not an error"
    );

    completions.assert_async().await;
    // Read-only toward the tool: the session root is exactly as it was.
    assert_eq!(snapshot(&root), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rejected_key_fails_once_at_preflight() {
    std::env::set_var(KEY_ENV, "test-key");
    let (_dir, _root, path, db) = scratch();

    let mut server = mockito::Server::new_async().await;
    let rejected = server
        .mock("POST", "/chat/completions")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": {"message": "invalid api key"}}"#)
        // One call for the preflight run, one for the session with
        // --no-preflight. 401 is never retried.
        .expect(2)
        .create_async()
        .await;
    let url = server.url();

    // Preflight catches it: exit 2 before any session is attempted.
    assert_eq!(
        execute_summarize(summarize_args(&path, &db, &url))
            .await
            .unwrap(),
        2
    );
    assert!(stored_briefs(&db).is_empty());

    // Without preflight the session itself fails: exit 1, one call, no retry.
    let skip = SummarizeSessionsArgs {
        no_preflight: true,
        ..summarize_args(&path, &db, &url)
    };
    assert_eq!(execute_summarize(skip).await.unwrap(), 1);
    rejected.assert_async().await;
}
