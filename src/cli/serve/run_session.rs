//! One-shot run-session handlers (`/api/v1/messages`, `/api/v1/sessions`).
//!
//! Covers: SSE and sync response modes, agent-run orchestration, session
//! registry management, auth probing, and the `RunFn` injectable boundary.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{FromRequest, Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use aikit_sdk::session_store::{SessionStore, SessionStoreError};
use aikit_sdk::{get_agent_status, AgentEvent, AgentEventPayload, RunError, RunOptions};

use crate::core::agent::get_agent_configs;

use super::{
    error_response, spawn_frame_forwarder, sse_response_with_headers, AppState, ServeEvent,
};

// ── run record ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum RunStatus {
    Running,
    Idle,
    Closed,
}

#[derive(Clone)]
pub(super) struct RunRecord {
    /// Session token the underlying CLI backend recognises for `--resume`.
    /// May differ from the server-minted map key.  `None` while the first
    /// turn is in flight and the backend hasn't yet returned a session id.
    pub backend_session_id: Option<String>,
    pub agent: String,
    pub status: RunStatus,
    /// When the session was first opened (not updated on resume turns).
    pub started_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
    pub abort_handle: Option<tokio::task::AbortHandle>,
    /// BUG-2 / ADR 0014: the cancel handle bound to this run's subprocess.
    /// Held so a timeout, a client disconnect, or an explicit `DELETE` can
    /// all terminate the same run through one mechanism.
    pub cancel: Option<aikit_sdk::runner::RunCancelHandle>,
    /// Captured stderr tail, set after the run completes.
    pub stderr_tail: String,
    pub last_exit_code: Option<i32>,
}

// ── run function type ─────────────────────────────────────────────────────────

/// Maximum bytes of stderr to surface in a JSON sync response.
pub(super) const MAX_STDERR_TAIL_BYTES: usize = 2048;

pub struct RunFnOutcome {
    pub exit_code: i32,
    pub session_id: Option<String>,
    pub stderr_tail: String,
}

/// Injectable agent-run function. The production implementation calls
/// `run_agent_events_cancellable`; test stubs inject canned frames.
///
/// BUG-2 / ADR 0014: callers pass a [`aikit_sdk::runner::RunCancelHandle`]
/// bound to this run so a timeout or client-disconnect on the serve side can
/// terminate the underlying subprocess through the one shared cancellation
/// mechanism rather than merely abandoning it.
pub type RunFn = Arc<
    dyn Fn(
            String,
            String,
            RunOptions,
            tokio::sync::mpsc::Sender<ServeEvent>,
            aikit_sdk::runner::RunCancelHandle,
        ) -> Result<RunFnOutcome, RunError>
        + Send
        + Sync,
>;

// ── auth ──────────────────────────────────────────────────────────────────────

pub(super) type AuthCache = Arc<Mutex<Option<(Instant, HashMap<String, AuthStatus>)>>>;

/// Per-backend authentication status.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum AuthStatus {
    Ok,
    Unauthenticated,
    Unknown,
}

const AUTH_CACHE_TTL: Duration = Duration::from_secs(60);
const AUTH_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

async fn probe_backend_auth(key: &str) -> AuthStatus {
    match key {
        "aikit" => env_auth(&["OPENAI_API_KEY", "AIKIT_API_KEY"]),
        "gemini" => env_auth(&["GEMINI_API_KEY", "GOOGLE_API_KEY"]),
        "codex" => spawn_status_probe("codex", &["login", "status"]).await,
        "claude" => spawn_status_probe("claude", &["auth", "status"]).await,
        "cursor" => {
            if env_auth(&["CURSOR_API_KEY"]) == AuthStatus::Ok {
                AuthStatus::Ok
            } else {
                spawn_status_probe("agent", &["status"]).await
            }
        }
        _ => AuthStatus::Unknown,
    }
}

fn env_auth(vars: &[&str]) -> AuthStatus {
    for v in vars {
        if std::env::var(v).map(|s| !s.is_empty()).unwrap_or(false) {
            return AuthStatus::Ok;
        }
    }
    AuthStatus::Unknown
}

async fn spawn_status_probe(bin: &str, args: &[&str]) -> AuthStatus {
    use std::process::Stdio;
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match tokio::time::timeout(AUTH_PROBE_TIMEOUT, cmd.status()).await {
        Ok(Ok(s)) if s.success() => AuthStatus::Ok,
        Ok(Ok(_)) => AuthStatus::Unauthenticated,
        _ => AuthStatus::Unknown,
    }
}

async fn probe_all(keys: &[String]) -> HashMap<String, AuthStatus> {
    let handles: Vec<_> = keys
        .iter()
        .map(|key| {
            let key = key.clone();
            tokio::spawn(async move { (key.clone(), probe_backend_auth(&key).await) })
        })
        .collect();
    let mut out = HashMap::new();
    for h in handles {
        if let Ok((key, status)) = h.await {
            out.insert(key, status);
        }
    }
    out
}

pub(super) async fn auth_statuses(
    state: &AppState,
    keys: &[String],
) -> HashMap<String, AuthStatus> {
    {
        let guard = state.auth_cache.lock().unwrap();
        if let Some((at, ref map)) = *guard {
            if at.elapsed() < AUTH_CACHE_TTL {
                return map.clone();
            }
        }
    }
    let fresh = probe_all(keys).await;
    {
        let mut guard = state.auth_cache.lock().unwrap();
        *guard = Some((Instant::now(), fresh.clone()));
    }
    fresh
}

// ── agent info ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(super) struct AgentInfo {
    pub key: String,
    pub name: String,
    pub available: bool,
    pub auth: AuthStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<aikit_sdk::BackendCapabilities>,
}

