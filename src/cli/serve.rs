//! `aikit serve` — HTTP server for multi-turn agent sessions with SSE streaming.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
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

use aikit_sdk::{AgentEventPayload, MessageKind, MessagePhase, MessageRole, RunError, RunOptions};

use crate::core::agent_definition::{
    load_persisted_registry, DefinitionRecord, DelegationAllowlist,
};

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

// ── session types ─────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum SessionStatus {
    Idle,
    Running,
    Closed,
}

#[derive(Clone, Serialize)]
struct SessionMessage {
    role: String,
    content: String,
    timestamp: DateTime<Utc>,
}

#[derive(Clone)]
struct Session {
    session_id: String,
    agent: String,
    model: Option<String>,
    yolo: bool,
    status: SessionStatus,
    created_at: DateTime<Utc>,
    last_active_at: DateTime<Utc>,
    messages: Vec<SessionMessage>,
    abort_handle: Option<tokio::task::AbortHandle>,
}

// ── run function type (injectable for tests) ──────────────────────────────────

/// Type alias for the agent-run function injected into AppState.
/// In production: wraps `aikit_sdk::run_agent_events`.
/// In tests: wraps a stub that emits fixed events without LLM credentials.
type RunFn = Arc<
    dyn Fn(
            String,
            String,
            RunOptions,
            tokio::sync::mpsc::Sender<Result<Event, Infallible>>,
        ) -> Result<i32, RunError>
        + Send
        + Sync,
>;

// ── app state ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    workdir: PathBuf,
    config: ServeConfig,
    run_fn: RunFn,
}

// ── HTTP body types ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CreateSessionRequest {
    agent: String,
    model: Option<String>,
    #[serde(default)]
    yolo: bool,
}

#[derive(Serialize)]
struct CreateSessionResponse {
    session_id: String,
    agent: String,
    model: Option<String>,
    status: SessionStatus,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct ListSessionsResponse {
    sessions: Vec<SessionSummary>,
}

#[derive(Serialize)]
struct SessionSummary {
    session_id: String,
    agent: String,
    status: SessionStatus,
    created_at: DateTime<Utc>,
    last_active_at: DateTime<Utc>,
    turn_count: usize,
}

#[derive(Serialize)]
struct GetSessionResponse {
    session_id: String,
    agent: String,
    status: SessionStatus,
    created_at: DateTime<Utc>,
    turn_count: usize,
    messages: Vec<SessionMessage>,
}

#[derive(Serialize)]
struct DeleteSessionResponse {
    session_id: String,
    status: SessionStatus,
}

#[derive(Deserialize)]
struct SendMessageRequest {
    content: String,
}

#[derive(Serialize)]
struct SseDonePayload {
    exit_code: i32,
    turn: usize,
}

#[derive(Serialize)]
struct SseErrorPayload {
    code: String,
    message: String,
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

fn delegation_to_json(d: &Option<DelegationAllowlist>) -> serde_json::Value {
    match d {
        None => serde_json::Value::Null,
        Some(DelegationAllowlist::All) => serde_json::Value::String("*".to_string()),
        Some(DelegationAllowlist::None) => serde_json::Value::Array(vec![]),
        Some(DelegationAllowlist::Names(v)) => {
            serde_json::Value::Array(v.iter().map(|s| serde_json::json!(s)).collect())
        }
    }
}

fn record_to_json(record: &DefinitionRecord) -> serde_json::Value {
    serde_json::json!({
        "name": record.definition.name,
        "description": record.definition.description,
        "source": record.source.to_string(),
        "model": record.definition.model,
        "tools": record.definition.tools,
        "disallowedTools": record.definition.disallowed_tools,
        "agents": delegation_to_json(&record.definition.delegation),
        "path": record.path.as_ref().map(|p| p.display().to_string()),
    })
}

// ── handlers ──────────────────────────────────────────────────────────────────

async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state;
    let version = env!("CARGO_PKG_VERSION");
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        format!(r#"{{"status":"ok","version":"{}"}}"#, version),
    )
}

async fn agents_handler(State(state): State<AppState>) -> impl IntoResponse {
    match load_persisted_registry(&state.workdir) {
        Ok(registry) => {
            let records = registry.all_sorted();
            let agents: Vec<serde_json::Value> =
                records.iter().map(|r| record_to_json(r)).collect();
            let body = serde_json::json!({ "agents": agents });
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                serde_json::to_string(&body).unwrap_or_else(|_| r#"{"agents":[]}"#.to_string()),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("agents_handler: load_persisted_registry failed: {}", e);
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Failed to load agent registry",
            )
        }
    }
}

async fn create_session_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    if body.agent.is_empty() {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            "agent field is required",
        );
    }

    let mut sessions = state.sessions.lock().unwrap();
    let active_count = sessions
        .values()
        .filter(|s| s.status != SessionStatus::Closed)
        .count();
    if active_count >= state.config.max_sessions {
        return error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "session_limit_reached",
            &format!(
                "Maximum of {} concurrent sessions reached",
                state.config.max_sessions
            ),
        );
    }

    let session_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let session = Session {
        session_id: session_id.clone(),
        agent: body.agent.clone(),
        model: body.model.clone(),
        yolo: body.yolo,
        status: SessionStatus::Idle,
        created_at: now,
        last_active_at: now,
        messages: vec![],
        abort_handle: None,
    };
    sessions.insert(session_id.clone(), session);

    let resp = CreateSessionResponse {
        session_id,
        agent: body.agent,
        model: body.model,
        status: SessionStatus::Idle,
        created_at: now,
    };
    (
        StatusCode::CREATED,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&resp).unwrap_or_default(),
    )
        .into_response()
}

