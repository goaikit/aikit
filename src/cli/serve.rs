//! `aikit serve` — HTTP server for multi-turn agent sessions.
//!
//! Two response shapes share one endpoint, `POST /v1/messages`, selected by
//! the request's `Accept` header (HTTP content negotiation):
//! - `Accept: text/event-stream` → SSE with `event: session`, `event: text`,
//!   `event: tool_use`, `event: tool_result`, `event: error`, then
//!   `event: done`.
//! - `Accept: application/json` → server runs to completion, accumulates the
//!   assistant text frames, and returns a single JSON body
//!   `{session_id, content, exit_code, error?}`.
//! - `Accept: */*`, missing, or both types present → SSE (default).
//! - Any other explicit media type → `406 Not Acceptable`.
//!
//! Session model:
//! - Sessions are created **implicitly** on the first `POST /v1/messages`
//!   call that omits `session_id`.
//! - In SSE mode the first frame is `event: session` carrying the new id; in
//!   the JSON shape the id appears in the response body. Subsequent calls
//!   quote that id in the request body to resume.
//! - For the `aikit` backend the id is assigned by the SDK (and persisted to
//!   `~/.aikit/sessions/...`). For other backends the id is treated as an
//!   opaque token forwarded to the underlying CLI's `--resume` flag.

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
    session_id: Option<String>,
    agent: String,
    status: RunStatus,
    started_at: DateTime<Utc>,
    last_active_at: DateTime<Utc>,
    abort_handle: Option<tokio::task::AbortHandle>,
    /// Tail of the last run's stderr (set after the run completes). Used to
    /// enrich the sync JSON response when the agent printed nothing on
    /// stdout but exited.
    stderr_tail: String,
    /// Last completed exit code, if any.
    last_exit_code: Option<i32>,
}

// ── typed event channel ───────────────────────────────────────────────────────

/// One structured event from the agent run. The handler converts these to SSE
/// for stream mode and accumulates them into a single JSON body for sync mode.
#[derive(Debug, Clone)]
pub enum StreamFrame {
    Session {
        session_id: String,
    },
    Text {
        content: String,
    },
    ToolUse {
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        name: String,
        output: String,
    },
    Error {
        code: String,
        message: String,
    },
}

// ── run function type (injectable for tests) ──────────────────────────────────

pub struct RunFnOutcome {
    pub exit_code: i32,
    /// Session id assigned by the SDK. `None` when the backend doesn't expose
    /// one (e.g. non-aikit agents on a fresh run).
    pub session_id: Option<String>,
    /// Captured stderr from the agent process (truncated to the last
    /// `MAX_STDERR_TAIL_BYTES`). Empty when nothing was captured. Used to
    /// surface a useful error in sync responses when `content` is empty.
    pub stderr_tail: String,
}

/// Maximum bytes of stderr to surface in the JSON sync response. Larger
/// agent stderr is truncated from the front (keep the tail).
const MAX_STDERR_TAIL_BYTES: usize = 2048;

/// Type alias for the agent-run function injected into AppState.
///
/// Contract:
/// - Emit `StreamFrame` values via `tx` as the run progresses. For a fresh
///   session, the very first frame should be `StreamFrame::Session`.
/// - Return the final exit code and (when known) the session_id.
pub type RunFn = Arc<
    dyn Fn(
            String,
            String,
            RunOptions,
            tokio::sync::mpsc::Sender<StreamFrame>,
        ) -> Result<RunFnOutcome, RunError>
        + Send
        + Sync,
>;

// ── app state ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
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

/// Response representation negotiated from the request's `Accept` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseMode {
    Sse,
    Sync,
    NotAcceptable,
}