#[derive(Serialize)]
struct ListAgentsResponse {
    agents: Vec<AgentInfo>,
}

pub(super) fn build_runnable_agents() -> Vec<AgentInfo> {
    let status_map = get_agent_status();
    let configs = get_agent_configs();
    let mut agents: Vec<AgentInfo> = status_map
        .into_iter()
        .filter(|(_, status)| status.available)
        .map(|(key, _)| {
            let name = configs
                .iter()
                .find(|c| c.key == key)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| key.clone());
            AgentInfo {
                capabilities: aikit_sdk::runner::Backend::from_key(&key).map(|b| b.capabilities()),
                key,
                name,
                available: true,
                auth: AuthStatus::Unknown,
            }
        })
        .collect();
    agents.sort_by(|a, b| a.key.cmp(&b.key));
    agents
}

pub(super) async fn agents_handler(State(state): State<AppState>) -> impl IntoResponse {
    // BUG-5: `build_runnable_agents` probes every backend binary
    // synchronously; run it on the blocking-thread pool so it can't stall
    // the tokio worker driving this request (or any other request sharing
    // the runtime) while a probe is in flight.
    let mut agents = tokio::task::spawn_blocking(build_runnable_agents)
        .await
        .unwrap_or_default();
    let keys: Vec<String> = agents.iter().map(|a| a.key.clone()).collect();
    let statuses = auth_statuses(&state, &keys).await;
    for a in &mut agents {
        a.auth = statuses.get(&a.key).copied().unwrap_or(AuthStatus::Unknown);
    }
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&ListAgentsResponse { agents })
            .unwrap_or_else(|_| r#"{"agents":[]}"#.to_string()),
    )
        .into_response()
}

// ── HTTP body / response types ────────────────────────────────────────────────

/// B4: Custom extractor that maps `JsonRejection` into the standard error envelope.
pub(super) struct JsonBody(pub SendMessageRequest);

impl<S> FromRequest<S> for JsonBody
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<SendMessageRequest>::from_request(req, state)
            .await
            .map(|Json(body)| JsonBody(body))
            .map_err(|rejection| {
                use axum::extract::rejection::JsonRejection;
                let (status, message) = match &rejection {
                    JsonRejection::JsonDataError(e) => {
                        (StatusCode::UNPROCESSABLE_ENTITY, e.body_text())
                    }
                    JsonRejection::JsonSyntaxError(e) => (StatusCode::BAD_REQUEST, e.body_text()),
                    JsonRejection::MissingJsonContentType(e) => {
                        (StatusCode::UNSUPPORTED_MEDIA_TYPE, e.body_text())
                    }
                    _ => (rejection.status(), rejection.body_text()),
                };
                error_response(status, "invalid_request", &message)
            })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SendMessageRequest {
    pub agent: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub content: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub yolo: bool,
    /// D2 / ADR 0012: opt-in least-privilege tool allowlist for this run.
    /// Threaded through `RunOptions::session_persona` into the existing
    /// `AgentPersona.tools` hard filter (`aikit-agent/src/loop_runner.rs`).
    /// Absent (the default) leaves the full toolset unchanged — this is a
    /// capability lever, not a security gate; the sandbox remains the trust
    /// boundary. No-op for external CLI backends, which have no equivalent
    /// tool-filter mechanism.
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    /// D2 / ADR 0012: opt-in tool denylist, same mechanism as `tools`
    /// above (`AgentPersona.disallowed_tools`).
    #[serde(default)]
    pub disallowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseMode {
    Sse,
    Sync,
    NotAcceptable,
}

fn resolve_response_mode(headers: &axum::http::HeaderMap) -> ResponseMode {
    let raw = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if raw.is_empty() || raw.contains("*/*") {
        return ResponseMode::Sse;
    }
    let wants_sse = raw.contains("text/event-stream");
    let wants_json = raw.contains("application/json");
    match (wants_sse, wants_json) {
        (true, _) => ResponseMode::Sse,
        (false, true) => ResponseMode::Sync,
        (false, false) => ResponseMode::NotAcceptable,
    }
}

#[derive(Serialize)]
struct RunSummary {
    session_id: String,
    agent: String,
    status: RunStatus,
    started_at: DateTime<Utc>,
    last_active_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct ListRunsResponse {
    sessions: Vec<RunSummary>,
}

#[derive(Serialize)]
struct DeleteSessionResponse {
    session_id: String,
    status: RunStatus,
}

#[derive(Serialize)]
struct SyncMessageResponse {
    session_id: Option<String>,
    content: String,
    exit_code: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<super::ErrorDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<UsageSummary>,
}

#[derive(Serialize)]
struct UsageSummary {
    input_tokens: u64,
    output_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_read_tokens: Option<u64>,
}

// ── validation ────────────────────────────────────────────────────────────────

pub(super) fn validate_request(
    body: &SendMessageRequest,
    state: &AppState,
    runnable: &[AgentInfo],
) -> Option<Response> {
    if body.agent.trim().is_empty() {
        return Some(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            "agent field is required",
        ));
    }
    if body.content.trim().is_empty() {
        return Some(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            "content must not be empty",
        ));
    }
    // SEC-10: reject a session_id that isn't a safe flat identifier before
    // it's ever used to build a filesystem path (SessionStore) or probed for
    // existence — closes both the path-traversal write and the
    // existence-probe oracle.
    if let Some(ref sid) = body.session_id {
        if !aikit_sdk::is_safe_id(sid) {
            return Some(error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_request",
                "session_id must be a safe identifier (alphanumeric, '-', '_', '.', max 128 chars)",
            ));
        }
    }
    if !runnable.iter().any(|a| a.key == body.agent) {
        return Some(error_response(
            StatusCode::NOT_FOUND,
            "agent_not_found",
            &format!(
                "Agent '{}' is not available. Use GET /api/v1/agents to see available agents.",
                body.agent
            ),
        ));
    }
    if let Some(ref sid) = body.session_id {
        let in_memory = state.runs.lock().unwrap().contains_key(sid.as_str());
        if !in_memory {
            // For the aikit backend, fall back to the persistent SessionStore so
            // cross-restart resume works when the client holds the aikit session id.
            if body.agent == "aikit" {
                let store = SessionStore::open();
                match store.load(sid) {
                    Ok(_) => {}
                    Err(SessionStoreError::NotFound(id)) => {
                        return Some(error_response(
                            StatusCode::NOT_FOUND,
                            "session_not_found",
                            &format!("Session '{}' not found", id),
                        ));
                    }
                    Err(e) => {
                        tracing::warn!("session store error: {:?}", e);
                        return Some(error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "internal_error",
                            "Failed to load session",
                        ));
                    }
                }
            } else {
                return Some(error_response(
                    StatusCode::NOT_FOUND,
                    "session_not_found",
                    &format!("Session '{}' not found", sid),
                ));
            }
        }
    }
    None
}