async fn list_sessions_handler(State(state): State<AppState>) -> impl IntoResponse {
    let sessions = state.sessions.lock().unwrap();
    let summaries: Vec<SessionSummary> = sessions
        .values()
        .filter(|s| s.status != SessionStatus::Closed)
        .map(|s| SessionSummary {
            session_id: s.session_id.clone(),
            agent: s.agent.clone(),
            status: s.status.clone(),
            created_at: s.created_at,
            last_active_at: s.last_active_at,
            turn_count: s.messages.len() / 2,
        })
        .collect();
    let resp = ListSessionsResponse {
        sessions: summaries,
    };
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&resp).unwrap_or_default(),
    )
}

async fn get_session_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let sessions = state.sessions.lock().unwrap();
    match sessions.get(&session_id) {
        Some(s) if s.status != SessionStatus::Closed => {
            let resp = GetSessionResponse {
                session_id: s.session_id.clone(),
                agent: s.agent.clone(),
                status: s.status.clone(),
                created_at: s.created_at,
                turn_count: s.messages.len() / 2,
                messages: s.messages.clone(),
            };
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                serde_json::to_string(&resp).unwrap_or_default(),
            )
                .into_response()
        }
        _ => error_response(
            StatusCode::NOT_FOUND,
            "session_not_found",
            "Session not found",
        ),
    }
}

async fn delete_session_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let abort_handle = {
        let mut sessions = state.sessions.lock().unwrap();
        match sessions.get_mut(&session_id) {
            Some(s) if s.status != SessionStatus::Closed => {
                let handle = s.abort_handle.take();
                s.status = SessionStatus::Closed;
                handle
            }
            _ => {
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
        status: SessionStatus::Closed,
    };
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&resp).unwrap_or_default(),
    )
        .into_response()
}