/// Resolve the response representation from an `Accept` header.
///
/// - Substring match, case-insensitive.
/// - Missing/empty header, `*/*`, or both types present → SSE (default).
/// - `text/event-stream` only → SSE.
/// - `application/json` only → Sync.
/// - Any other explicit media type → NotAcceptable (406).
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
struct SyncMessageResponse {
    session_id: Option<String>,
    content: String,
    exit_code: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorDetail>,
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

// ── frame → SSE ─────────────────────────────────────────────────────────────

fn frame_to_sse(frame: &StreamFrame) -> Event {
    match frame {
        StreamFrame::Session { session_id } => {
            let data = serde_json::json!({ "session_id": session_id }).to_string();
            Event::default().event("session").data(data)
        }
        StreamFrame::Text { content } => {
            let data = serde_json::json!({ "content": content }).to_string();
            Event::default().event("text").data(data)
        }
        StreamFrame::ToolUse { name, input } => {
            let data = serde_json::json!({ "name": name, "input": input }).to_string();
            Event::default().event("tool_use").data(data)
        }
        StreamFrame::ToolResult { name, output } => {
            let data = serde_json::json!({ "name": name, "output": output }).to_string();
            Event::default().event("tool_result").data(data)
        }
        StreamFrame::Error { code, message } => {
            let data = serde_json::json!({ "code": code, "message": message }).to_string();
            Event::default().event("error").data(data)
        }
    }
}

// ── handlers ─────────────────────────────────────────────────────────────────

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

/// Validation common to stream and sync modes. Returns Some(error_response) if
/// the request can't proceed.
fn validate_request(body: &SendMessageRequest, state: &AppState) -> Option<Response> {
    if body.agent.trim().is_empty() {
        return Some(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            "agent field is required",
        ));
    }
    if body.content.is_empty() {
        return Some(error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            "content must not be empty",
        ));
    }

    let runnable = build_runnable_agents();
    if !runnable.iter().any(|a| a.key == body.agent) {
        return Some(error_response(
            StatusCode::NOT_FOUND,
            "agent_not_found",
            &format!(
                "Agent '{}' is not available. Use GET /v1/agents to see available agents.",
                body.agent
            ),
        ));
    }

    if let Some(ref sid) = body.session_id {
        let in_memory = {
            let runs = state.runs.lock().unwrap();
            let busy = runs
                .values()
                .any(|r| r.session_id.as_deref() == Some(sid) && r.status == RunStatus::Running);
            if busy {
                return Some(error_response(
                    StatusCode::CONFLICT,
                    "session_busy",
                    "Session is currently processing a message",
                ));
            }
            runs.values().any(|r| r.session_id.as_deref() == Some(sid))
        };

        if body.agent == "aikit" && !in_memory {
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
        }
    }

    let runs = state.runs.lock().unwrap();
    let active = runs
        .values()
        .filter(|r| r.status == RunStatus::Running)
        .count();
    if active >= state.config.max_sessions {
        return Some(error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "session_limit_reached",
            &format!(
                "Maximum of {} concurrent sessions reached",
                state.config.max_sessions
            ),
        ));
    }

    None
}

/// Map a RunError to a StreamFrame::Error. Returns the exit code that should be
/// reported alongside it.
fn run_error_to_frame(err: RunError) -> (StreamFrame, i32) {
    let frame = match err {
        RunError::TimedOut { .. } => StreamFrame::Error {
            code: "run_timeout".to_string(),
            message: "Run exceeded timeout".to_string(),
        },
        RunError::SessionNotFound(id) => StreamFrame::Error {
            code: "session_not_found".to_string(),
            message: format!("Session '{}' not found", id),
        },
        RunError::AgentNotRunnable(msg) => StreamFrame::Error {
            code: "agent_not_runnable".to_string(),
            message: msg,
        },
        e => {
            tracing::error!("run error: {}", e);
            StreamFrame::Error {
                code: "internal_error".to_string(),
                message: "Internal error".to_string(),
            }
        }
    };
    (frame, 1)
}