/// Classify backend stderr into a stable error code.
pub(super) fn classify_error_code(stderr_or_content: &str) -> &'static str {
    let hay = stderr_or_content.to_ascii_lowercase();
    const PATTERNS: &[&str] = &[
        "invalid api key",
        "authentication required",
        "not logged in",
        "unauthorized",
        "cursor_api_key",
        "fix external api key",
        "no api key",
    ];
    if PATTERNS.iter().any(|p| hay.contains(p)) {
        return "unauthenticated";
    }
    if hay.contains("please run") && hay.contains("login") {
        return "unauthenticated";
    }
    "agent_error"
}

fn run_error_to_serve_event(err: RunError) -> (ServeEvent, i32) {
    let item = match err {
        RunError::TimedOut { .. } => ServeEvent::Error {
            code: "run_timeout".to_string(),
            message: "Run exceeded timeout".to_string(),
        },
        RunError::SessionNotFound(id) => ServeEvent::Error {
            code: "session_not_found".to_string(),
            message: format!("Session '{}' not found", id),
        },
        RunError::AgentNotRunnable(msg) => ServeEvent::Error {
            code: "agent_not_runnable".to_string(),
            message: msg,
        },
        e => {
            tracing::error!("run error: {}", e);
            ServeEvent::Error {
                code: "internal_error".to_string(),
                message: "Internal error".to_string(),
            }
        }
    };
    (item, 1)
}

// ── spawn_run ─────────────────────────────────────────────────────────────────

