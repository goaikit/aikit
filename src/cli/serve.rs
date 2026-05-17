//! `aikit serve` — HTTP server for multi-turn agent sessions with SSE streaming.
//!
//! Session model:
//! - Sessions are created **implicitly** on the first `POST /v1/messages` call
//!   that omits `session_id`.
//! - The first SSE frame of a brand-new session is `event: session` carrying
//!   the freshly-assigned `session_id`. Subsequent calls quote that id in the
//!   request body to resume.
//! - For the `aikit` backend the id is assigned by the SDK (and persisted to
//!   `~/.aikit/sessions/...`). For other backends the id is treated as an
//!   opaque token forwarded to the underlying CLI's `--resume` flag; if the
//!   client doesn't supply one for a new run, no `session` frame is emitted.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use aikit_sdk::session_store::{SessionStore, SessionStoreError};
use aikit_sdk::{
    get_agent_status, AgentEventPayload, MessageKind, MessagePhase, MessageRole, RunError,
    RunOptions,
};

use crate::core::agent::get_agent_configs;

// ── public args ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct ServeArgs {
    pub host: String,
    pub port: u16,
    pub run_timeout_secs: u64,
    pub max_sessions: usize,
    pub api_key: Option<String>,
}

// ── internal config ───────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct ServeConfig {
    host: String,
    port: u16,
    run_timeout_secs: u64,
    max_sessions: usize,
    api_key: Option<String>,
}

// ── run record (in-memory) ────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum RunStatus {
    Running,
    Idle,
    Closed,
}

#[derive(Clone)]
struct RunRecord {
    /// SDK-assigned session_id (None until the run reports it or completes
    /// without one).
    session_id: Option<String>,
    /// Client-supplied or server-resolved agent key.
    agent: String,
    status: RunStatus,
    started_at: DateTime<Utc>,
    last_active_at: DateTime<Utc>,
    abort_handle: Option<tokio::task::AbortHandle>,
}

// ── run function type (injectable for tests) ──────────────────────────────────

pub struct RunFnOutcome {
    pub exit_code: i32,
    /// Session id assigned by the SDK. `None` when the backend doesn't expose
    /// one (e.g. non-aikit agents on a fresh run).
    pub session_id: Option<String>,
}

/// Type alias for the agent-run function injected into AppState.
///
/// Contract:
/// - Stream SSE events to the provided sender. The first frame for a fresh
///   session should be `event: session` with `{"session_id": "..."}` so the
///   client learns the id without waiting for `done`.
/// - Return the final exit code and (when known) the session_id.
pub type RunFn = Arc<
    dyn Fn(
            String,
            String,
            RunOptions,
            tokio::sync::mpsc::Sender<Result<Event, Infallible>>,
        ) -> Result<RunFnOutcome, RunError>
        + Send
        + Sync,
>;

// ── app state ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    /// Active and recently-completed runs, keyed by an internal request id.
    /// Cleared when a run is deleted or when the server shuts down.
    runs: Arc<Mutex<HashMap<String, RunRecord>>>,
    config: ServeConfig,
    run_fn: RunFn,
}

// ── HTTP body types ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SendMessageRequest {
    agent: String,
    #[serde(default)]
    session_id: Option<String>,
    content: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    yolo: bool,
}

#[derive(Serialize)]
struct AgentInfo {
    key: String,
    name: String,
    available: bool,
}

#[derive(Serialize)]
struct ListAgentsResponse {
    agents: Vec<AgentInfo>,
}