/// Drive the run. Returns the channel receiver, the spawned task handle, and
/// the in-memory request id assigned to this run.
fn spawn_run(
    state: &AppState,
    body: &SendMessageRequest,
) -> (tokio::sync::mpsc::Receiver<StreamFrame>, String) {
    let request_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    {
        let mut runs = state.runs.lock().unwrap();
        runs.insert(
            request_id.clone(),
            RunRecord {
                session_id: body.session_id.clone(),
                agent: body.agent.clone(),
                status: RunStatus::Running,
                started_at: now,
                last_active_at: now,
                abort_handle: None,
                stderr_tail: String::new(),
                last_exit_code: None,
            },
        );
    }

    let session_id_in = body.session_id.clone();
    let content = body.content.clone();
    let model = body.model.clone();
    let yolo = body.yolo;
    let agent = body.agent.clone();
    let timeout = Duration::from_secs(state.config.run_timeout_secs);
    let run_fn = state.run_fn.clone();
    let runs_ref = Arc::clone(&state.runs);

    let (tx, rx) = tokio::sync::mpsc::channel::<StreamFrame>(64);
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

        let tx_inner = tx.clone();
        let blocking_handle =
            tokio::task::spawn_blocking(move || run_fn(agent, content, options, tx_inner));

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
            _ = tx.closed() => {
                // Client disconnected.
                let mut runs = runs_ref.lock().unwrap();
                if let Some(r) = runs.get_mut(&request_id_clone) {
                    r.status = RunStatus::Idle;
                    r.last_active_at = Utc::now();
                    r.abort_handle = None;
                }
                return;
            }
        };

        let _exit_code = match outcome {
            Ok(out) => {
                if out.session_id.is_some() {
                    let mut runs = runs_ref.lock().unwrap();
                    if let Some(r) = runs.get_mut(&request_id_clone) {
                        if r.session_id.is_none() {
                            r.session_id = out.session_id.clone();
                        }
                        r.stderr_tail = out.stderr_tail.clone();
                        r.last_exit_code = Some(out.exit_code);
                    }
                } else {
                    let mut runs = runs_ref.lock().unwrap();
                    if let Some(r) = runs.get_mut(&request_id_clone) {
                        r.stderr_tail = out.stderr_tail.clone();
                        r.last_exit_code = Some(out.exit_code);
                    }
                }
                out.exit_code
            }
            Err(err) => {
                let (frame, code) = run_error_to_frame(err);
                let _ = tx.send(frame).await;
                code
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

        // tx drops here, closing the channel and signalling completion.
        drop(tx);
        let _ = _exit_code;
    });

    {
        let mut runs = state.runs.lock().unwrap();
        if let Some(r) = runs.get_mut(&request_id) {
            r.abort_handle = Some(task.abort_handle());
        }
    }

    drop(task);

    (rx, request_id)
}

async fn messages_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<SendMessageRequest>,
) -> Response {
    let mode = resolve_response_mode(&headers);
    if mode == ResponseMode::NotAcceptable {
        return error_response(
            StatusCode::NOT_ACCEPTABLE,
            "not_acceptable",
            "Accept must include text/event-stream or application/json",
        );
    }

    if let Some(err) = validate_request(&body, &state) {
        return err;
    }

    let (rx, request_id) = spawn_run(&state, &body);

    match mode {
        ResponseMode::Sse => sse_response(rx, Arc::clone(&state.runs), request_id),
        ResponseMode::Sync => sync_response(rx, Arc::clone(&state.runs), request_id).await,
        ResponseMode::NotAcceptable => unreachable!(),
    }
}

/// Convert a receiver of StreamFrame into an SSE response. A terminal
/// `event: done` frame is appended once the run channel closes. A forwarder
/// task pumps frames into a second channel so we can append the terminator
/// without depending on stream combinators outside `tokio_stream`.
fn sse_response(
    mut rx: tokio::sync::mpsc::Receiver<StreamFrame>,
    _runs: Arc<Mutex<HashMap<String, RunRecord>>>,
    _request_id: String,
) -> Response {
    let (out_tx, out_rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);

    tokio::spawn(async move {
        let mut saw_error = false;
        loop {
            tokio::select! {
                maybe_frame = rx.recv() => {
                    match maybe_frame {
                        Some(frame) => {
                            if matches!(frame, StreamFrame::Error { .. }) {
                                saw_error = true;
                            }
                            if out_tx.send(Ok(frame_to_sse(&frame))).await.is_err() {
                                return;
                            }
                        }
                        None => break,
                    }
                }
                _ = out_tx.closed() => {
                    // Client disconnected; drop rx (via `return`) so the run
                    // task notices via its own `tx.closed()` select branch.
                    return;
                }
            }
        }
        let exit_code = if saw_error { 1 } else { 0 };
        let data = serde_json::json!({ "exit_code": exit_code }).to_string();
        let _ = out_tx
            .send(Ok(Event::default().event("done").data(data)))
            .await;
    });

    let stream = ReceiverStream::new(out_rx);

    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    );
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));

    (headers, Sse::new(stream)).into_response()
}