/// B3/B5: `runs` is keyed by a stable server-minted `session_id` so that
/// multi-turn conversations occupy exactly one record.  Resume turns upsert
/// into the existing record rather than inserting a sibling.
///
/// The returned `String` is the `server_session_id` — the key clients use for
/// subsequent turns and for `GET/DELETE /api/v1/sessions/{id}`.
#[allow(clippy::result_large_err)]
pub(super) fn spawn_run(
    state: &AppState,
    body: &SendMessageRequest,
) -> Result<(tokio::sync::mpsc::Receiver<ServeEvent>, String), Response> {
    let now = Utc::now();

    // Stable server-assigned session id.  For new sessions we mint a UUID; for
    // resume turns the client supplies the id it received from the prior turn.
    let server_session_id = body
        .session_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // The backend resume token (may differ from server_session_id for external
    // CLIs whose own session id is returned in the stream).
    let backend_session_id_for_run: Option<String>;

    // BUG-2 / ADR 0014: one cancel handle for this run, shared by the
    // production `run_fn` (which binds it to the actual child process), the
    // server-level timeout branch below, and a client-disconnect. Whichever
    // fires first wins; `cancel()` is idempotent.
    let cancel = aikit_sdk::runner::RunCancelHandle::new();

    {
        let mut runs = state.runs.lock().unwrap();

        // B9 (atomic busy + capacity + upsert under one lock).
        if let Some(r) = runs.get(&server_session_id) {
            if r.status == RunStatus::Running {
                return Err(error_response(
                    StatusCode::CONFLICT,
                    "session_busy",
                    "Session is currently processing a message",
                ));
            }
            // BUG-10: a deleted/timed-out session is terminal. Resuming it
            // via POST must fail the same way GET/DELETE already do, or a
            // session that looks gone everywhere else comes back to life.
            if r.status == RunStatus::Closed {
                return Err(error_response(
                    StatusCode::NOT_FOUND,
                    "session_not_found",
                    &format!("Session '{}' not found", server_session_id),
                ));
            }
            // For an in-memory resume turn use the recorded backend token.
            // If the backend never returned one, fall back to the server id
            // (works for aikit whose session_id == backend session_id).
            backend_session_id_for_run = r
                .backend_session_id
                .clone()
                .or_else(|| body.session_id.clone());
        } else {
            // New session or cross-restart aikit resume: treat the client-
            // provided id as the backend token (it IS the aikit session_id).
            backend_session_id_for_run = body.session_id.clone();
        }

        let active = runs
            .values()
            .filter(|r| r.status == RunStatus::Running)
            .count();
        if active >= state.config.max_sessions {
            return Err(error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "session_limit_reached",
                &format!(
                    "Maximum of {} concurrent sessions reached",
                    state.config.max_sessions
                ),
            ));
        }

        // Upsert: update an existing record in place or insert a fresh one.
        // `started_at` is preserved on resume turns so it reflects when the
        // session was first opened.
        let record = runs
            .entry(server_session_id.clone())
            .or_insert_with(|| RunRecord {
                backend_session_id: backend_session_id_for_run.clone(),
                agent: body.agent.clone(),
                status: RunStatus::Running,
                started_at: now,
                last_active_at: now,
                abort_handle: None,
                cancel: None,
                stderr_tail: String::new(),
                last_exit_code: None,
            });
        record.status = RunStatus::Running;
        record.last_active_at = now;
        record.abort_handle = None;
        record.cancel = Some(cancel.clone());
        record.stderr_tail = String::new();
        record.last_exit_code = None;
    }

    let content = body.content.clone();
    let model = body.model.clone();
    let yolo = body.yolo;
    let agent = body.agent.clone();
    // D2 / ADR 0012: opt-in tool policy, plumbed through the existing
    // `session_persona` mechanism (see below) rather than a parallel one.
    let tools = body.tools.clone();
    let disallowed_tools = body.disallowed_tools.clone();
    let timeout = Duration::from_secs(state.config.run_timeout_secs);
    let run_fn = state.run_fn.clone();
    let runs_ref = Arc::clone(&state.runs);

    let (inner_tx, mut inner_rx) = tokio::sync::mpsc::channel::<ServeEvent>(64);
    let (outer_tx, rx) = tokio::sync::mpsc::channel::<ServeEvent>(64);

    // B5: emit the server-minted session id as the very first event so
    // clients have a stable, resolvable id before any backend events arrive.
    // This is serve's own bookkeeping, expressed as the canonical
    // `SessionStarted` payload (ARCH-4 / ADR 0016) rather than a serve-private
    // frame shape.
    let _ = outer_tx.try_send(ServeEvent::Agent(AgentEvent {
        agent_key: body.agent.clone(),
        seq: 0,
        stream: aikit_sdk::AgentEventStream::Stdout,
        payload: AgentEventPayload::SessionStarted {
            session_id: server_session_id.clone(),
        },
    }));

    // B11: relay so back-end `SessionStarted` events (backend token) update
    // the record without racing with the initial synthetic event above.
    let relay_runs = Arc::clone(&runs_ref);
    let relay_sid = server_session_id.clone();
    let relay_outer_tx = outer_tx.clone();
    tokio::spawn(async move {
        while let Some(item) = inner_rx.recv().await {
            // When the backend emits its own session id, store it as the
            // resume token but do NOT forward another SessionStarted event
            // (we already sent one).
            if let ServeEvent::Agent(AgentEvent {
                payload: AgentEventPayload::SessionStarted { ref session_id },
                ..
            }) = item
            {
                let mut runs = relay_runs.lock().unwrap();
                if let Some(r) = runs.get_mut(&relay_sid) {
                    if r.backend_session_id.is_none() {
                        r.backend_session_id = Some(session_id.clone());
                    }
                }
                // Suppress the duplicate SessionStarted event from the backend.
                continue;
            }
            if relay_outer_tx.send(item).await.is_err() {
                break;
            }
        }
    });

    let tx = outer_tx;
    let sid_clone = server_session_id.clone();

    let task = tokio::task::spawn(async move {
        let mut options = RunOptions::new()
            .with_yolo(yolo)
            .with_stream(true)
            .with_timeout(timeout);
        // Pass the backend resume token (not the server id) so external CLIs
        // receive the id they originally issued (e.g. claude --resume <token>).
        if let Some(ref backend_sid) = backend_session_id_for_run {
            options = options.with_session_id(backend_sid);
        }
        if let Some(m) = model {
            options = options.with_model(m);
        }
        // D2 / ADR 0012: when the request carries a tool policy, thread it
        // into `RunOptions::session_persona` — the SAME mechanism
        // `--session-persona` already uses (see `src/cli/run.rs`) to reach
        // `aikit_agent_adapter::apply_session_options`, which deserializes
        // this into `AgentPersona` and sets `AgentConfig.session_persona`,
        // hard-filtered by `build_tools` in
        // `aikit-agent/src/loop_runner.rs`. `name`/`description`/`prompt`
        // are required by `AgentPersona`'s `Deserialize` impl but unused
        // here beyond an empty (no-op) prompt — this is a tool-policy
        // overlay, not a persona swap. Absent (both None), behavior is
        // unchanged: full default toolset. This only affects the in-process
        // `aikit` backend, which is the only one that reads
        // `session_persona`; external CLI backends silently ignore it.
        if tools.is_some() || disallowed_tools.is_some() {
            options = options.with_session_persona(serde_json::json!({
                "name": "",
                "description": "",
                "prompt": "",
                "tools": tools,
                "disallowed_tools": disallowed_tools,
            }));
        }

        let tx_inner = inner_tx.clone();
        let cancel_for_blocking = cancel.clone();
        let blocking_handle = tokio::task::spawn_blocking(move || {
            run_fn(agent, content, options, tx_inner, cancel_for_blocking)
        });

        // B12: server-level wall-clock timeout.
        let outcome: Result<RunFnOutcome, RunError> = tokio::select! {
            timed_out = tokio::time::timeout(timeout, blocking_handle) => {
                match timed_out {
                    Err(_elapsed) => {
                        // BUG-2 / ADR 0014: actually cancel the run — kill
                        // the child (SIGTERM -> grace -> SIGKILL over its
                        // process group) rather than merely abandoning the
                        // JoinHandle. `cancel()` is idempotent, so if the
                        // run_fn's own internal watchdog gets there first
                        // this is a harmless no-op. `cancel()` itself blocks
                        // its calling thread for up to the ~3s grace period,
                        // so it's dispatched onto the blocking pool rather
                        // than run directly on this tokio worker; we don't
                        // await it — the response can close immediately
                        // while termination finishes in the background.
                        spawn_cancel(&cancel);
                        let _ = tx.send(ServeEvent::Error {
                            code: "run_timeout".to_string(),
                            message: "Run exceeded timeout".to_string(),
                        }).await;
                        let mut runs = runs_ref.lock().unwrap();
                        if let Some(r) = runs.get_mut(&sid_clone) {
                            // BUG-2: terminal state, NOT Idle — Idle would
                            // pass the busy check above and let a second
                            // turn start a concurrent run on this session
                            // while the first one may still be dying.
                            r.status = RunStatus::Closed;
                            r.last_active_at = Utc::now();
                            r.abort_handle = None;
                            r.cancel = None;
                            r.last_exit_code = Some(1);
                        }
                        prune_closed_and_stale(&mut runs);
                        drop(tx);
                        return;
                    }
                    Ok(join_result) => match join_result {
                        Ok(r) => r,
                        Err(join_err) => {
                            spawn_cancel(&cancel);
                            if !join_err.is_cancelled() {
                                tracing::error!("spawn_blocking join error: {}", join_err);
                            }
                            let mut runs = runs_ref.lock().unwrap();
                            if let Some(r) = runs.get_mut(&sid_clone) {
                                r.status = RunStatus::Closed;
                                r.last_active_at = Utc::now();
                                r.abort_handle = None;
                                r.cancel = None;
                            }
                            prune_closed_and_stale(&mut runs);
                            return;
                        }
                    },
                }
            }
            _ = tx.closed() => {
                // BUG-2/BUG-7: the client went away (SSE stream dropped or
                // sync request cancelled) — cancel the underlying run
                // instead of leaving it to finish unobserved, and close out
                // the session terminally so it can't be resumed mid-flight.
                spawn_cancel(&cancel);
                let mut runs = runs_ref.lock().unwrap();
                if let Some(r) = runs.get_mut(&sid_clone) {
                    r.status = RunStatus::Closed;
                    r.last_active_at = Utc::now();
                    r.abort_handle = None;
                    r.cancel = None;
                }
                prune_closed_and_stale(&mut runs);
                return;
            }
        };

        let _exit_code = match outcome {
            Ok(out) => {
                {
                    let mut runs = runs_ref.lock().unwrap();
                    if let Some(r) = runs.get_mut(&sid_clone) {
                        if r.backend_session_id.is_none() {
                            r.backend_session_id = out.session_id.clone();
                        }
                        r.stderr_tail = out.stderr_tail.clone();
                        r.last_exit_code = Some(out.exit_code);
                    }
                }
                if out.exit_code != 0 && !out.stderr_tail.is_empty() {
                    let code = classify_error_code(&out.stderr_tail).to_string();
                    let _ = tx
                        .send(ServeEvent::Error {
                            code,
                            message: out.stderr_tail.clone(),
                        })
                        .await;
                }
                out.exit_code
            }
            Err(err) => {
                let (item, code) = run_error_to_serve_event(err);
                let _ = tx.send(item).await;
                code
            }
        };

        {
            let mut runs = runs_ref.lock().unwrap();
            if let Some(r) = runs.get_mut(&sid_clone) {
                r.status = RunStatus::Idle;
                r.last_active_at = Utc::now();
                r.abort_handle = None;
                // The run completed on its own (or errored) without needing
                // cancellation; nothing left to hold the handle for.
                r.cancel = None;
            }
            prune_closed_and_stale(&mut runs);
        }
        drop(tx);
        let _ = _exit_code;
    });

    {
        let mut runs = state.runs.lock().unwrap();
        if let Some(r) = runs.get_mut(&server_session_id) {
            r.abort_handle = Some(task.abort_handle());
        }
    }
    drop(task);
    Ok((rx, server_session_id))
}

