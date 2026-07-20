//! End-to-end test of the new implicit-session API:
//!   1. POST /api/v1/messages with no session_id → server emits
//!      `event: session_started` then streams content and `event: done`.
//!   2. POST /api/v1/messages with the returned session_id → resumes the same
//!      conversation (session event echoes the supplied id, no new id minted).
//!   3. POST /api/v1/messages omitting session_id again → server mints a fresh,
//!      different id (a wholly new conversation).
//!
//! Agent execution is fully stubbed — no LLM credentials required.
//!
//! ARCH-4 / ADR 0016: serve emits the canonical `aikit_sdk::AgentEventPayload`
//! directly over SSE — the `event:` name is the payload's own snake_case
//! variant tag (`aikit_text_delta`, `stream_message`, `token_usage_line`,
//! `session_started`, …), not a serve-private frame shape. These tests build
//! stub items with the canonical `ServeEvent`/`AgentEvent` types and assert
//! on the canonical tag names.

use std::time::Duration;

use aikit::cli::serve::{
    execute_with_run_fn, make_failing_stub_run_fn, make_stub_run_fn_with_session, RunFn, ServeArgs,
    ServeEvent,
};
use aikit_sdk::{
    AgentEvent, AgentEventPayload, AgentEventStream, MessageKind, MessagePhase, MessageRole,
    StreamMessage, TokenUsage, UsageSource,
};

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

async fn start_server_with(run_fn: RunFn) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let args = make_args(port);

    tokio::spawn(async move {
        execute_with_run_fn(args, run_fn).await.ok();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    port
}

/// Build a canonical `AikitTextDelta` item — the shape the built-in `aikit`
/// backend uses for assistant text (every test in this file targets
/// `agent: "aikit"`).
fn text_event(content: &str) -> ServeEvent {
    ServeEvent::Agent(AgentEvent {
        agent_key: "aikit".to_string(),
        seq: 0,
        stream: AgentEventStream::Stdout,
        payload: AgentEventPayload::AikitTextDelta {
            content: content.to_string(),
            turn_id: None,
        },
    })
}

/// Build a canonical reasoning `StreamMessage` item. Reasoning has no
/// dedicated top-level event tag — it's a `StreamMessage` whose `kind` is
/// `Reasoning`, matching the SDK's actual vocabulary.
fn reasoning_event(content: &str) -> ServeEvent {
    ServeEvent::Agent(AgentEvent {
        agent_key: "aikit".to_string(),
        seq: 0,
        stream: AgentEventStream::Stdout,
        payload: AgentEventPayload::StreamMessage(StreamMessage {
            text: content.to_string(),
            phase: MessagePhase::Delta,
            role: MessageRole::Assistant,
            kind: MessageKind::Reasoning,
            source: AgentEventStream::Stdout,
            raw_line_seq: 0,
            turn_id: None,
        }),
    })
}

/// Build a canonical `TokenUsageLine` item.
fn token_usage_event(
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: Option<u64>,
) -> ServeEvent {
    ServeEvent::Agent(AgentEvent {
        agent_key: "aikit".to_string(),
        seq: 0,
        stream: AgentEventStream::Stdout,
        payload: AgentEventPayload::TokenUsageLine {
            usage: TokenUsage {
                input_tokens,
                output_tokens,
                total_tokens: None,
                cache_read_tokens,
                cache_creation_tokens: None,
                reasoning_tokens: None,
            },
            source: UsageSource::Aikit,
            raw_agent_line_seq: 0,
        },
    })
}

/// Extract the `session_id` carried by the first `event: session_started`
/// item in an SSE response body. Returns None if no such item is present.
fn parse_session_id(sse_body: &str) -> Option<String> {
    let mut is_session = false;
    for line in sse_body.lines() {
        if line.trim() == "event: session_started" {
            is_session = true;
            continue;
        }
        if is_session {
            if let Some(json) = line.strip_prefix("data: ") {
                let v: serde_json::Value = serde_json::from_str(json).ok()?;
                return v["session_id"].as_str().map(|s| s.to_string());
            }
        }
        if line.trim().is_empty() {
            is_session = false;
        }
    }
    None
}

#[tokio::test]
async fn test_health_endpoint() {
    let port = start_server_with(make_stub_run_fn_with_session(vec![], None)).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://127.0.0.1:{}/healthz", port))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert!(
        body["version"].as_str().is_some(),
        "version field must be present"
    );
}