#[derive(Serialize)]
struct RunSummary {
    session_id: Option<String>,
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
struct SseDonePayload {
    exit_code: i32,
}

#[derive(Serialize)]
struct SseErrorPayload {
    code: String,
    message: String,
}

#[derive(Serialize)]
struct SseSessionPayload {
    session_id: String,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: String,
    message: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    let body = serde_json::to_string(&ErrorResponse {
        error: ErrorDetail {
            code: code.to_string(),
            message: message.to_string(),
        },
    })
    .unwrap_or_else(|_| {
        r#"{"error":{"code":"internal_error","message":"serialization failed"}}"#.to_string()
    });
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

// ── agents handler ────────────────────────────────────────────────────────────

async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state;
    let version = env!("CARGO_PKG_VERSION");
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        format!(r#"{{"status":"ok","version":"{}"}}"#, version),
    )
}

/// Build the list of runnable agents (those `aikit run` would accept).
///
/// Sources truth from the same SDK calls as `aikit check`: `get_agent_status`
/// for availability and `get_agent_configs` for display names. Filters to
/// only available agents — dev tools (git, vscode) and non-runnable entries
/// never appear.
fn build_runnable_agents() -> Vec<AgentInfo> {
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
                key,
                name,
                available: true,
            }
        })
        .collect();

    // get_agent_status returns BTreeMap so it's already sorted by key; but be
    // explicit so future changes don't surprise callers.
    agents.sort_by(|a, b| a.key.cmp(&b.key));
    agents
}

async fn agents_handler(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state;
    let agents = build_runnable_agents();
    let resp = ListAgentsResponse { agents };
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&resp).unwrap_or_else(|_| r#"{"agents":[]}"#.to_string()),
    )
        .into_response()
}

// ── session list/get/delete ───────────────────────────────────────────────────

async fn list_runs_handler(State(state): State<AppState>) -> impl IntoResponse {
    let runs = state.runs.lock().unwrap();
    let sessions: Vec<RunSummary> = runs
        .values()
        .filter(|r| r.status != RunStatus::Closed)
        .map(|r| RunSummary {
            session_id: r.session_id.clone(),
            agent: r.agent.clone(),
            status: r.status.clone(),
            started_at: r.started_at,
            last_active_at: r.last_active_at,
        })
        .collect();
    let resp = ListRunsResponse { sessions };
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&resp).unwrap_or_default(),
    )
}

