//! D2 / ADR 0012 ("tool availability is a per-agent capability, not a serve
//! flag"): `POST /api/v1/messages` can carry an opt-in tool policy (`tools`
//! and/or `disallowed_tools`) that gets threaded into the `RunOptions` the
//! run is built from via the *existing* `session_persona` mechanism (the
//! same one `--session-persona` uses), which `aikit_agent_adapter` deserializes
//! into `AgentPersona` and `aikit-agent`'s `build_tools` hard-filters against.
//!
//! These tests don't re-verify the filter itself (`loop_runner.rs`, out of
//! scope for this change) — they verify serve's plumbing: that a request
//! WITH a tool policy produces a `RunOptions` carrying it, and a request
//! WITHOUT one leaves `RunOptions` unchanged (default: full toolset, no
//! `session_persona`).
//!
//! Agent execution is fully stubbed via `make_capturing_stub_run_fn`, which
//! records the `RunOptions` each invocation was actually built with — no LLM
//! credentials required.

use std::time::Duration;

use aikit::cli::serve::{execute_with_run_fn, make_capturing_stub_run_fn, ServeArgs};

fn make_args(port: u16) -> ServeArgs {
    ServeArgs {
        host: "127.0.0.1".to_string(),
        port,
        run_timeout_secs: 30,
        max_sessions: 10,
        api_key: None,
        insecure: false,
    }
}

#[tokio::test]
async fn tool_policy_present_is_threaded_into_run_options() {
    let (run_fn, captured) = make_capturing_stub_run_fn();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let args = make_args(port);
    tokio::spawn(async move {
        execute_with_run_fn(args, run_fn).await.ok();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .json(&serde_json::json!({
            "agent": "aikit",
            "content": "hi",
            "tools": ["read_file", "write_file"],
            "disallowed_tools": ["run_bash"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let _ = resp.text().await.unwrap();

    let runs = captured.lock().unwrap();
    assert_eq!(runs.len(), 1, "expected exactly one run_fn invocation");
    let persona = runs[0]
        .session_persona
        .as_ref()
        .expect("a request WITH a tool policy must produce a RunOptions carrying session_persona");

    assert_eq!(
        persona["tools"],
        serde_json::json!(["read_file", "write_file"]),
        "tools allowlist must be threaded through unchanged; got {persona}"
    );
    assert_eq!(
        persona["disallowed_tools"],
        serde_json::json!(["run_bash"]),
        "disallowed_tools denylist must be threaded through unchanged; got {persona}"
    );

    // Sanity: the resulting JSON is a valid `AgentPersona` — deserializable
    // by the exact type `aikit_agent_adapter::apply_session_options` uses,
    // not merely serve-shaped JSON that happens to look right.
    #[derive(serde::Deserialize)]
    struct CheckPersona {
        #[allow(dead_code)]
        name: String,
        #[allow(dead_code)]
        description: String,
        #[allow(dead_code)]
        prompt: String,
        tools: Option<Vec<String>>,
        disallowed_tools: Option<Vec<String>>,
    }
    let deser: CheckPersona = serde_json::from_value(persona.clone())
        .expect("session_persona must deserialize as a valid AgentPersona");
    assert_eq!(
        deser.tools,
        Some(vec!["read_file".to_string(), "write_file".to_string()])
    );
    assert_eq!(deser.disallowed_tools, Some(vec!["run_bash".to_string()]));
}

#[tokio::test]
async fn tool_policy_absent_leaves_default_full_toolset() {
    let (run_fn, captured) = make_capturing_stub_run_fn();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let args = make_args(port);
    tokio::spawn(async move {
        execute_with_run_fn(args, run_fn).await.ok();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .json(&serde_json::json!({
            "agent": "aikit",
            "content": "hi",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let _ = resp.text().await.unwrap();

    let runs = captured.lock().unwrap();
    assert_eq!(runs.len(), 1, "expected exactly one run_fn invocation");
    assert!(
        runs[0].session_persona.is_none(),
        "a request WITHOUT a tool policy must leave RunOptions.session_persona \
         unset (default: full toolset, per ADR 0012); got {:?}",
        runs[0].session_persona
    );
}

#[tokio::test]
async fn tools_only_no_disallowed_tools_still_threads_through() {
    // Confirms the two fields are independent: setting only `tools` (no
    // `disallowed_tools`) still produces a session_persona, with
    // `disallowed_tools` absent/null inside it.
    let (run_fn, captured) = make_capturing_stub_run_fn();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let args = make_args(port);
    tokio::spawn(async move {
        execute_with_run_fn(args, run_fn).await.ok();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .json(&serde_json::json!({
            "agent": "aikit",
            "content": "hi",
            "tools": ["read_file"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let _ = resp.text().await.unwrap();

    let runs = captured.lock().unwrap();
    let persona = runs[0]
        .session_persona
        .as_ref()
        .expect("tools-only request must still produce session_persona");
    assert_eq!(persona["tools"], serde_json::json!(["read_file"]));
    assert!(persona["disallowed_tools"].is_null());
}