/// Dispatch `cancel.cancel()` onto the blocking-thread pool without
/// awaiting it. `RunCancelHandle::cancel()` blocks its calling thread for up
/// to the ~3s SIGTERM->SIGKILL grace period (see
/// `transport::subprocess::kill_process_group`); calling it directly from an
/// async task would stall that tokio worker for the same span. Fire-and-
/// forget is safe here because the handle's own `Drop` backstop guarantees
/// the process group is reaped even if the spawned task were somehow never
/// polled to completion.
fn spawn_cancel(cancel: &aikit_sdk::runner::RunCancelHandle) {
    let cancel = cancel.clone();
    tokio::task::spawn_blocking(move || cancel.cancel());
}

/// B2: prune stale records after each run — `Closed` sessions are removed
/// outright (deletion/terminal-timeout is final; BUG-10 also relies on
/// `spawn_run` rejecting a `Closed` resume before a record is ever pruned
/// away), and `Idle` sessions older than an hour are swept to bound memory.
fn prune_closed_and_stale(runs: &mut HashMap<String, RunRecord>) {
    let one_hour_ago = Utc::now() - chrono::Duration::hours(1);
    runs.retain(|_, r| {
        r.status != RunStatus::Closed
            && !(r.status == RunStatus::Idle && r.last_active_at < one_hour_ago)
    });
}

// ── handlers ──────────────────────────────────────────────────────────────────