async fn get_run_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let runs = state.runs.lock().unwrap();
    let found = runs
        .values()
        .find(|r| r.session_id.as_deref() == Some(&session_id) && r.status != RunStatus::Closed);
    match found {
        Some(r) => {
            let resp = RunSummary {
                session_id: r.session_id.clone(),
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

async fn delete_run_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let abort_handle = {
        let mut runs = state.runs.lock().unwrap();
        let key = runs.iter().find_map(|(k, r)| {
            if r.session_id.as_deref() == Some(&session_id) && r.status != RunStatus::Closed {
                Some(k.clone())
            } else {
                None
            }
        });
        match key {
            Some(k) => {
                let r = runs.get_mut(&k).unwrap();
                r.status = RunStatus::Closed;
                r.abort_handle.take()
            }
            None => {
                return error_response(
                    StatusCode::NOT_FOUND,
                    "session_not_found",
                    "Session not found",
                );
            }
        }
    };

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

// ── messages handler ─────────────────────────────────────────────────────────

async fn messages_handler(
    State(state): State<AppState>,
    Json(body): Json<SendMessageRequest>,
) -> Response {
    // 1. Body validation
    if body.agent.trim().is_empty() {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            "agent field is required",
        );
    }
    if body.content.is_empty() {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            "content must not be empty",
        );
    }

    // 2. Agent must be in the runnable list (and currently available).
    let agent = body.agent.clone();
    let runnable = build_runnable_agents();
    if !runnable.iter().any(|a| a.key == agent) {
        return error_response(
            StatusCode::NOT_FOUND,
            "agent_not_found",
            &format!(
                "Agent '{}' is not available. Use GET /v1/agents to see available agents.",
                agent
            ),
        );
    }

    // 3. Resume path: check that the session isn't already mid-run.
    //    For aikit, attempt a pre-flight disk lookup so we can return 404
    //    before opening the SSE stream when the id is bogus. If the session
    //    is unknown to the on-disk store but is currently tracked in memory
    //    (e.g. it was created earlier this process and the on-disk store is
    //    elsewhere — common in tests), allow it through and let the SDK
    //    decide. Other backends always go straight to the SDK; their
    //    SessionNotFound error is surfaced via the SSE error frame.
    if let Some(ref sid) = body.session_id {
        let in_memory = {
            let runs = state.runs.lock().unwrap();
            let busy = runs
                .values()
                .any(|r| r.session_id.as_deref() == Some(sid) && r.status == RunStatus::Running);
            if busy {
                return error_response(
                    StatusCode::CONFLICT,
                    "session_busy",
                    "Session is currently processing a message",
                );
            }
            runs.values().any(|r| r.session_id.as_deref() == Some(sid))
        };

        if agent == "aikit" && !in_memory {
            let store = SessionStore::open();
            match store.load(sid) {
                Ok(_) => {}
                Err(SessionStoreError::NotFound(id)) => {
                    return error_response(
                        StatusCode::NOT_FOUND,
                        "session_not_found",
                        &format!("Session '{}' not found", id),
                    );
                }
                Err(e) => {
                    tracing::warn!("session store error: {:?}", e);
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "internal_error",
                        "Failed to load session",
                    );
                }
            }
        }
    }

    // 4. Capacity check: active runs must be < max_sessions.
    {
        let runs = state.runs.lock().unwrap();
        let active = runs
            .values()
            .filter(|r| r.status == RunStatus::Running)
            .count();
        if active >= state.config.max_sessions {
            return error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "session_limit_reached",
                &format!(
                    "Maximum of {} concurrent sessions reached",
                    state.config.max_sessions
                ),
            );
        }
    }

    // 5. Register run.
    let request_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    {
        let mut runs = state.runs.lock().unwrap();
        runs.insert(
            request_id.clone(),
            RunRecord {
                session_id: body.session_id.clone(),
                agent: agent.clone(),
                status: RunStatus::Running,
                started_at: now,
                last_active_at: now,
                abort_handle: None,
            },
        );
    }

    // 6. Build options and spawn the run.
    let session_id_in = body.session_id.clone();
    let content = body.content.clone();
    let model = body.model.clone();
    let yolo = body.yolo;
    let timeout = Duration::from_secs(state.config.run_timeout_secs);
    let run_fn = state.run_fn.clone();
    let runs_ref = Arc::clone(&state.runs);

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);
    let tx_clone = tx.clone();
    let request_id_clone = request_id.clone();

    let task = tokio::task::spawn(async move {
        let mut options = RunOptions::new()
            .with_yolo(yolo)
            .with_stream(true)
            .with_timeout(timeout);
        if let Some(ref sid) = session_id_in {
            options = options.with_session_id(sid);
        }
        if let Some(m) = model {
            options = options.with_model(m);
        }

        let tx_inner = tx_clone.clone();
        let agent_for_run = agent.clone();
        let blocking_handle =
            tokio::task::spawn_blocking(move || run_fn(agent_for_run, content, options, tx_inner));

        let outcome: Result<RunFnOutcome, RunError> = tokio::select! {
            result = blocking_handle => {
                match result {
                    Ok(r) => r,
                    Err(join_err) => {
                        if !join_err.is_cancelled() {
                            tracing::error!(
                                "messages_handler: spawn_blocking join error: {}",
                                join_err
                            );
                        }
                        // Treat task cancellation as silent close.
                        let mut runs = runs_ref.lock().unwrap();
                        if let Some(r) = runs.get_mut(&request_id_clone) {
                            r.status = RunStatus::Idle;
                            r.last_active_at = Utc::now();
                            r.abort_handle = None;
                        }
                        return;
                    }
                }
            }
            _ = tx_clone.closed() => {
                // Client disconnected. Mark idle; the blocking thread may
                // continue briefly but its sends silently fail.
                let mut runs = runs_ref.lock().unwrap();
                if let Some(r) = runs.get_mut(&request_id_clone) {
                    r.status = RunStatus::Idle;
                    r.last_active_at = Utc::now();
                    r.abort_handle = None;
                }
                return;
            }
        };

        // Normal completion: emit done (or error) frame, update record.
        let exit_code = match outcome {
            Ok(out) => {
                // Backfill session_id if we learned it from the SDK.
                if out.session_id.is_some() {
                    let mut runs = runs_ref.lock().unwrap();
                    if let Some(r) = runs.get_mut(&request_id_clone) {
                        if r.session_id.is_none() {
                            r.session_id = out.session_id.clone();
                        }
                    }
                }
                out.exit_code
            }
            Err(RunError::TimedOut { .. }) => {
                let err = serde_json::to_string(&SseErrorPayload {
                    code: "run_timeout".to_string(),
                    message: "Run exceeded timeout".to_string(),
                })
                .unwrap_or_default();
                let _ = tx_clone
                    .send(Ok(Event::default().event("error").data(err)))
                    .await;
                1
            }
            Err(RunError::SessionNotFound(id)) => {
                // Shouldn't happen — we pre-flight aikit sessions — but other
                // backends can still bubble this up.
                let err = serde_json::to_string(&SseErrorPayload {
                    code: "session_not_found".to_string(),
                    message: format!("Session '{}' not found", id),
                })
                .unwrap_or_default();
                let _ = tx_clone
                    .send(Ok(Event::default().event("error").data(err)))
                    .await;
                1
            }
            Err(RunError::AgentNotRunnable(msg)) => {
                let err = serde_json::to_string(&SseErrorPayload {
                    code: "agent_not_runnable".to_string(),
                    message: msg,
                })
                .unwrap_or_default();
                let _ = tx_clone
                    .send(Ok(Event::default().event("error").data(err)))
                    .await;
                1
            }
            Err(e) => {
                tracing::error!("messages_handler: run error: {}", e);
                let err = serde_json::to_string(&SseErrorPayload {
                    code: "internal_error".to_string(),
                    message: "Internal error".to_string(),
                })
                .unwrap_or_default();
                let _ = tx_clone
                    .send(Ok(Event::default().event("error").data(err)))
                    .await;
                1
            }
        };

        {
            let mut runs = runs_ref.lock().unwrap();
            if let Some(r) = runs.get_mut(&request_id_clone) {
                r.status = RunStatus::Idle;
                r.last_active_at = Utc::now();
                r.abort_handle = None;
            }
        }

        let done = serde_json::to_string(&SseDonePayload { exit_code }).unwrap_or_default();
        let _ = tx_clone
            .send(Ok(Event::default().event("done").data(done)))
            .await;
    });

    {
        let mut runs = state.runs.lock().unwrap();
        if let Some(r) = runs.get_mut(&request_id) {
            r.abort_handle = Some(task.abort_handle());
        }
    }

    drop(task);

    let stream = ReceiverStream::new(rx);

    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    );
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));

    (headers, Sse::new(stream)).into_response()
}