#[tokio::test]
async fn test_readyz_endpoint() {
    let port = start_server_with(make_stub_run_fn_with_session(vec![], None)).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://127.0.0.1:{}/readyz", port))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ready");
}

#[tokio::test]
async fn test_old_health_endpoint_not_served() {
    let port = start_server_with(make_stub_run_fn_with_session(vec![], None)).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://127.0.0.1:{}/health", port))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        404,
        "/health must not be served (use /healthz)"
    );
}

#[tokio::test]
async fn test_agents_endpoint_returns_runnable_only() {
    let port = start_server_with(make_stub_run_fn_with_session(vec![], None)).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://127.0.0.1:{}/api/v1/agents", port))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let agents = body["agents"].as_array().expect("agents must be an array");

    // Dev tools (git/vscode) must never appear here.
    for a in agents {
        let key = a["key"].as_str().unwrap();
        assert_ne!(key, "git", "git is not an agent");
        assert_ne!(key, "code", "vscode is not an agent");
        assert!(
            a["available"].as_bool().unwrap_or(false),
            "only available agents should be listed; got {}",
            a
        );
        // E2: every agent carries an `auth` field, one of the three valid
        // values. In the test env it'll be `ok`/`unknown`/`unauthenticated`
        // depending on local credentials — assert presence + validity, not a
        // specific value.
        let auth = a["auth"]
            .as_str()
            .unwrap_or_else(|| panic!("agent must carry an auth field; got {}", a));
        assert!(
            matches!(auth, "ok" | "unauthenticated" | "unknown"),
            "auth must be one of ok/unauthenticated/unknown; got {} in {}",
            auth,
            a
        );
    }

    // `aikit` is always runnable (no external binary required) so it must
    // appear in any environment.
    assert!(
        agents.iter().any(|a| a["key"] == "aikit"),
        "aikit must be in the runnable list; got {:?}",
        agents
    );
}

#[tokio::test]
async fn test_old_v1_prefix_not_served() {
    let port = start_server_with(make_stub_run_fn_with_session(vec![], None)).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://127.0.0.1:{}/v1/agents", port))
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        404,
        "/v1/* must not be served (use /api/v1/*)"
    );
}

#[tokio::test]
async fn test_api_root_redirect() {
    let port = start_server_with(make_stub_run_fn_with_session(vec![], None)).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .get(format!("http://127.0.0.1:{}/api/", port))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 308, "GET /api/ must 308-redirect to /api/v1");
}