pub(super) async fn messages_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    JsonBody(body): JsonBody,
) -> Response {
    let mode = resolve_response_mode(&headers);
    if mode == ResponseMode::NotAcceptable {
        return error_response(
            StatusCode::NOT_ACCEPTABLE,
            "not_acceptable",
            "Accept must include text/event-stream or application/json",
        );
    }
    // BUG-5: resolve the runnable-agent list off the async worker thread.
    let runnable = tokio::task::spawn_blocking(build_runnable_agents)
        .await
        .unwrap_or_default();
    if let Some(err) = validate_request(&body, &state, &runnable) {
        return err;
    }
    let (rx, server_session_id) = match spawn_run(&state, &body) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match mode {
        ResponseMode::Sse => {
            let stream = spawn_frame_forwarder(rx, {
                let runs = Arc::clone(&state.runs);
                let server_session_id = server_session_id.clone();
                move |saw_error| {
                    runs.lock()
                        .unwrap()
                        .get(&server_session_id)
                        .and_then(|r| r.last_exit_code)
                        .unwrap_or(if saw_error { 1 } else { 0 })
                }
            });
            sse_response_with_headers(stream, None)
        }
        ResponseMode::Sync => sync_response(rx, Arc::clone(&state.runs), server_session_id).await,
        ResponseMode::NotAcceptable => unreachable!(),
    }
}

/// Drain the receiver and return a single JSON body.
async fn sync_response(
    mut rx: tokio::sync::mpsc::Receiver<ServeEvent>,
    runs: Arc<Mutex<HashMap<String, RunRecord>>>,
    server_session_id: String,
) -> Response {
    use aikit_sdk::{MessageKind, MessageRole};

    let mut session_id: Option<String> = None;
    let mut content = String::new();
    let mut error: Option<super::ErrorDetail> = None;
    let mut usage_seen = false;
    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;
    let mut cache_read_tokens: Option<u64> = None;

    while let Some(item) = rx.recv().await {
        match item {
            ServeEvent::Error { code, message } => {
                if error.is_none() {
                    error = Some(super::ErrorDetail { code, message });
                }
            }
            ServeEvent::Agent(event) => match event.payload {
                AgentEventPayload::SessionStarted { session_id: id } => {
                    if session_id.is_none() {
                        session_id = Some(id);
                    }
                }
                AgentEventPayload::StreamMessage(msg)
                    if msg.role == MessageRole::Assistant && msg.kind == MessageKind::Message =>
                {
                    content.push_str(&msg.text);
                }
                AgentEventPayload::AikitTextDelta { content: c, .. } => content.push_str(&c),
                // Mirrors the production run_fn's own Final-after-Delta dedup:
                // AikitTextFinal repeats what AikitTextDelta already sent.
                AgentEventPayload::AikitTextFinal { .. } => {}
                AgentEventPayload::TokenUsageLine { usage, .. } => {
                    usage_seen = true;
                    input_tokens = input_tokens.saturating_add(usage.input_tokens);
                    output_tokens = output_tokens.saturating_add(usage.output_tokens);
                    if let Some(c) = usage.cache_read_tokens {
                        cache_read_tokens = Some(cache_read_tokens.unwrap_or(0).saturating_add(c));
                    }
                }
                AgentEventPayload::QuotaExceeded { info, .. } => {
                    if error.is_none() {
                        error = Some(super::ErrorDetail {
                            code: "quota_exceeded".to_string(),
                            message: info.raw_message,
                        });
                    }
                }
                // Everything else (tool use/result, reasoning, subagent,
                // context compression, step finish, raw lines, …) has no
                // representation in the accumulated sync-mode body; SSE mode
                // is where full fidelity is available.
                _ => {}
            },
        }
    }

    let usage = usage_seen.then_some(UsageSummary {
        input_tokens,
        output_tokens,
        cache_read_tokens,
    });

    let (record_stderr, record_exit) = {
        let guard = runs.lock().unwrap();
        if let Some(r) = guard.get(&server_session_id) {
            (r.stderr_tail.clone(), r.last_exit_code)
        } else {
            (String::new(), None)
        }
    };

    let (exit_code, error) = if let Some(e) = error {
        let code = record_exit.filter(|c| *c != 0).unwrap_or(1);
        (code, Some(e))
    } else {
        let code = record_exit.unwrap_or(0);
        if code != 0 {
            let message = if !record_stderr.is_empty() {
                record_stderr.clone()
            } else {
                format!("Agent exited with code {}", code)
            };
            (
                code,
                Some(super::ErrorDetail {
                    code: classify_error_code(&message).to_string(),
                    message,
                }),
            )
        } else {
            (code, None)
        }
    };

    let resp = SyncMessageResponse {
        session_id,
        content,
        exit_code,
        error,
        usage,
    };
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&resp).unwrap_or_default(),
    )
        .into_response()
}

pub(super) async fn list_runs_handler(State(state): State<AppState>) -> impl IntoResponse {
    let runs = state.runs.lock().unwrap();
    let sessions: Vec<RunSummary> = runs
        .iter()
        .filter(|(_, r)| r.status != RunStatus::Closed)
        .map(|(sid, r)| RunSummary {
            session_id: sid.clone(),
            agent: r.agent.clone(),
            status: r.status.clone(),
            started_at: r.started_at,
            last_active_at: r.last_active_at,
        })
        .collect();
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&ListRunsResponse { sessions }).unwrap_or_default(),
    )
}

pub(super) async fn get_run_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let runs = state.runs.lock().unwrap();
    match runs
        .get(&session_id)
        .filter(|r| r.status != RunStatus::Closed)
    {
        Some(r) => {
            let resp = RunSummary {
                session_id: session_id.clone(),
                agent: r.agent.clone(),
                status: r.status.clone(),
                started_at: r.started_at,
                last_active_at: r.last_active_at,
            };
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                serde_json::to_string(&resp).unwrap_or_default(),
            )
                .into_response()
        }
        None => error_response(
            StatusCode::NOT_FOUND,
            "session_not_found",
            "Session not found",
        ),
    }
}