// ── auth middleware ───────────────────────────────────────────────────────────

async fn auth_middleware(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let expected_key = match &state.config.api_key {
        Some(k) => k.clone(),
        None => return next.run(request).await,
    };

    let auth_header = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let token = auth_header.and_then(|h| h.strip_prefix("Bearer "));

    if token.map(|t| t == expected_key).unwrap_or(false) {
        next.run(request).await
    } else {
        error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Invalid or missing API key",
        )
    }
}

// ── fallback handler ──────────────────────────────────────────────────────────

async fn not_found_handler() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", "Not Found")
}

// ── router ────────────────────────────────────────────────────────────────────

fn build_router(state: AppState) -> Router {
    let router = Router::new()
        .route("/health", get(health_handler))
        .route("/v1/agents", get(agents_handler))
        .route("/v1/messages", post(messages_handler))
        .route("/v1/sessions", get(list_runs_handler))
        .route("/v1/sessions/{session_id}", get(get_run_handler))
        .route("/v1/sessions/{session_id}", delete(delete_run_handler))
        .fallback(not_found_handler)
        .with_state(state.clone());

    if state.config.api_key.is_some() {
        router.layer(middleware::from_fn_with_state(state, auth_middleware))
    } else {
        router
    }
}

// ── production run_fn ─────────────────────────────────────────────────────────