#[tokio::test]
async fn test_new_then_resume_then_new_flow() {
    // The stub back-end mints a fixed "stub-session-1" backend token, but the
    // *server* now mints its own stable UUID as the map key. Clients must use
    // the server-emitted session_id, not the backend token.
    let run_fn = make_stub_run_fn_with_session(
        vec![text_event("hello")],
        Some("stub-session-1".to_string()),
    );
    let port = start_server_with(run_fn).await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    // ── 1. First turn: no session_id → server mints + returns a UUID ──
    let resp = client
        .post(format!("{}/api/v1/messages", base))
        .json(&serde_json::json!({"agent": "aikit", "content": "hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(
        resp.headers()
            .get("content-type")
            .map(|v| v.to_str().unwrap_or("").contains("text/event-stream"))
            .unwrap_or(false),
        "response must be SSE"
    );
    let text = resp.text().await.unwrap();

    let sid1 =
        parse_session_id(&text).expect("first turn must emit an event: session_started item");
    assert!(!sid1.is_empty(), "server must mint a non-empty session_id");
    assert!(
        text.contains("event: aikit_text_delta"),
        "stream must contain the stub text event; got:\n{}",
        text
    );
    assert!(
        text.contains("event: done"),
        "stream must contain done; got:\n{}",
        text
    );

    // The session should now be listable under its server-minted id.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let resp = client
        .get(format!("{}/api/v1/sessions/{}", base, sid1))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["session_id"], sid1);
    assert_eq!(body["agent"], "aikit");
    assert_eq!(body["status"], "idle");

    // ── 2. Second turn: same session_id → server returns the same UUID ──
    let resp = client
        .post(format!("{}/api/v1/messages", base))
        .json(&serde_json::json!({
            "agent": "aikit",
            "session_id": sid1,
            "content": "again",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    let sid2 = parse_session_id(&text).expect("resume must emit a session frame");
    assert_eq!(sid2, sid1, "resume must return the same server session_id");

    // ── 3. Third turn: no session_id again → a fresh, different UUID ──
    let resp = client
        .post(format!("{}/api/v1/messages", base))
        .json(&serde_json::json!({"agent": "aikit", "content": "new topic"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    let sid3 = parse_session_id(&text).expect("third turn must emit a session frame");
    assert!(!sid3.is_empty(), "server must mint a non-empty session_id");
    assert_ne!(
        sid3, sid1,
        "new conversation must have a different session_id"
    );
}

#[tokio::test]
async fn test_accept_application_json_returns_sync() {
    // Stub emits two text frames; sync mode concatenates them and returns
    // a single JSON body — no SSE. Selection is driven entirely by `Accept`.
    let run_fn = make_stub_run_fn_with_session(
        vec![text_event("Hello, "), text_event("world!")],
        Some("sync-session-1".to_string()),
    );
    let port = start_server_with(run_fn).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .header("Accept", "application/json")
        .json(&serde_json::json!({"agent": "aikit", "content": "hi"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.contains("application/json"),
        "Accept: application/json must return JSON, got content-type: {}",
        ct
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    let sid = body["session_id"]
        .as_str()
        .expect("session_id must be present");
    assert!(!sid.is_empty(), "server must emit a session_id");
    assert_eq!(body["content"], "Hello, world!");
    assert_eq!(body["exit_code"], 0);
    assert!(
        body.get("error").is_none(),
        "no error expected; got: {body}"
    );
}

#[tokio::test]
async fn test_accept_application_json_resume() {
    // First call (no session_id) creates the session in the in-memory tracker
    // under the stub's mint id. The second call (with that session_id) is
    // allowed through the resume pre-flight because it's known in memory, and
    // the stub then echoes the supplied id. Both turns use the JSON shape.
    let run_fn =
        make_stub_run_fn_with_session(vec![text_event("ok")], Some("sync-resume-test".to_string()));
    let port = start_server_with(run_fn).await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    let resp = client
        .post(format!("{}/api/v1/messages", base))
        .header("Accept", "application/json")
        .json(&serde_json::json!({"agent": "aikit", "content": "first"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let sid = body["session_id"]
        .as_str()
        .expect("session_id must be present")
        .to_string();
    assert!(!sid.is_empty(), "server must emit a session_id");

    // Resume uses the server-minted UUID, not the stub's backend token.
    let resp = client
        .post(format!("{}/api/v1/messages", base))
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "agent": "aikit",
            "session_id": sid,
            "content": "again",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["session_id"], sid,
        "resume must return the same server session_id"
    );
    assert_eq!(body["content"], "ok");
}

#[tokio::test]
async fn test_accept_event_stream_returns_sse() {
    let run_fn =
        make_stub_run_fn_with_session(vec![text_event("hi")], Some("sse-explicit".to_string()));
    let port = start_server_with(run_fn).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .header("Accept", "text/event-stream")
        .json(&serde_json::json!({"agent": "aikit", "content": "hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.contains("text/event-stream"),
        "Accept: text/event-stream must return SSE, got: {}",
        ct
    );
    let text = resp.text().await.unwrap();
    assert!(text.contains("event: session_started"));
    assert!(text.contains("event: done"));
}

#[tokio::test]
async fn test_default_accept_is_sse() {
    // reqwest sends no explicit Accept (or `*/*`); the server must fall
    // back to SSE.
    let run_fn =
        make_stub_run_fn_with_session(vec![text_event("hi")], Some("sse-default".to_string()));
    let port = start_server_with(run_fn).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .json(&serde_json::json!({"agent": "aikit", "content": "hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.contains("text/event-stream"),
        "default (no Accept) must return SSE, got: {}",
        ct
    );
}

#[tokio::test]
async fn test_sync_empty_content_with_nonzero_exit_surfaces_stderr() {
    // Simulates the failure mode where the agent process exits non-zero
    // with no recognisable stdout — exactly what happens when claude/gemini
    // print only an error to stderr. The sync handler must surface that
    // tail in the JSON body instead of returning `content:"" exit_code:0`.
    let stub = make_failing_stub_run_fn(2, "Error: model is overloaded, try later");
    let port = start_server_with(stub).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .header("Accept", "application/json")
        .json(&serde_json::json!({"agent": "aikit", "content": "hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["content"], "");
    assert_eq!(body["exit_code"], 2);
    assert_eq!(body["error"]["code"], "agent_error");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("model is overloaded"),
        "stderr tail must appear in error message; got: {body}"
    );
}

#[tokio::test]
async fn test_accept_unknown_returns_406() {
    let port = start_server_with(make_stub_run_fn_with_session(vec![], None)).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .header("Accept", "text/html")
        .json(&serde_json::json!({"agent": "aikit", "content": "hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 406);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "not_acceptable");
}

#[tokio::test]
async fn test_unknown_agent_returns_404_before_streaming() {
    let port = start_server_with(make_stub_run_fn_with_session(vec![], None)).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .json(&serde_json::json!({"agent": "definitely-not-real", "content": "hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "agent_not_found");
}

#[tokio::test]
async fn test_empty_agent_returns_422() {
    let port = start_server_with(make_stub_run_fn_with_session(vec![], None)).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .json(&serde_json::json!({"agent": "", "content": "hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn test_empty_content_returns_422() {
    let port = start_server_with(make_stub_run_fn_with_session(vec![], None)).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .json(&serde_json::json!({"agent": "aikit", "content": ""}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn test_aikit_resume_with_unknown_id_returns_404() {
    let port = start_server_with(make_stub_run_fn_with_session(vec![], None)).await;
    let client = reqwest::Client::new();

    // Use an env-isolated AIKIT_SESSIONS_DIR so this test never accidentally
    // collides with a real session on disk. We set it for this process, but
    // the spawned server inherits it via env::var lookup inside SessionStore.
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("AIKIT_SESSIONS_DIR", tmp.path());

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .json(&serde_json::json!({
            "agent": "aikit",
            "session_id": "00000000-0000-0000-0000-000000000000",
            "content": "resume nonexistent",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "session_not_found");

    std::env::remove_var("AIKIT_SESSIONS_DIR");
}

#[tokio::test]
async fn test_list_sessions_includes_completed_run() {
    let port = start_server_with(make_stub_run_fn_with_session(
        vec![text_event("hi")],
        Some("stub-list-test".to_string()),
    ))
    .await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    let resp = client
        .post(format!("{}/api/v1/messages", base))
        .json(&serde_json::json!({"agent": "aikit", "content": "hi"}))
        .send()
        .await
        .unwrap();
    let text = resp.text().await.unwrap();
    let sid = parse_session_id(&text).expect("must emit a session frame");

    tokio::time::sleep(Duration::from_millis(50)).await;

    let resp = client
        .get(format!("{}/api/v1/sessions", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let list = body["sessions"].as_array().unwrap();
    assert!(
        list.iter()
            .any(|s| s["session_id"] == sid && s["agent"] == "aikit"),
        "list must include the just-completed session; got: {:?}",
        list
    );
}

#[tokio::test]
async fn test_delete_session() {
    let port = start_server_with(make_stub_run_fn_with_session(
        vec![],
        Some("stub-delete-test".to_string()),
    ))
    .await;
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    let resp = client
        .post(format!("{}/api/v1/messages", base))
        .json(&serde_json::json!({"agent": "aikit", "content": "hi"}))
        .send()
        .await
        .unwrap();
    let text = resp.text().await.unwrap();
    // Capture the server-minted session_id — needed for DELETE and GET.
    let sid = parse_session_id(&text).expect("must emit a session frame");

    tokio::time::sleep(Duration::from_millis(50)).await;

    let resp = client
        .delete(format!("{}/api/v1/sessions/{}", base, sid))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "closed");

    // Subsequent GET is 404.
    let resp = client
        .get(format!("{}/api/v1/sessions/{}", base, sid))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_not_found_route() {
    let port = start_server_with(make_stub_run_fn_with_session(vec![], None)).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://127.0.0.1:{}/api/v1/nonexistent", port))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_sse_emits_token_usage_and_reasoning_events() {
    // ARCH-4 / ADR 0016: previously-dropped events now reach the SSE stream,
    // each under its own canonical tag — `stream_message` (reasoning is a
    // `kind`, not a distinct top-level tag) and `token_usage_line`.
    let run_fn = make_stub_run_fn_with_session(
        vec![
            reasoning_event("let me think"),
            token_usage_event(10, 20, Some(3)),
            text_event("answer"),
        ],
        Some("usage-sse-1".to_string()),
    );
    let port = start_server_with(run_fn).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .header("Accept", "text/event-stream")
        .json(&serde_json::json!({"agent": "aikit", "content": "hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();

    assert!(
        text.contains("event: stream_message") && text.contains("\"kind\":\"reasoning\""),
        "stream must contain a reasoning stream_message event; got:\n{text}"
    );
    assert!(
        text.contains("event: token_usage_line"),
        "stream must contain a token_usage_line event; got:\n{text}"
    );
    assert!(
        text.contains("\"input_tokens\":10") && text.contains("\"output_tokens\":20"),
        "token_usage_line event must carry the token counts; got:\n{text}"
    );
}

#[tokio::test]
async fn test_sync_aggregates_token_usage() {
    // E1/B13: sync body sums TokenUsage frames into a `usage` object.
    let run_fn = make_stub_run_fn_with_session(
        vec![
            token_usage_event(10, 5, Some(2)),
            token_usage_event(4, 6, Some(1)),
            text_event("done"),
        ],
        Some("usage-sync-1".to_string()),
    );
    let port = start_server_with(run_fn).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .header("Accept", "application/json")
        .json(&serde_json::json!({"agent": "aikit", "content": "hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["content"], "done");
    assert_eq!(body["usage"]["input_tokens"], 14);
    assert_eq!(body["usage"]["output_tokens"], 11);
    assert_eq!(body["usage"]["cache_read_tokens"], 3);
}

#[tokio::test]
async fn test_sync_without_usage_omits_field() {
    // Backward-compat: runs with no TokenUsage frames omit `usage` entirely.
    let run_fn =
        make_stub_run_fn_with_session(vec![text_event("hi")], Some("no-usage-1".to_string()));
    let port = start_server_with(run_fn).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/messages", port))
        .header("Accept", "application/json")
        .json(&serde_json::json!({"agent": "aikit", "content": "hi"}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body.get("usage").is_none(),
        "usage must be omitted when no token frames seen; got: {body}"
    );
}

#[tokio::test]
async fn test_legacy_create_session_endpoint_removed() {
    let port = start_server_with(make_stub_run_fn_with_session(vec![], None)).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/sessions", port))
        .json(&serde_json::json!({"agent": "aikit"}))
        .send()
        .await
        .unwrap();
    // POST /api/v1/sessions used to create a session; it must now be gone.
    // Either 404 (no route at all) or 405 (route exists for GET only) is OK.
    let s = resp.status().as_u16();
    assert!(
        s == 404 || s == 405,
        "POST /api/v1/sessions must not create sessions (got {})",
        s
    );
}
