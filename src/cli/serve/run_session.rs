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
use aikit_sdk::{get_agent_status, RunError, RunOptions};

use crate::core::agent::get_agent_configs;

use super::{
    error_response, spawn_frame_forwarder, sse_response_with_headers, AppState, StreamFrame,
};

// ── run record ────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum RunStatus {
    Running,
    Idle,
    Closed,
}

#[derive(Clone)]
pub(super) struct RunRecord {
    pub session_id: Option<String>,
    pub agent: String,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
    pub abort_handle: Option<tokio::task::AbortHandle>,
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
/// `run_agent_events`; test stubs inject canned frames.
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
    let mut agents = build_runnable_agents();
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

pub(super) fn validate_request(body: &SendMessageRequest, state: &AppState) -> Option<Response> {
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
                "Agent '{}' is not available. Use GET /api/v1/agents to see available agents.",
                body.agent
            ),
        ));
    }
    if let Some(ref sid) = body.session_id {
        let in_memory = {
            let runs = state.runs.lock().unwrap();
            runs.values().any(|r| r.session_id.as_deref() == Some(sid))
        };
        if !in_memory {
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

// ── spawn_run ─────────────────────────────────────────────────────────────────

#[allow(clippy::result_large_err)]
pub(super) fn spawn_run(
    state: &AppState,
    body: &SendMessageRequest,
) -> Result<(tokio::sync::mpsc::Receiver<StreamFrame>, String), Response> {
    let request_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    {
        let mut runs = state.runs.lock().unwrap();
        // B9: atomic busy-check + capacity-check + insert under one lock.
        if let Some(ref sid) = body.session_id {
            let busy = runs.values().any(|r| {
                r.session_id.as_deref() == Some(sid.as_str()) && r.status == RunStatus::Running
            });
            if busy {
                return Err(error_response(
                    StatusCode::CONFLICT,
                    "session_busy",
                    "Session is currently processing a message",
                ));
            }
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

    // B11: relay channel so `Session` frames update the record before the run
    // completes, enabling concurrent requests to find the session_id early.
    let (inner_tx, mut inner_rx) = tokio::sync::mpsc::channel::<StreamFrame>(64);
    let (outer_tx, rx) = tokio::sync::mpsc::channel::<StreamFrame>(64);
    let relay_runs = Arc::clone(&runs_ref);
    let relay_request_id = request_id.clone();
    let relay_outer_tx = outer_tx.clone();
    tokio::spawn(async move {
        while let Some(frame) = inner_rx.recv().await {
            if let StreamFrame::Session { ref session_id } = frame {
                let mut runs = relay_runs.lock().unwrap();
                if let Some(r) = runs.get_mut(&relay_request_id) {
                    if r.session_id.is_none() {
                        r.session_id = Some(session_id.clone());
                    }
                }
            }
            if relay_outer_tx.send(frame).await.is_err() {
                break;
            }
        }
    });
    let tx = outer_tx;
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

        let tx_inner = inner_tx.clone();
        let blocking_handle =
            tokio::task::spawn_blocking(move || run_fn(agent, content, options, tx_inner));

        // B12: server-level wall-clock timeout so the blocking path is bounded.
        let outcome: Result<RunFnOutcome, RunError> = tokio::select! {
            timed_out = tokio::time::timeout(timeout, blocking_handle) => {
                match timed_out {
                    Err(_elapsed) => {
                        let _ = tx.send(StreamFrame::Error {
                            code: "run_timeout".to_string(),
                            message: "Run exceeded timeout".to_string(),
                        }).await;
                        let mut runs = runs_ref.lock().unwrap();
                        if let Some(r) = runs.get_mut(&request_id_clone) {
                            r.status = RunStatus::Idle;
                            r.last_active_at = Utc::now();
                            r.abort_handle = None;
                            r.last_exit_code = Some(1);
                        }
                        drop(tx);
                        return;
                    }
                    Ok(join_result) => match join_result {
                        Ok(r) => r,
                        Err(join_err) => {
                            if !join_err.is_cancelled() {
                                tracing::error!("spawn_blocking join error: {}", join_err);
                            }
                            let mut runs = runs_ref.lock().unwrap();
                            if let Some(r) = runs.get_mut(&request_id_clone) {
                                r.status = RunStatus::Idle;
                                r.last_active_at = Utc::now();
                                r.abort_handle = None;
                            }
                            return;
                        }
                    },
                }
            }
            _ = tx.closed() => {
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
                {
                    let mut runs = runs_ref.lock().unwrap();
                    if let Some(r) = runs.get_mut(&request_id_clone) {
                        if r.session_id.is_none() {
                            r.session_id = out.session_id.clone();
                        }
                        r.stderr_tail = out.stderr_tail.clone();
                        r.last_exit_code = Some(out.exit_code);
                    }
                }
                // E2: emit an error frame for non-zero exits with captured stderr.
                if out.exit_code != 0 && !out.stderr_tail.is_empty() {
                    let code = classify_error_code(&out.stderr_tail).to_string();
                    let _ = tx
                        .send(StreamFrame::Error {
                            code,
                            message: out.stderr_tail.clone(),
                        })
                        .await;
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
            // B2: prune stale records after each run.
            let one_hour_ago = Utc::now() - chrono::Duration::hours(1);
            runs.retain(|_, r| {
                r.status != RunStatus::Closed
                    && !(r.status == RunStatus::Idle && r.last_active_at < one_hour_ago)
            });
        }
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
    Ok((rx, request_id))
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
    if let Some(err) = validate_request(&body, &state) {
        return err;
    }
    let (rx, request_id) = match spawn_run(&state, &body) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match mode {
        ResponseMode::Sse => {
            let stream = spawn_frame_forwarder(rx, {
                let runs = Arc::clone(&state.runs);
                let request_id = request_id.clone();
                move |saw_error| {
                    runs.lock()
                        .unwrap()
                        .get(&request_id)
                        .and_then(|r| r.last_exit_code)
                        .unwrap_or(if saw_error { 1 } else { 0 })
                }
            });
            sse_response_with_headers(stream, None)
        }
        ResponseMode::Sync => sync_response(rx, Arc::clone(&state.runs), request_id).await,
        ResponseMode::NotAcceptable => unreachable!(),
    }
}

/// Drain the receiver and return a single JSON body.
async fn sync_response(
    mut rx: tokio::sync::mpsc::Receiver<StreamFrame>,
    runs: Arc<Mutex<HashMap<String, RunRecord>>>,
    request_id: String,
) -> Response {
    let mut session_id: Option<String> = None;
    let mut content = String::new();
    let mut error: Option<super::ErrorDetail> = None;
    let mut usage_seen = false;
    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;
    let mut cache_read_tokens: Option<u64> = None;

    while let Some(frame) = rx.recv().await {
        match frame {
            StreamFrame::Session { session_id: id } => {
                if session_id.is_none() {
                    session_id = Some(id);
                }
            }
            StreamFrame::Text { content: c } => content.push_str(&c),
            StreamFrame::TokenUsage {
                input_tokens: i,
                output_tokens: o,
                cache_read_tokens: c,
                ..
            } => {
                usage_seen = true;
                input_tokens = input_tokens.saturating_add(i);
                output_tokens = output_tokens.saturating_add(o);
                if let Some(c) = c {
                    cache_read_tokens = Some(cache_read_tokens.unwrap_or(0).saturating_add(c));
                }
            }
            StreamFrame::Error { code, message } => {
                if error.is_none() {
                    error = Some(super::ErrorDetail { code, message });
                }
            }
            StreamFrame::Reasoning { .. }
            | StreamFrame::ToolUse { .. }
            | StreamFrame::ToolResult { .. }
            | StreamFrame::SubagentSpawn { .. }
            | StreamFrame::SubagentResult { .. }
            | StreamFrame::ContextCompressed { .. }
            | StreamFrame::StepFinish { .. } => {}
        }
    }

    let usage = usage_seen.then_some(UsageSummary {
        input_tokens,
        output_tokens,
        cache_read_tokens,
    });

    let (record_stderr, record_exit) = {
        let guard = runs.lock().unwrap();
        if let Some(r) = guard.get(&request_id) {
            if session_id.is_none() {
                session_id.clone_from(&r.session_id);
            }
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

pub(super) async fn delete_run_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let abort_handles: Vec<tokio::task::AbortHandle> = {
        let mut runs = state.runs.lock().unwrap();
        let matching_keys: Vec<String> = runs
            .iter()
            .filter_map(|(k, r)| {
                if r.session_id.as_deref() == Some(&session_id) && r.status != RunStatus::Closed {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .collect();
        if matching_keys.is_empty() {
            return error_response(
                StatusCode::NOT_FOUND,
                "session_not_found",
                "Session not found",
            );
        }
        matching_keys
            .into_iter()
            .filter_map(|k| {
                let r = runs.get_mut(&k).unwrap();
                r.status = RunStatus::Closed;
                r.abort_handle.take()
            })
            .collect()
    };
    for handle in abort_handles {
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

#[allow(dead_code)]
pub fn make_stub_run_fn() -> RunFn {
    make_stub_run_fn_with_session(vec![], None)
}

#[allow(dead_code)]
pub fn make_blocking_stub_run_fn(duration: std::time::Duration) -> RunFn {
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

#[allow(dead_code)]
pub fn make_timeout_stub_run_fn() -> RunFn {
    Arc::new(|_agent, _prompt, _options, _tx| {
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
    fn classify_error_code_non_auth_is_agent_error() {
        assert_eq!(
            classify_error_code("Error: model is overloaded, try later"),
            "agent_error"
        );
        assert_eq!(classify_error_code(""), "agent_error");
    }
}