/// Build the production run_fn that calls run_agent_events and forwards events
/// to the SSE channel. Captures the SDK-assigned session_id from
/// `AgentEventPayload::SessionStarted` and:
///   1. emits it as the first `event: session` SSE frame, and
///   2. returns it via `RunFnOutcome.session_id` so the server can backfill
///      its run record.
pub fn make_production_run_fn() -> RunFn {
    Arc::new(
        move |agent: String,
              prompt: String,
              options: RunOptions,
              tx: tokio::sync::mpsc::Sender<Result<Event, Infallible>>| {
            use aikit_sdk::run_agent_events;

            let captured_session_id = Arc::new(Mutex::new(None::<String>));
            let captured_for_cb = Arc::clone(&captured_session_id);
            let tx_cb = tx.clone();

            let result = run_agent_events(&agent, &prompt, options, move |event| {
                if let AgentEventPayload::SessionStarted { session_id } = &event.payload {
                    *captured_for_cb.lock().unwrap() = Some(session_id.clone());
                    let payload = serde_json::to_string(&SseSessionPayload {
                        session_id: session_id.clone(),
                    })
                    .unwrap_or_default();
                    let _ =
                        tx_cb.blocking_send(Ok(Event::default().event("session").data(payload)));
                    return;
                }
                if let Some(ev) = agent_event_to_sse(&event) {
                    let _ = tx_cb.blocking_send(Ok(ev));
                }
            });

            let session_id = captured_session_id.lock().unwrap().clone();

            result.map(|r| RunFnOutcome {
                exit_code: r.exit_code().unwrap_or(0),
                session_id,
            })
        },
    )
}

/// Map an AgentEvent to an SSE Event, returning None if the event should be suppressed.
fn agent_event_to_sse(event: &aikit_sdk::AgentEvent) -> Option<Event> {
    match &event.payload {
        AgentEventPayload::SessionStarted { .. } => None, // handled separately
        AgentEventPayload::StreamMessage(msg) => match (msg.role, msg.kind, msg.phase) {
            (MessageRole::Assistant, MessageKind::Message, MessagePhase::Delta)
            | (MessageRole::Assistant, MessageKind::Message, MessagePhase::Final) => {
                let data = serde_json::json!({ "content": msg.text }).to_string();
                Some(Event::default().event("text").data(data))
            }
            (MessageRole::Tool, MessageKind::ToolOutput, _) => {
                let data = serde_json::json!({
                    "name": event.agent_key,
                    "output": msg.text
                })
                .to_string();
                Some(Event::default().event("tool_result").data(data))
            }
            _ => None,
        },
        AgentEventPayload::QuotaExceeded { info, .. } => {
            let data = serde_json::json!({
                "code": "quota_exceeded",
                "message": info.raw_message
            })
            .to_string();
            Some(Event::default().event("error").data(data))
        }
        AgentEventPayload::AikitTextDelta { content, .. } => {
            let data = serde_json::json!({ "content": content }).to_string();
            Some(Event::default().event("text").data(data))
        }
        AgentEventPayload::AikitTextFinal { content, .. } => {
            let data = serde_json::json!({ "content": content }).to_string();
            Some(Event::default().event("text").data(data))
        }
        AgentEventPayload::AikitToolUse {
            tool_name,
            tool_input,
            ..
        } => {
            let data = serde_json::json!({
                "name": tool_name,
                "input": tool_input
            })
            .to_string();
            Some(Event::default().event("tool_use").data(data))
        }
        _ => None,
    }
}

// ── entry point ───────────────────────────────────────────────────────────────

/// Start the HTTP server. This function does not return until the server shuts down.
pub async fn execute(args: ServeArgs) -> anyhow::Result<()> {
    execute_with_run_fn(args, make_production_run_fn()).await
}