pub(super) async fn delete_run_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let (abort_handle, cancel): (
        Option<tokio::task::AbortHandle>,
        Option<aikit_sdk::runner::RunCancelHandle>,
    ) = {
        let mut runs = state.runs.lock().unwrap();
        match runs.get_mut(&session_id) {
            None
            | Some(RunRecord {
                status: RunStatus::Closed,
                ..
            }) => {
                return error_response(
                    StatusCode::NOT_FOUND,
                    "session_not_found",
                    "Session not found",
                );
            }
            Some(r) => {
                r.status = RunStatus::Closed;
                (r.abort_handle.take(), r.cancel.take())
            }
        }
    };
    // ADR 0014: DELETE terminates the underlying subprocess through the same
    // cancel mechanism as a timeout or client disconnect, not just detaching
    // the tokio task that was draining it.
    if let Some(c) = cancel {
        spawn_cancel(&c);
    }
    if let Some(handle) = abort_handle {
        handle.abort();
    }
    let resp = DeleteSessionResponse {
        session_id,
        status: RunStatus::Closed,
    };
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&resp).unwrap_or_default(),
    )
        .into_response()
}

// ── test stub helpers ─────────────────────────────────────────────────────────

/// Build a synthetic `SessionStarted` item, matching the shape the
/// production `run_fn` emits for B5 (see `mod.rs::make_production_run_fn`).
fn stub_session_started(agent: &str, session_id: &str) -> ServeEvent {
    ServeEvent::Agent(AgentEvent {
        agent_key: agent.to_string(),
        seq: 0,
        stream: aikit_sdk::AgentEventStream::Stdout,
        payload: AgentEventPayload::SessionStarted {
            session_id: session_id.to_string(),
        },
    })
}

#[allow(dead_code)]
pub fn make_stub_run_fn_with_session(
    items: Vec<ServeEvent>,
    session_id_for_new: Option<String>,
) -> RunFn {
    Arc::new(move |agent, _prompt, options, tx, _cancel| {
        let sid: Option<String> = options
            .session_id
            .clone()
            .or_else(|| session_id_for_new.clone());
        if let Some(ref id) = sid {
            let _ = tx.blocking_send(stub_session_started(&agent, id));
        }
        for item in &items {
            let _ = tx.blocking_send(item.clone());
        }
        Ok(RunFnOutcome {
            exit_code: 0,
            session_id: sid,
            stderr_tail: String::new(),
        })
    })
}

/// D2 / ADR 0012: a stub `run_fn` that records the `RunOptions` each
/// invocation was actually built with (in particular `session_persona`,
/// which carries the tool policy) so a test can assert on what `spawn_run`
/// constructed, not just on the resulting event stream.
#[allow(dead_code)]
pub fn make_capturing_stub_run_fn() -> (RunFn, Arc<Mutex<Vec<RunOptions>>>) {
    let captured: Arc<Mutex<Vec<RunOptions>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_for_closure = Arc::clone(&captured);
    let run_fn: RunFn = Arc::new(move |agent, _prompt, options, tx, _cancel| {
        captured_for_closure.lock().unwrap().push(options.clone());
        let sid = options.session_id.clone();
        if let Some(ref id) = sid {
            let _ = tx.blocking_send(stub_session_started(&agent, id));
        }
        Ok(RunFnOutcome {
            exit_code: 0,
            session_id: sid,
            stderr_tail: String::new(),
        })
    });
    (run_fn, captured)
}

#[allow(dead_code)]
pub fn make_stub_run_fn() -> RunFn {
    make_stub_run_fn_with_session(vec![], None)
}

#[allow(dead_code)]
pub fn make_blocking_stub_run_fn(duration: std::time::Duration) -> RunFn {
    Arc::new(move |agent, _prompt, options, tx, _cancel| {
        if let Some(ref id) = options.session_id {
            let _ = tx.blocking_send(stub_session_started(&agent, id));
        }
        std::thread::sleep(duration);
        Ok(RunFnOutcome {
            exit_code: 0,
            session_id: options.session_id.clone(),
            stderr_tail: String::new(),
        })
    })
}

#[allow(dead_code)]
pub fn make_failing_stub_run_fn(exit_code: i32, stderr_tail: &'static str) -> RunFn {
    let tail = stderr_tail.to_string();
    Arc::new(move |_agent, _prompt, _options, _tx, _cancel| {
        Ok(RunFnOutcome {
            exit_code,
            session_id: None,
            stderr_tail: tail.clone(),
        })
    })
}