/// Drain the receiver to completion and return a single JSON body. Text
/// frames are concatenated; the first session id and the first error are kept.
async fn sync_response(
    mut rx: tokio::sync::mpsc::Receiver<StreamFrame>,
    runs: Arc<Mutex<HashMap<String, RunRecord>>>,
    request_id: String,
) -> Response {
    let mut session_id: Option<String> = None;
    let mut content = String::new();
    let mut error: Option<ErrorDetail> = None;

    while let Some(frame) = rx.recv().await {
        match frame {
            StreamFrame::Session { session_id: id } => {
                if session_id.is_none() {
                    session_id = Some(id);
                }
            }
            StreamFrame::Text { content: c } => {
                content.push_str(&c);
            }
            StreamFrame::Error { code, message } => {
                if error.is_none() {
                    error = Some(ErrorDetail { code, message });
                }
            }
            // Tool frames are intentionally dropped in sync mode — clients
            // that need tool visibility should use stream mode.
            StreamFrame::ToolUse { .. } | StreamFrame::ToolResult { .. } => {}
        }
    }

    // Backfill session id, stderr, and exit code from the in-memory record
    // (the run_fn outcome may have populated them even if no frames were
    // emitted to the channel).
    let (record_stderr, record_exit) = {
        let runs_guard = runs.lock().unwrap();
        if let Some(r) = runs_guard.get(&request_id) {
            if session_id.is_none() {
                session_id.clone_from(&r.session_id);
            }
            (r.stderr_tail.clone(), r.last_exit_code)
        } else {
            (String::new(), None)
        }
    };

    // Determine exit code & error:
    //   - If an explicit Error frame arrived → exit_code 1 with that error.
    //   - Else use the recorded exit code (0 if missing).
    //   - If the run exited non-zero with NO mapped content, synthesize an
    //     `agent_error` error containing the stderr tail so the client gets
    //     a useful diagnosis instead of `content:""` and `exit_code:0`.
    let (exit_code, error) = if let Some(e) = error {
        (1, Some(e))
    } else {
        let code = record_exit.unwrap_or(0);
        if code != 0 && content.is_empty() {
            let message = if record_stderr.is_empty() {
                format!("Agent exited with code {}", code)
            } else {
                record_stderr.clone()
            };
            (
                code,
                Some(ErrorDetail {
                    code: "agent_error".to_string(),
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
    };
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&resp).unwrap_or_default(),
    )
        .into_response()
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

async fn not_found_handler() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", "Not Found")
}

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

/// Build the production run_fn that calls `run_agent_events` and translates
/// SDK events into `StreamFrame` values. Captures the SDK-assigned session_id
/// both as a frame and in `RunFnOutcome.session_id`.
pub fn make_production_run_fn() -> RunFn {
    Arc::new(
        move |agent: String,
              prompt: String,
              options: RunOptions,
              tx: tokio::sync::mpsc::Sender<StreamFrame>| {
            use aikit_sdk::run_agent_events;

            tracing::info!(
                target: "aikit::serve::run",
                agent = %agent,
                prompt_len = prompt.len(),
                session_id = ?options.session_id,
                model = ?options.model,
                yolo = options.yolo,
                "spawning agent run"
            );

            let captured_session_id = Arc::new(Mutex::new(None::<String>));
            let captured_for_cb = Arc::clone(&captured_session_id);
            let tx_cb = tx.clone();
            // Per-run state: if the backend emits any StreamMessage with
            // phase=Delta, we suppress a later StreamMessage with phase=Final
            // (it's the concatenation of the deltas; emitting both would
            // double the content in sync mode). Backends that only emit
            // Final (e.g. claude in `-p` mode) still get it through.
            let saw_assistant_delta = Arc::new(Mutex::new(false));
            let saw_delta_for_cb = Arc::clone(&saw_assistant_delta);

            let agent_for_cb = agent.clone();
            let result = run_agent_events(&agent, &prompt, options, move |event| {
                let payload_kind = payload_kind_name(&event.payload);
                // Dedup Final-after-Delta for non-aikit assistant messages.
                if let AgentEventPayload::StreamMessage(msg) = &event.payload {
                    if msg.role == MessageRole::Assistant && msg.kind == MessageKind::Message {
                        if msg.phase == MessagePhase::Delta {
                            *saw_delta_for_cb.lock().unwrap() = true;
                        } else if msg.phase == MessagePhase::Final
                            && *saw_delta_for_cb.lock().unwrap()
                        {
                            tracing::trace!(
                                target: "aikit::serve::run",
                                agent = %agent_for_cb,
                                payload = payload_kind,
                                "suppressing Final assistant StreamMessage (deltas already emitted)"
                            );
                            return;
                        }
                    }
                }
                match agent_event_to_frame(&event, &agent_for_cb) {
                    Some(frame) => {
                        let frame_name = frame_kind_name(&frame);
                        tracing::debug!(
                            target: "aikit::serve::run",
                            agent = %agent_for_cb,
                            payload = payload_kind,
                            frame = frame_name,
                            "mapped SDK event to frame"
                        );
                        if let StreamFrame::Session { ref session_id } = frame {
                            *captured_for_cb.lock().unwrap() = Some(session_id.clone());
                        }
                        if let Err(e) = tx_cb.blocking_send(frame) {
                            tracing::warn!(
                                target: "aikit::serve::run",
                                agent = %agent_for_cb,
                                payload = payload_kind,
                                error = %e,
                                "frame channel send failed (client likely disconnected)"
                            );
                        }
                    }
                    None => {
                        tracing::trace!(
                            target: "aikit::serve::run",
                            agent = %agent_for_cb,
                            payload = payload_kind,
                            "SDK event suppressed (no matching frame)"
                        );
                    }
                }
            });

            let session_id = captured_session_id.lock().unwrap().clone();

            match result {
                Ok(r) => {
                    let exit_code = r.exit_code().unwrap_or(0);
                    let stderr_tail = stderr_tail(&r.stderr);
                    tracing::info!(
                        target: "aikit::serve::run",
                        agent = %agent,
                        exit_code,
                        session_id = ?session_id,
                        stderr_bytes = r.stderr.len(),
                        "agent run completed"
                    );
                    if exit_code != 0 && !stderr_tail.is_empty() {
                        tracing::warn!(
                            target: "aikit::serve::run",
                            agent = %agent,
                            exit_code,
                            stderr_tail = %stderr_tail,
                            "agent exited non-zero"
                        );
                    }
                    Ok(RunFnOutcome {
                        exit_code,
                        session_id,
                        stderr_tail,
                    })
                }
                Err(e) => {
                    tracing::error!(
                        target: "aikit::serve::run",
                        agent = %agent,
                        error = %e,
                        "agent run failed"
                    );
                    Err(e)
                }
            }
        },
    )
}

/// Short name for an `AgentEventPayload` variant, for log fields.
fn payload_kind_name(p: &AgentEventPayload) -> &'static str {
    match p {
        AgentEventPayload::JsonLine(_) => "json_line",
        AgentEventPayload::RawLine(_) => "raw_line",
        AgentEventPayload::RawBytes(_) => "raw_bytes",
        AgentEventPayload::StreamMessage(_) => "stream_message",
        AgentEventPayload::TokenUsageLine { .. } => "token_usage_line",
        AgentEventPayload::QuotaExceeded { .. } => "quota_exceeded",
        AgentEventPayload::RawTransportLine { .. } => "raw_transport_line",
        AgentEventPayload::AikitTextDelta { .. } => "aikit_text_delta",
        AgentEventPayload::AikitTextFinal { .. } => "aikit_text_final",
        AgentEventPayload::AikitToolUse { .. } => "aikit_tool_use",
        AgentEventPayload::AikitToolResult { .. } => "aikit_tool_result",
        AgentEventPayload::AikitSubagentSpawn { .. } => "aikit_subagent_spawn",
        AgentEventPayload::AikitSubagentResult { .. } => "aikit_subagent_result",
        AgentEventPayload::AikitContextCompressed { .. } => "aikit_context_compressed",
        AgentEventPayload::AikitStepFinish { .. } => "aikit_step_finish",
        AgentEventPayload::SessionStarted { .. } => "session_started",
        _ => "other",
    }
}

fn frame_kind_name(f: &StreamFrame) -> &'static str {
    match f {
        StreamFrame::Session { .. } => "session",
        StreamFrame::Text { .. } => "text",
        StreamFrame::ToolUse { .. } => "tool_use",
        StreamFrame::ToolResult { .. } => "tool_result",
        StreamFrame::Error { .. } => "error",
    }
}