async fn messages_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> Response {
    // Pre-flight validation
    if body.content.is_empty() {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            "content must not be empty",
        );
    }

    // Validate and transition to Running
    let session_info = {
        let mut sessions = state.sessions.lock().unwrap();
        match sessions.get_mut(&session_id) {
            None
            | Some(Session {
                status: SessionStatus::Closed,
                ..
            }) => {
                return error_response(
                    StatusCode::NOT_FOUND,
                    "session_not_found",
                    "Session not found",
                );
            }
            Some(s) if s.status == SessionStatus::Running => {
                return error_response(
                    StatusCode::CONFLICT,
                    "session_busy",
                    "Session is currently processing a message",
                );
            }
            Some(s) => {
                s.status = SessionStatus::Running;
                s.last_active_at = Utc::now();
                s.messages.push(SessionMessage {
                    role: "user".to_string(),
                    content: body.content.clone(),
                    timestamp: Utc::now(),
                });
                (
                    s.agent.clone(),
                    s.model.clone(),
                    s.yolo,
                    s.session_id.clone(),
                )
            }
        }
    };

    let (agent, model, yolo, sid) = session_info;
    let content = body.content.clone();
    let timeout = Duration::from_secs(state.config.run_timeout_secs);
    let run_fn = state.run_fn.clone();
    let sessions_ref = Arc::clone(&state.sessions);
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);
    let tx_clone = tx.clone();
    let session_id_clone = session_id.clone();

    let task = tokio::task::spawn(async move {
        let mut options = RunOptions::new()
            .with_yolo(yolo)
            .with_stream(true)
            .with_timeout(timeout)
            .with_session_id(&sid);
        if let Some(m) = model {
            options = options.with_model(m);
        }

        let tx_inner = tx_clone.clone();
        let blocking_handle =
            tokio::task::spawn_blocking(move || run_fn(agent, content, options, tx_inner));

        // Select between the blocking task completing and the client disconnecting.
        // tx_clone.closed() resolves when the SSE Receiver is dropped (client disconnect).
        let exit_code: i32 = tokio::select! {
            result = blocking_handle => {
                match result {
                    Ok(Ok(code)) => code,
                    Ok(Err(RunError::TimedOut { .. })) => {
                        let err_payload = serde_json::to_string(&SseErrorPayload {
                            code: "run_timeout".to_string(),
                            message: "Run exceeded timeout".to_string(),
                        })
                        .unwrap_or_default();
                        let _ = tx_clone
                            .send(Ok(Event::default().event("error").data(err_payload)))
                            .await;
                        1
                    }
                    Ok(Err(RunError::AgentNotRunnable(msg))) => {
                        let err_payload = serde_json::to_string(&SseErrorPayload {
                            code: "agent_not_runnable".to_string(),
                            message: msg,
                        })
                        .unwrap_or_default();
                        let _ = tx_clone
                            .send(Ok(Event::default().event("error").data(err_payload)))
                            .await;
                        1
                    }
                    Ok(Err(e)) => {
                        tracing::error!("messages_handler: run_agent_events error: {}", e);
                        let err_payload = serde_json::to_string(&SseErrorPayload {
                            code: "internal_error".to_string(),
                            message: "Internal error".to_string(),
                        })
                        .unwrap_or_default();
                        let _ = tx_clone
                            .send(Ok(Event::default().event("error").data(err_payload)))
                            .await;
                        1
                    }
                    Err(join_err) => {
                        if !join_err.is_cancelled() {
                            tracing::error!(
                                "messages_handler: spawn_blocking join error: {}",
                                join_err
                            );
                        }
                        -1
                    }
                }
            }
            _ = tx_clone.closed() => {
                // Client disconnected — session recovers to Idle immediately.
                // The blocking thread may continue briefly but tx sends will silently fail.
                let mut sessions = sessions_ref.lock().unwrap();
                if let Some(s) = sessions.get_mut(&session_id_clone) {
                    s.status = SessionStatus::Idle;
                    s.last_active_at = Utc::now();
                    s.abort_handle = None;
                }
                return;
            }
        };

        // Normal completion path: update session and send done frame
        let turn_count = {
            let mut sessions = sessions_ref.lock().unwrap();
            if let Some(s) = sessions.get_mut(&session_id_clone) {
                s.status = SessionStatus::Idle;
                s.last_active_at = Utc::now();
                s.abort_handle = None;
                s.messages.len() / 2
            } else {
                0
            }
        };

        if exit_code >= 0 {
            let done_payload = serde_json::to_string(&SseDonePayload {
                exit_code,
                turn: turn_count,
            })
            .unwrap_or_default();
            let _ = tx_clone
                .send(Ok(Event::default().event("done").data(done_payload)))
                .await;
        }
    });

    // Store abort handle
    {
        let mut sessions = state.sessions.lock().unwrap();
        if let Some(s) = sessions.get_mut(&session_id) {
            s.abort_handle = Some(task.abort_handle());
        }
    }

    // Drop the JoinHandle — task runs independently; abort_handle is stored in session
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
        .route("/v1/sessions", post(create_session_handler))
        .route("/v1/sessions", get(list_sessions_handler))
        .route("/v1/sessions/{session_id}", get(get_session_handler))
        .route("/v1/sessions/{session_id}", delete(delete_session_handler))
        .route("/v1/sessions/{session_id}/messages", post(messages_handler))
        .fallback(not_found_handler)
        .with_state(state.clone());

    if state.config.api_key.is_some() {
        router.layer(middleware::from_fn_with_state(state, auth_middleware))
    } else {
        router
    }
}