/// Start the HTTP server with an injectable run function (used in tests).
pub async fn execute_with_run_fn(args: ServeArgs, run_fn: RunFn) -> anyhow::Result<()> {
    let config = ServeConfig {
        host: args.host.clone(),
        port: args.port,
        run_timeout_secs: args.run_timeout_secs,
        max_sessions: args.max_sessions,
        api_key: args.api_key,
    };

    let state = AppState {
        runs: Arc::new(Mutex::new(HashMap::new())),
        config: config.clone(),
        run_fn,
    };

    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid address {}:{}: {}", config.host, config.port, e))?;

    let listener = tokio::net::TcpListener::bind(&addr).await.map_err(|_e| {
        anyhow::anyhow!(
            "error: address already in use: {}:{}",
            config.host,
            config.port
        )
    })?;

    let bound_addr = listener.local_addr()?;

    eprintln!("Listening on http://{}", bound_addr);

    let is_loopback = addr.ip().is_loopback();
    if !is_loopback {
        eprintln!(
            "Warning: server is bound to a non-loopback address. Set --api-key or restrict access via network ACLs."
        );
    }

    let router = build_router(state.clone());

    let shutdown_signal = async {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm =
                signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
            tokio::select! {
                _ = ctrl_c => {},
                _ = sigterm.recv() => {},
            }
        }
        #[cfg(not(unix))]
        {
            let _ = ctrl_c.await;
        }
        tracing::info!("Shutdown signal received; draining in-flight sessions...");
    };

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal)
        .await
        .map_err(|e| anyhow::anyhow!("server error: {}", e))?;

    {
        let mut runs = state.runs.lock().unwrap();
        for r in runs.values_mut() {
            if let Some(handle) = r.abort_handle.take() {
                handle.abort();
            }
        }
    }

    tokio::time::sleep(Duration::from_secs(5)).await;

    Ok(())
}

// ── test stub helpers ─────────────────────────────────────────────────────────

/// Build a stub run_fn that emits the given SSE events and returns success.
/// If `session_id_for_new` is `Some`, a `session` SSE frame is emitted first
/// when the incoming RunOptions has no `session_id`, simulating SDK-assigned
/// session creation.
#[allow(dead_code)]
pub fn make_stub_run_fn(events: Vec<(&'static str, &'static str)>) -> RunFn {
    make_stub_run_fn_with_session(events, None)
}

/// Stub variant that simulates a fresh-session run: when the incoming options
/// have no `session_id`, the stub emits `event: session` with the supplied id
/// and reports it back via `RunFnOutcome.session_id`. When the options DO
/// carry an id (resume), the stub echoes it instead.
#[allow(dead_code)]
pub fn make_stub_run_fn_with_session(
    events: Vec<(&'static str, &'static str)>,
    session_id_for_new: Option<String>,
) -> RunFn {
    Arc::new(move |_agent, _prompt, options, tx| {
        // Resolve the session id we will report.
        let sid: Option<String> = options
            .session_id
            .clone()
            .or_else(|| session_id_for_new.clone());

        if let Some(ref id) = sid {
            let payload = serde_json::json!({ "session_id": id }).to_string();
            let _ = tx.blocking_send(Ok(Event::default().event("session").data(payload)));
        }

        for (name, data) in &events {
            let ev = Event::default().event(*name).data(*data);
            let _ = tx.blocking_send(Ok(ev));
        }

        Ok(RunFnOutcome {
            exit_code: 0,
            session_id: sid,
        })
    })
}

/// Build a stub run_fn that sleeps for `duration` before returning success.
/// Use this to test concurrent-request rejection and disconnect cleanup.
#[allow(dead_code)]
pub fn make_blocking_stub_run_fn(duration: Duration) -> RunFn {
    Arc::new(move |_agent, _prompt, options, tx| {
        // Echo the session id if one was provided so resume tests can verify.
        if let Some(ref id) = options.session_id {
            let payload = serde_json::json!({ "session_id": id }).to_string();
            let _ = tx.blocking_send(Ok(Event::default().event("session").data(payload)));
        }
        std::thread::sleep(duration);
        Ok(RunFnOutcome {
            exit_code: 0,
            session_id: options.session_id.clone(),
        })
    })
}

/// Build a stub run_fn that returns a `RunError::TimedOut` error.
#[allow(dead_code)]
pub fn make_timeout_stub_run_fn() -> RunFn {
    Arc::new(|_agent, _prompt, _options, _tx| {
        Err(aikit_sdk::RunError::TimedOut {
            timeout: Duration::from_secs(1),
            stdout: vec![],
            stderr: vec![],
        })
    })
}