/// Last `MAX_STDERR_TAIL_BYTES` bytes of `stderr` rendered as a lossy UTF-8
/// string. Returns empty when there's nothing useful.
fn stderr_tail(stderr: &[u8]) -> String {
    if stderr.is_empty() {
        return String::new();
    }
    let start = stderr.len().saturating_sub(MAX_STDERR_TAIL_BYTES);
    let slice = &stderr[start..];
    String::from_utf8_lossy(slice).trim().to_string()
}

/// Map an SDK `AgentEvent` to a typed `StreamFrame`, or None to suppress.
fn agent_event_to_frame(event: &aikit_sdk::AgentEvent, agent_key: &str) -> Option<StreamFrame> {
    match &event.payload {
        AgentEventPayload::SessionStarted { session_id } => Some(StreamFrame::Session {
            session_id: session_id.clone(),
        }),
        AgentEventPayload::StreamMessage(msg) => match (msg.role, msg.kind, msg.phase) {
            (MessageRole::Assistant, MessageKind::Message, MessagePhase::Delta)
            | (MessageRole::Assistant, MessageKind::Message, MessagePhase::Final) => {
                Some(StreamFrame::Text {
                    content: msg.text.clone(),
                })
            }
            (MessageRole::Tool, MessageKind::ToolOutput, _) => Some(StreamFrame::ToolResult {
                name: agent_key.to_string(),
                output: msg.text.clone(),
            }),
            _ => None,
        },
        AgentEventPayload::QuotaExceeded { info, .. } => Some(StreamFrame::Error {
            code: "quota_exceeded".to_string(),
            message: info.raw_message.clone(),
        }),
        // Emit deltas only. `AikitTextFinal` is the concatenation of all
        // preceding deltas, so emitting both would double the content in
        // sync mode. SSE clients still get the incremental streaming UX.
        AgentEventPayload::AikitTextDelta { content, .. } => Some(StreamFrame::Text {
            content: content.clone(),
        }),
        AgentEventPayload::AikitTextFinal { .. } => None,
        AgentEventPayload::AikitToolUse {
            tool_name,
            tool_input,
            ..
        } => Some(StreamFrame::ToolUse {
            name: tool_name.clone(),
            input: tool_input.clone(),
        }),
        _ => None,
    }
}