#[allow(dead_code)]
pub fn make_timeout_stub_run_fn() -> RunFn {
    Arc::new(|_agent, _prompt, _options, _tx, _cancel| {
        Err(aikit_sdk::RunError::TimedOut {
            timeout: std::time::Duration::from_secs(1),
            stdout: vec![],
            stderr: vec![],
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_status_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&AuthStatus::Ok).unwrap(), "\"ok\"");
        assert_eq!(
            serde_json::to_string(&AuthStatus::Unauthenticated).unwrap(),
            "\"unauthenticated\""
        );
        assert_eq!(
            serde_json::to_string(&AuthStatus::Unknown).unwrap(),
            "\"unknown\""
        );
    }

    #[test]
    fn classify_error_code_detects_auth_patterns() {
        for s in [
            "Invalid API key · Fix external API key",
            "Authentication required to continue",
            "Error: not logged in",
            "Please run `codex login` first",
            "401 Unauthorized",
            "CURSOR_API_KEY is not set",
            "Fix external API key and retry",
            "no api key found",
        ] {
            assert_eq!(
                classify_error_code(s),
                "unauthenticated",
                "expected auth classification for: {s}"
            );
        }
    }

    #[test]
    fn whitespace_only_content_is_rejected() {
        use crate::cli::serve::ServeConfig;
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let state = AppState {
            runs: Arc::new(Mutex::new(HashMap::new())),
            live_sessions: Arc::new(Mutex::new(HashMap::new())),
            pending_live_sessions: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            config: ServeConfig {
                host: "127.0.0.1".into(),
                port: 8787,
                run_timeout_secs: 30,
                max_sessions: 10,
                api_key: None,
                insecure: false,
            },
            run_fn: make_stub_run_fn(),
            auth_cache: Arc::new(Mutex::new(None)),
        };

        for ws in ["   ", "\t", "\n", " \t\n "] {
            let body = SendMessageRequest {
                agent: "aikit".into(),
                session_id: None,
                content: ws.into(),
                model: None,
                yolo: false,
                tools: None,
                disallowed_tools: None,
            };
            assert!(
                validate_request(&body, &state, &[]).is_some(),
                "expected rejection for content={ws:?}"
            );
        }
    }

    // --- SEC-10: session_id must be a safe identifier ---

    #[test]
    fn malicious_session_id_is_rejected_by_validate_request() {
        use crate::cli::serve::ServeConfig;
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let state = AppState {
            runs: Arc::new(Mutex::new(HashMap::new())),
            live_sessions: Arc::new(Mutex::new(HashMap::new())),
            pending_live_sessions: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            config: ServeConfig {
                host: "127.0.0.1".into(),
                port: 8787,
                run_timeout_secs: 30,
                max_sessions: 10,
                api_key: None,
                insecure: false,
            },
            run_fn: make_stub_run_fn(),
            auth_cache: Arc::new(Mutex::new(None)),
        };

        let runnable = vec![AgentInfo {
            key: "aikit".to_string(),
            name: "aikit".to_string(),
            available: true,
            auth: AuthStatus::Unknown,
            capabilities: None,
        }];

        for malicious in [
            "../../etc/passwd",
            "../secret",
            "a/b",
            "/etc/passwd",
            "..",
            ".",
            "",
        ] {
            let body = SendMessageRequest {
                agent: "aikit".into(),
                session_id: Some(malicious.to_string()),
                content: "hi".into(),
                model: None,
                yolo: false,
                tools: None,
                disallowed_tools: None,
            };
            let resp = validate_request(&body, &state, &runnable);
            assert!(
                resp.is_some(),
                "expected rejection for session_id={malicious:?}"
            );
        }

        // A safe id with no matching in-memory record still fails (session
        // not found in the persistent store), but for a *different* reason
        // than the unsafe ids above — sanity check that the safe-id gate
        // isn't over-broad and that we actually reach the lookup logic.
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("AIKIT_SESSIONS_DIR", tmp.path());
        let body = SendMessageRequest {
            agent: "aikit".into(),
            session_id: Some("perfectly-safe_id.123".to_string()),
            content: "hi".into(),
            model: None,
            yolo: false,
            tools: None,
            disallowed_tools: None,
        };
        let resp = validate_request(&body, &state, &runnable);
        std::env::remove_var("AIKIT_SESSIONS_DIR");
        assert!(resp.is_some(), "unknown-but-safe id must still 404");
    }

    // --- BUG-10: a Closed session must not be resurrectable via POST ---

    #[test]
    fn closed_session_rejects_resume_post() {
        use crate::cli::serve::ServeConfig;
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let state = AppState {
            runs: Arc::new(Mutex::new(HashMap::new())),
            live_sessions: Arc::new(Mutex::new(HashMap::new())),
            pending_live_sessions: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            config: ServeConfig {
                host: "127.0.0.1".into(),
                port: 8787,
                run_timeout_secs: 30,
                max_sessions: 10,
                api_key: None,
                insecure: false,
            },
            run_fn: make_stub_run_fn(),
            auth_cache: Arc::new(Mutex::new(None)),
        };

        let session_id = "closed-session-id";
        {
            let mut runs = state.runs.lock().unwrap();
            runs.insert(
                session_id.to_string(),
                RunRecord {
                    backend_session_id: Some(session_id.to_string()),
                    agent: "aikit".to_string(),
                    status: RunStatus::Closed,
                    started_at: Utc::now(),
                    last_active_at: Utc::now(),
                    abort_handle: None,
                    cancel: None,
                    stderr_tail: String::new(),
                    last_exit_code: Some(0),
                },
            );
        }

        let body = SendMessageRequest {
            agent: "aikit".into(),
            session_id: Some(session_id.to_string()),
            content: "resume me".into(),
            model: None,
            yolo: false,
            tools: None,
            disallowed_tools: None,
        };

        let err = spawn_run(&state, &body).expect_err("a Closed session must reject resume");
        let (parts, _body) = err.into_parts();
        assert_eq!(
            parts.status,
            StatusCode::NOT_FOUND,
            "resuming a Closed session must 404, not silently resurrect it"
        );

        // The record must still be Closed afterward — spawn_run must not
        // have mutated it back to Running as a side effect of the attempt.
        let runs = state.runs.lock().unwrap();
        assert_eq!(
            runs.get(session_id).map(|r| r.status.clone()),
            Some(RunStatus::Closed)
        );
    }

    #[test]
    fn classify_error_code_non_auth_is_agent_error() {
        assert_eq!(
            classify_error_code("Error: model is overloaded, try later"),
            "agent_error"
        );
        assert_eq!(classify_error_code(""), "agent_error");
    }
}