// ── production run_fn ─────────────────────────────────────────────────────────

/// Build the production run_fn that calls run_agent_events and forwards events to the SSE channel.
pub fn make_production_run_fn() -> RunFn {
    Arc::new(
        move |agent: String,
              prompt: String,
              options: RunOptions,
              tx: tokio::sync::mpsc::Sender<Result<Event, Infallible>>| {
            use aikit_sdk::run_agent_events;

            let result = run_agent_events(&agent, &prompt, options, |event| {
                let sse_event = agent_event_to_sse(&event);
                if let Some(ev) = sse_event {
                    // Best-effort send — if receiver is dropped, client disconnected
                    let _ = tx.blocking_send(Ok(ev));
                }
            });

            result.map(|r| r.exit_code().unwrap_or(0))
        },
    )
}

/// Map an AgentEvent to an SSE Event, returning None if the event should be suppressed.
fn agent_event_to_sse(event: &aikit_sdk::AgentEvent) -> Option<Event> {
    match &event.payload {
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
        // Suppress token usage, raw transport, and other internal events
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

    let workdir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let state = AppState {
        sessions: Arc::new(Mutex::new(HashMap::new())),
        workdir,
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

    // Abort any in-flight sessions
    {
        let mut sessions = state.sessions.lock().unwrap();
        for session in sessions.values_mut() {
            if let Some(handle) = session.abort_handle.take() {
                handle.abort();
            }
        }
    }

    // Allow up to 5 seconds for tasks to wind down
    tokio::time::sleep(Duration::from_secs(5)).await;

    Ok(())
}

// ── test stub helpers ─────────────────────────────────────────────────────────

/// Build a stub run function that emits a fixed sequence of SSE events and exits with code 0.
/// `events` is a list of `(event_name, data)` pairs.
/// No LLM credentials are needed — safe for CI.
#[allow(dead_code)]
pub fn make_stub_run_fn(events: Vec<(&'static str, &'static str)>) -> RunFn {
    Arc::new(move |_agent, _prompt, _options, tx| {
        for (name, data) in &events {
            let ev = Event::default().event(*name).data(*data);
            let _ = tx.blocking_send(Ok(ev));
        }
        Ok(0)
    })
}

/// Build a stub run function that sleeps for `duration` before returning.
/// Use this to test concurrent request rejection (409 session_busy).
#[allow(dead_code)]
pub fn make_blocking_stub_run_fn(duration: Duration) -> RunFn {
    Arc::new(move |_agent, _prompt, _options, _tx| {
        std::thread::sleep(duration);
        Ok(0)
    })
}

/// Build a stub run function that returns a `RunError::TimedOut` error.
/// Use this to test the timeout SSE error path.
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