// ── entry point ───────────────────────────────────────────────────────────────

pub async fn execute(args: ServeArgs) -> anyhow::Result<()> {
    init_tracing();
    execute_with_run_fn(args, make_production_run_fn()).await
}

/// Install a tracing subscriber that honours `RUST_LOG` and defaults to
/// `info` for serve + SDK targets. No-op if a subscriber is already set
/// (e.g. when invoked from a binary that initialised tracing itself).
fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let default_filter =
        "aikit=info,aikit_cli=info,aikit::serve=info,aikit::serve::run=info,aikit_sdk=warn";
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .try_init();
}

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

/// Test stub that emits the given typed frames and returns success. If
/// `session_id_for_new` is `Some`, a `Session` frame is emitted first when the
/// incoming RunOptions has no `session_id` (simulating SDK session creation).
#[allow(dead_code)]
pub fn make_stub_run_fn_with_session(
    frames: Vec<StreamFrame>,
    session_id_for_new: Option<String>,
) -> RunFn {
    Arc::new(move |_agent, _prompt, options, tx| {
        let sid: Option<String> = options
            .session_id
            .clone()
            .or_else(|| session_id_for_new.clone());

        if let Some(ref id) = sid {
            let _ = tx.blocking_send(StreamFrame::Session {
                session_id: id.clone(),
            });
        }

        for frame in &frames {
            let _ = tx.blocking_send(frame.clone());
        }

        Ok(RunFnOutcome {
            exit_code: 0,
            session_id: sid,
            stderr_tail: String::new(),
        })
    })
}

/// Convenience: a stub that emits no frames.
#[allow(dead_code)]
pub fn make_stub_run_fn() -> RunFn {
    make_stub_run_fn_with_session(vec![], None)
}

/// Stub that sleeps for `duration` before returning success. Used to test
/// concurrent-request rejection and disconnect cleanup.
#[allow(dead_code)]
pub fn make_blocking_stub_run_fn(duration: Duration) -> RunFn {
    Arc::new(move |_agent, _prompt, options, tx| {
        if let Some(ref id) = options.session_id {
            let _ = tx.blocking_send(StreamFrame::Session {
                session_id: id.clone(),
            });
        }
        std::thread::sleep(duration);
        Ok(RunFnOutcome {
            exit_code: 0,
            session_id: options.session_id.clone(),
            stderr_tail: String::new(),
        })
    })
}

/// Stub that emits no frames, returns a non-zero exit code, and surfaces a
/// stderr tail. Used to verify the sync handler's empty-content fallback.
#[allow(dead_code)]
pub fn make_failing_stub_run_fn(exit_code: i32, stderr_tail: &'static str) -> RunFn {
    let tail = stderr_tail.to_string();
    Arc::new(move |_agent, _prompt, _options, _tx| {
        Ok(RunFnOutcome {
            exit_code,
            session_id: None,
            stderr_tail: tail.clone(),
        })
    })
}

/// Stub that immediately returns `RunError::TimedOut`.
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
