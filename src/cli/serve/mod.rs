//! `aikit serve` — HTTP server for multi-turn agent sessions.
//!
//! Two response shapes share one endpoint, `POST /api/v1/messages`, selected
//! by the `Accept` header:
//! - `text/event-stream` → SSE. serve emits the canonical SDK
//!   [`aikit_sdk::AgentEventPayload`] directly (ADR 0016 / ARCH-4): the SSE
//!   `event:` name is the payload's own snake_case variant tag (e.g.
//!   `stream_message`, `tool_use`, `tool_result`, `token_usage_line`,
//!   `session_started`, `aikit_subagent_spawn`, …) and `data:` is that
//!   variant's inner JSON, unmodified. Two additional SSE events are
//!   serve-level control signals, not agent events: `error` (an
//!   orchestration failure — timeout, capacity, closed session, internal
//!   error, abnormal termination) and `done` (terminal event carrying
//!   `exit_code`).
//! - `application/json` → server accumulates text + usage, returns a single
//!   `{session_id, content, exit_code, error?, usage?}` body.
//! - `*/*` or missing → SSE (default). Any other type → 406.
//!
//! Bidirectional sessions (`/api/v1/live-sessions`) are handled by
//! [`live_session`] and one-shot runs by [`run_session`].

mod live_session;
mod run_session;

#[cfg(feature = "agent-adapters")]
mod capture;
#[cfg(feature = "agent-adapters")]
pub(crate) mod storage;

use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::Router;
use serde::Serialize;
use tokio_stream::wrappers::ReceiverStream;

use aikit_sdk::{
    AgentEvent, AgentEventPayload, MessageKind, MessagePhase, MessageRole, RunOptions,
};

use cli_framework::api::{
    ApiServerBuilder, ApiVersion, ApiVersionName, DefaultVersion, ReadinessReport, Stability,
};
use cli_framework::tower::util::BoxCloneLayer;

// Stub run-fns used by integration tests — not referenced from the binary itself.
#[allow(unused_imports)]
pub use run_session::{
    make_blocking_stub_run_fn, make_failing_stub_run_fn, make_stub_run_fn,
    make_stub_run_fn_with_session, make_timeout_stub_run_fn, RunFn, RunFnOutcome,
};

// ── public args ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct ServeArgs {
    pub host: String,
    pub port: u16,
    pub run_timeout_secs: u64,
    pub max_sessions: usize,
    pub api_key: Option<String>,
    /// SEC-2 / ADR 0012: override the fail-closed startup check that
    /// otherwise refuses to bind a non-loopback address without
    /// `--api-key`. Off by default; operators must opt in explicitly.
    pub insecure: bool,
}

// ── shared config ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub(super) struct ServeConfig {
    pub host: String,
    pub port: u16,
    pub run_timeout_secs: u64,
    pub max_sessions: usize,
    pub api_key: Option<String>,
    pub insecure: bool,
}

// ── app state ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
#[allow(private_interfaces)]
pub(super) struct AppState {
    pub(super) runs: Arc<Mutex<HashMap<String, run_session::RunRecord>>>,
    pub(super) live_sessions: live_session::LiveSessions,
    /// SEC-3: slots reserved by in-flight live-session opens that have passed the capacity
    /// check but not yet inserted their record. Mutated only while holding the
    /// `live_sessions` lock, so check-and-reserve is atomic and concurrent opens can't
    /// overshoot `max_sessions`.
    pub(super) pending_live_sessions: Arc<std::sync::atomic::AtomicUsize>,
    pub(super) config: ServeConfig,
    pub(super) run_fn: RunFn,
    pub(super) auth_cache: run_session::AuthCache,
}

// ── shared types ──────────────────────────────────────────────────────────────

/// One item flowing from run orchestration to the SSE encoder.
///
/// ADR 0016 / ARCH-4: the old serve-private event vocabulary — a second,
/// lossy re-map of the SDK's canonical [`AgentEventPayload`] — is deleted
/// outright. serve now forwards `AgentEvent` unmodified; the only other item
/// this channel carries is a serve-level `Error`, which signals an
/// *orchestration* failure (timeout, capacity, closed session, internal
/// error, abnormal termination) that originates from serve itself, not from
/// the agent, and therefore has no canonical `AgentEventPayload` shape to
/// borrow.
#[derive(Debug, Clone)]
pub enum ServeEvent {
    /// A canonical SDK agent event, forwarded as-is (no re-mapping).
    Agent(AgentEvent),
    /// A serve-level orchestration error (not an agent event).
    Error { code: String, message: String },
}

#[derive(Serialize)]
pub(super) struct ErrorDetail {
    pub code: String,
    pub message: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

pub(super) fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
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

// ── ServeEvent → SSE ──────────────────────────────────────────────────────────

/// Serialize one [`ServeEvent`] to an SSE `Event`.
///
/// For `ServeEvent::Agent`, the SSE `event:` name is derived directly from
/// the payload's own serde tag (its externally-tagged JSON representation is
/// always a single-key object, `{"<snake_case_variant>": <inner>}` — every
/// `AgentEventPayload` variant is a newtype or struct variant, never a bare
/// unit) and `data:` is that inner JSON, unmodified. This is the canonical
/// passthrough ADR 0016 requires: no hand-maintained variant→name table to
/// drift out of sync with the SDK, and no lossy re-mapping.
pub(super) fn serve_event_to_sse(item: &ServeEvent) -> Event {
    match item {
        ServeEvent::Agent(event) => {
            let value = serde_json::to_value(&event.payload).unwrap_or(serde_json::Value::Null);
            if let serde_json::Value::Object(map) = value {
                if let Some((tag, inner)) = map.into_iter().next() {
                    return Event::default().event(tag).data(inner.to_string());
                }
            }
            // Unreachable for any current `AgentEventPayload` variant (all are
            // newtype/struct, never unit), but degrade gracefully rather than
            // panic if the SDK ever adds one.
            Event::default()
                .event("agent_event")
                .data(serde_json::to_string(&event.payload).unwrap_or_else(|_| "null".into()))
        }
        ServeEvent::Error { code, message } => Event::default()
            .event("error")
            .data(serde_json::json!({ "code": code, "message": message }).to_string()),
    }
}

/// True when `item` represents an error condition — either a serve-level
/// `ServeEvent::Error`, or a canonical `QuotaExceeded` agent event. Quota
/// exhaustion is agent-originated and passes through as its own canonical
/// SSE event (`quota_exceeded`) for full fidelity, but it still counts as an
/// error for the stream's terminal `done` exit-code fallback, matching prior
/// behaviour when it was lossily re-mapped onto a generic error frame.
fn is_error_signal(item: &ServeEvent) -> bool {
    match item {
        ServeEvent::Error { .. } => true,
        ServeEvent::Agent(event) => {
            matches!(event.payload, AgentEventPayload::QuotaExceeded { .. })
        }
    }
}

// ── shared SSE utilities ──────────────────────────────────────────────────────

/// Spawn an async forwarder that pumps `item_rx` into a new SSE event channel,
/// then emits a terminal `done` event. `get_exit_code(saw_error)` is called
/// once after the stream closes to determine the done payload.
///
/// Returns the receiver stream ready to be wrapped in `Sse::new(...)`.
pub(super) fn spawn_frame_forwarder(
    mut item_rx: tokio::sync::mpsc::Receiver<ServeEvent>,
    get_exit_code: impl FnOnce(bool) -> i32 + Send + 'static,
) -> ReceiverStream<Result<Event, Infallible>> {
    let (out_tx, out_rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);
    tokio::spawn(async move {
        let mut saw_error = false;
        loop {
            tokio::select! {
                maybe = item_rx.recv() => match maybe {
                    Some(item) => {
                        if is_error_signal(&item) {
                            saw_error = true;
                        }
                        if out_tx.send(Ok(serve_event_to_sse(&item))).await.is_err() {
                            return;
                        }
                    }
                    None => break,
                },
                _ = out_tx.closed() => return,
            }
        }
        let exit_code = get_exit_code(saw_error);
        let _ = out_tx
            .send(Ok(Event::default().event("done").data(
                serde_json::json!({ "exit_code": exit_code }).to_string(),
            )))
            .await;
    });
    ReceiverStream::new(out_rx)
}

/// Build an SSE response from a frame stream, setting standard no-cache headers.
/// `extra_header` is an optional `(name, value)` to append (e.g. `x-session-id`).
pub(super) fn sse_response_with_headers(
    stream: ReceiverStream<Result<Event, Infallible>>,
    extra_header: Option<(&str, &str)>,
) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    );
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    if let Some((name, val)) = extra_header {
        if let (Ok(hname), Ok(hval)) = (
            axum::http::header::HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(val),
        ) {
            headers.insert(hname, hval);
        }
    }
    (headers, Sse::new(stream)).into_response()
}

fn payload_kind_name(p: &AgentEventPayload) -> &'static str {
    match p {
        AgentEventPayload::JsonLine(_) => "json_line",
        AgentEventPayload::RawLine(_) => "raw_line",
        AgentEventPayload::RawBytes(_) => "raw_bytes",
        AgentEventPayload::StreamMessage(_) => "stream_message",
        AgentEventPayload::ToolUse { .. } => "tool_use",
        AgentEventPayload::ToolResult { .. } => "tool_result",
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

pub(super) fn stderr_tail(stderr: &[u8]) -> String {
    if stderr.is_empty() {
        return String::new();
    }
    let start = stderr
        .len()
        .saturating_sub(run_session::MAX_STDERR_TAIL_BYTES);
    String::from_utf8_lossy(&stderr[start..]).trim().to_string()
}

// ── production run_fn ─────────────────────────────────────────────────────────

pub fn make_production_run_fn() -> RunFn {
    Arc::new(
        move |agent: String,
              prompt: String,
              options: RunOptions,
              tx: tokio::sync::mpsc::Sender<ServeEvent>,
              cancel: aikit_sdk::runner::RunCancelHandle| {
            use aikit_sdk::runner::run_agent_events_cancellable;

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
            let saw_assistant_delta = Arc::new(Mutex::new(false));
            let saw_delta_for_cb = Arc::clone(&saw_assistant_delta);
            let agent_for_cb = agent.clone();

            let result =
                run_agent_events_cancellable(&agent, &prompt, options, &cancel, move |event| {
                    let payload_kind = payload_kind_name(&event.payload);

                    // B5: capture the first session_id learned from a turn_id
                    // and synthesize a canonical `SessionStarted` event for
                    // it — this is serve's own bookkeeping (not a re-map of
                    // the triggering event, which is still forwarded below).
                    if let AgentEventPayload::StreamMessage(msg) = &event.payload {
                        if let Some(ref tid) = msg.turn_id {
                            let mut cap = captured_for_cb.lock().unwrap();
                            if cap.is_none() {
                                *cap = Some(tid.clone());
                                drop(cap);
                                let synthetic = AgentEvent {
                                    agent_key: agent_for_cb.clone(),
                                    seq: event.seq,
                                    stream: event.stream,
                                    payload: AgentEventPayload::SessionStarted {
                                        session_id: tid.clone(),
                                    },
                                };
                                let _ = tx_cb.blocking_send(ServeEvent::Agent(synthetic));
                            }
                        }
                        // Dedup Final-after-Delta for non-aikit assistant messages:
                        // many CLI backends emit a Delta stream of chunks
                        // followed by a redundant Final containing the same
                        // concatenated text; forwarding both would duplicate
                        // content for consumers. This is stream hygiene, not
                        // vocabulary loss — every other `StreamMessage` (any
                        // role/kind/phase) passes through untouched below.
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
                                    "suppressing Final assistant StreamMessage (dup of Delta)"
                                );
                                return;
                            }
                        }
                    }

                    // Mirror the same Final-after-Delta dedup for the
                    // built-in aikit backend's own delta/final text events.
                    if matches!(event.payload, AgentEventPayload::AikitTextFinal { .. }) {
                        tracing::trace!(
                            target: "aikit::serve::run",
                            agent = %agent_for_cb,
                            payload = payload_kind,
                            "suppressing AikitTextFinal (dup of AikitTextDelta)"
                        );
                        return;
                    }

                    if let AgentEventPayload::SessionStarted { ref session_id } = event.payload {
                        let mut cap = captured_for_cb.lock().unwrap();
                        if cap.is_none() {
                            *cap = Some(session_id.clone());
                        }
                    }

                    tracing::debug!(
                        target: "aikit::serve::run",
                        agent = %agent_for_cb,
                        payload = payload_kind,
                        "forwarding canonical event"
                    );
                    if let Err(e) = tx_cb.blocking_send(ServeEvent::Agent(event)) {
                        tracing::warn!(
                            target: "aikit::serve::run",
                            agent = %agent_for_cb,
                            error = %e,
                            "event channel send failed"
                        );
                    }
                });

            let session_id = captured_session_id.lock().unwrap().clone();
            match result {
                Ok(r) => {
                    // BUG-3: `ExitStatus::code()` is `None` on signal death
                    // (OOM-kill, `kill -9`, segfault) — never coerce that to
                    // 0 (success). Map it to a distinct sentinel and surface
                    // an error frame so abnormal termination is visible
                    // rather than silently reported as a clean exit.
                    let exit_code = match r.exit_code() {
                        Some(code) => code,
                        None => {
                            let _ = tx.blocking_send(ServeEvent::Error {
                                code: "abnormal_termination".to_string(),
                                message: "Agent process terminated abnormally (signal or crash)"
                                    .to_string(),
                            });
                            137
                        }
                    };
                    let stderr_tail = stderr_tail(&r.stderr);
                    tracing::info!(
                        target: "aikit::serve::run",
                        agent = %agent, exit_code, session_id = ?session_id,
                        stderr_bytes = r.stderr.len(),
                        "agent run completed"
                    );
                    if exit_code != 0 && !stderr_tail.is_empty() {
                        tracing::warn!(
                            target: "aikit::serve::run",
                            agent = %agent, exit_code, stderr_tail = %stderr_tail,
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
                    tracing::error!(target: "aikit::serve::run", agent = %agent, error = %e, "agent run failed");
                    Err(e)
                }
            }
        },
    )
}

// ── router + entry point ──────────────────────────────────────────────────────

/// Build the default capture registry + storage for `aikit serve`, and
/// optionally spawn a background WatchDriver (spec 010 §14.5). The SQLite
/// DB lives under `~/.local/share/aikit/capture.db`.
#[cfg(feature = "agent-adapters")]
pub(crate) fn capture_db_path() -> std::path::PathBuf {
    dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("aikit")
        .join("capture.db")
}

#[cfg(feature = "agent-adapters")]
fn build_capture_state() -> anyhow::Result<capture::CaptureState> {
    use aikit_session_capture::Registry;

    let mut reg = Registry::new();
    #[cfg(feature = "claudecode")]
    reg.register(Box::new(
        aikit_session_capture::claudecode::ClaudeCodeAdapter::new(),
    ));
    #[cfg(feature = "codex")]
    reg.register(Box::new(aikit_session_capture::codex::CodexAdapter::new()));
    #[cfg(feature = "opencode")]
    reg.register(Box::new(
        aikit_session_capture::opencode::OpenCodeAdapter::new(),
    ));

    let db_path = capture_db_path();
    let conn = storage::schema::open(&db_path)?;
    let event_store: std::sync::Arc<dyn aikit_session_capture::EventStore> =
        std::sync::Arc::new(storage::SqliteEventStore::new(conn.clone()));
    let cursor_store: std::sync::Arc<dyn aikit_session_capture::CursorStore> =
        std::sync::Arc::new(storage::SqliteCursorStore::new(conn));

    let state = capture::CaptureState::new(reg, event_store, cursor_store);

    // Spawn the background WatchDriver if the `watcher` feature is enabled.
    // The driver drains into the same parse_and_store_file pipeline the
    // manual POST /capture/scan route uses (spec §14.3 invariant).
    #[cfg(feature = "watcher")]
    {
        let state_ref = std::sync::Arc::new(state.clone());
        // Build a NotifyWatchDriver over every detected adapter's watch paths.
        let adapters: Vec<&dyn aikit_session_capture::Adapter> = state_ref.registry.all();
        match aikit_session_capture::watch::NotifyWatchDriver::new(
            adapters,
            std::time::Duration::from_millis(250),
        ) {
            Ok(driver) => {
                state_ref.spawn_watcher(Box::new(driver));
                tracing::info!(
                    target: "aikit::serve::capture",
                    "watch driver started (250ms debounce)"
                );
            }
            Err(e) => {
                tracing::warn!(
                    target: "aikit::serve::capture",
                    error = %e,
                    "watch driver failed to start; falling back to manual scans only"
                );
            }
        }
    }

    Ok(state)
}

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/agents", get(run_session::agents_handler))
        .route("/messages", post(run_session::messages_handler))
        .route("/sessions", get(run_session::list_runs_handler))
        .route("/sessions/{session_id}", get(run_session::get_run_handler))
        .route(
            "/sessions/{session_id}",
            delete(run_session::delete_run_handler),
        )
        .route(
            "/live-sessions",
            post(live_session::create_live_session_handler),
        )
        .route(
            "/live-sessions",
            get(live_session::list_live_sessions_handler),
        )
        .route(
            "/live-sessions/{session_id}/control",
            post(live_session::live_session_control_handler),
        )
        .route(
            "/live-sessions/{session_id}",
            delete(live_session::delete_live_session_handler),
        )
        .with_state(state)
}

pub async fn execute(args: ServeArgs) -> anyhow::Result<()> {
    init_tracing();
    execute_with_run_fn(args, make_production_run_fn()).await
}

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
        insecure: args.insecure,
    };

    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid address {}:{}: {}", config.host, config.port, e))?;

    // SEC-2 / ADR 0012: the sandbox is the trust boundary for the agent
    // itself (full toolset, `run_bash` included — see ADR 0012); the
    // network perimeter is the *only* remaining in-app control, and it must
    // fail closed. A non-loopback bind with no `--api-key` would let any
    // network-reachable caller drive an unconstrained agent (including
    // shell execution) with zero authentication, so refuse to start unless
    // the operator either supplies a key or explicitly opts into
    // `--insecure`. Loopback stays open by default (matches ADR 0012:
    // existing local consumers — agentrt, the optimization loop, chat BFFs
    // — are unaffected).
    if !addr.ip().is_loopback() && config.api_key.is_none() && !config.insecure {
        anyhow::bail!(
            "refusing to start: {addr} is a non-loopback bind address and no --api-key was \
             set. aikit serve has no host-safety sandboxing of its own (see ADR 0012 — the \
             agent's full toolset, including shell execution, is left intact deliberately); \
             the network perimeter is the only control standing between a network-reachable \
             caller and an unauthenticated, unconstrained agent. Deploy aikit serve inside a \
             disposable sandbox/container and either pass --api-key <KEY> to require \
             `Authorization: Bearer <KEY>`, or bind to a loopback address (127.0.0.1) and put \
             an auth-enforcing reverse proxy in front for remote access. Pass --insecure only \
             if you understand and accept this exposure."
        );
    }
    if !addr.ip().is_loopback() && config.api_key.is_none() && config.insecure {
        eprintln!(
            "SECURITY WARNING: serving on non-loopback address {addr} with NO --api-key \
             (--insecure was set). Any network-reachable caller can drive this agent \
             (including shell execution) with zero authentication. This is not recommended \
             outside a fully disposable, network-isolated sandbox."
        );
    }

    let state = AppState {
        runs: Arc::new(Mutex::new(HashMap::new())),
        live_sessions: Arc::new(Mutex::new(HashMap::new())),
        pending_live_sessions: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        config: config.clone(),
        run_fn,
        auth_cache: Arc::new(Mutex::new(None)),
    };

    let domain_router = build_router(state.clone());

    #[cfg(feature = "tools")]
    let domain_router = domain_router.merge(aikit_magictool::router(
        aikit_magictool::state_with_registry({
            let mut reg = aikit_magictool::ToolRegistry::new();
            reg.register(crate::tools::draft_agent_definition_tool());
            reg
        }),
    ));

    #[cfg(feature = "agent-adapters")]
    let domain_router = {
        let capture_state = build_capture_state()?;
        domain_router.merge(capture::build_router(capture_state))
    };

    let mut builder = ApiServerBuilder::new()
        .version(ApiVersion {
            name: ApiVersionName::new_unchecked("v1"),
            router: domain_router,
            stability: Stability::Stable,
            deprecation: None,
        })
        .default_version(DefaultVersion::Pinned(ApiVersionName::new_unchecked("v1")))
        .readiness_check(Arc::new(|| {
            Box::pin(async {
                ReadinessReport {
                    ready: true,
                    checks: BTreeMap::new(),
                }
            })
        }));

    if let Some(ref key) = config.api_key {
        let key = key.clone();
        let bearer_layer = BoxCloneLayer::new(axum::middleware::from_fn(
            move |req: axum::http::Request<axum::body::Body>, next: axum::middleware::Next| {
                let key = key.clone();
                async move {
                    let authorized = req
                        .headers()
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|h| h.strip_prefix("Bearer "))
                        .map(|t| t == key.as_str())
                        .unwrap_or(false);
                    if authorized {
                        next.run(req).await
                    } else {
                        error_response(
                            StatusCode::UNAUTHORIZED,
                            "unauthorized",
                            "Invalid or missing API key",
                        )
                    }
                }
            },
        ));
        builder = builder.auth(bearer_layer);
    }

    let server = builder.build();

    eprintln!("Listening on http://{}", addr);
    if !addr.ip().is_loopback() && config.api_key.is_some() {
        // The fail-closed check above already guarantees a non-loopback
        // bind has *some* auth in place (a key, since --insecure without a
        // key was warned about separately); this is just a deployment-
        // hygiene reminder, not a security gap.
        eprintln!(
            "Note: server is bound to a non-loopback address with --api-key set. Also \
             restrict access via network ACLs / a sandbox boundary per ADR 0012 — aikit \
             serve does not sandbox the agent itself."
        );
    }

    let token = server.shutdown_token();
    let runs_ref = Arc::clone(&state.runs);
    tokio::spawn(async move {
        token.cancelled().await;
        let mut runs = runs_ref.lock().unwrap();
        for r in runs.values_mut() {
            if let Some(handle) = r.abort_handle.take() {
                handle.abort();
            }
        }
    });

    server
        .serve(&addr.to_string())
        .await
        .map_err(|e| anyhow::anyhow!("server error: {}", e))
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_sdk::{
        AgentEvent, AgentEventStream, MessagePhase, MessageRole, StreamMessage, TokenUsage,
        UsageSource,
    };

    fn event(payload: AgentEventPayload) -> AgentEvent {
        AgentEvent {
            agent_key: "aikit".to_string(),
            seq: 0,
            stream: AgentEventStream::Stdout,
            payload,
        }
    }

    /// Extract `(tag, inner_json)` the same way `serve_event_to_sse` does,
    /// without going through the opaque `axum::response::sse::Event` type.
    fn tag_and_inner(payload: &AgentEventPayload) -> (String, serde_json::Value) {
        let value = serde_json::to_value(payload).unwrap();
        match value {
            serde_json::Value::Object(map) => {
                let (tag, inner) = map.into_iter().next().expect("non-empty object");
                (tag, inner)
            }
            other => panic!("expected externally-tagged object, got {other:?}"),
        }
    }

    #[test]
    fn canonical_tool_use_and_result_pass_through_untranslated() {
        // ARCH-4: `ToolResult.call_id` keeps its own meaning — no more
        // overloaded `name` field standing in for agent-key on CLI backends
        // and call-id on structured ones.
        let use_ev = AgentEventPayload::ToolUse {
            call_id: "tu_1".into(),
            tool_name: "Bash".into(),
            input: serde_json::json!({"command": "ls"}),
        };
        let (tag, data) = tag_and_inner(&use_ev);
        assert_eq!(tag, "tool_use");
        assert_eq!(data["tool_name"], "Bash");
        assert_eq!(data["call_id"], "tu_1");
        assert_eq!(data["input"]["command"], "ls");

        let res_ev = AgentEventPayload::ToolResult {
            call_id: "tu_1".into(),
            output: serde_json::json!("file.txt\n"),
            is_error: false,
        };
        let (tag, data) = tag_and_inner(&res_ev);
        assert_eq!(tag, "tool_result");
        assert_eq!(data["call_id"], "tu_1");
        assert_eq!(data["output"], "file.txt\n");
        assert_eq!(data["is_error"], false);
    }

    #[test]
    fn token_usage_line_passes_through_with_native_usage_source_serialization() {
        // Previously-dropped event (ARCH-4 recovery target): TokenUsageLine
        // was never forwarded by the old serve-side event re-map targeting
        // SSE — now it flows as its own canonical `token_usage_line` event.
        let ev = AgentEventPayload::TokenUsageLine {
            usage: TokenUsage {
                input_tokens: 12,
                output_tokens: 34,
                total_tokens: Some(46),
                cache_read_tokens: Some(5),
                cache_creation_tokens: None,
                reasoning_tokens: None,
            },
            source: UsageSource::Aikit,
            raw_agent_line_seq: 7,
        };
        let (tag, data) = tag_and_inner(&ev);
        assert_eq!(tag, "token_usage_line");
        assert_eq!(data["usage"]["input_tokens"], 12);
        assert_eq!(data["usage"]["output_tokens"], 34);
        assert_eq!(data["usage"]["cache_read_tokens"], 5);
        // Native enum serialization (no serve-local lowercasing map).
        assert_eq!(data["source"], "Aikit");
    }

    #[test]
    fn reasoning_stream_message_passes_through() {
        // Previously-dropped event: Reasoning-kind StreamMessages were
        // suppressed for every role except the narrow (_, Reasoning, _) arm
        // that happened to be kept; now every StreamMessage passes through.
        let ev = AgentEventPayload::StreamMessage(StreamMessage {
            text: "thinking...".to_string(),
            phase: MessagePhase::Delta,
            role: MessageRole::Assistant,
            kind: aikit_sdk::MessageKind::Reasoning,
            source: AgentEventStream::Stdout,
            raw_line_seq: 0,
            turn_id: None,
        });
        let (tag, data) = tag_and_inner(&ev);
        assert_eq!(tag, "stream_message");
        assert_eq!(data["text"], "thinking...");
        assert_eq!(data["kind"], "reasoning");
    }

    #[test]
    fn aikit_subagent_and_context_compression_events_pass_through() {
        // Previously-dropped events (ARCH-4 recovery target): subagent
        // spawn/result and context compression were never reachable over SSE
        // with full fidelity before (the old serve-side re-map produced a
        // similar shape, but the canonical fields — call_id-correct
        // identifiers, final_message, etc. — were still lossy).
        let spawn = AgentEventPayload::AikitSubagentSpawn {
            subagent_id: "sub-1".into(),
            workdir: "/tmp/sub-1".into(),
        };
        let (tag, data) = tag_and_inner(&spawn);
        assert_eq!(tag, "aikit_subagent_spawn");
        assert_eq!(data["subagent_id"], "sub-1");

        let result = AgentEventPayload::AikitSubagentResult {
            subagent_id: "sub-1".into(),
            status: "done".into(),
            changed_files: vec!["a.rs".into()],
            key_findings: "fixed it".into(),
            final_message: "all good".into(),
        };
        let (tag, data) = tag_and_inner(&result);
        assert_eq!(tag, "aikit_subagent_result");
        assert_eq!(data["final_message"], "all good");

        let compressed = AgentEventPayload::AikitContextCompressed {
            original_tokens: 1000,
            compressed_tokens: 200,
            turns_summarized: 5,
        };
        let (tag, data) = tag_and_inner(&compressed);
        assert_eq!(tag, "aikit_context_compressed");
        assert_eq!(data["original_tokens"], 1000);

        let step = AgentEventPayload::AikitStepFinish {
            iteration: 3,
            finish_reason: "stop".into(),
        };
        let (tag, data) = tag_and_inner(&step);
        assert_eq!(tag, "aikit_step_finish");
        assert_eq!(data["iteration"], 3);
    }

    #[test]
    fn cli_tool_message_stream_events_pass_through_as_stream_message() {
        // codex/opencode emit (Tool, Message) for command text and (Tool,
        // ToolOutput) for its output — no longer squashed into a generic
        // ToolUse/ToolResult with a synthetic call_id/name; they keep their
        // native `stream_message` shape with role/kind intact.
        let ev = AgentEventPayload::StreamMessage(StreamMessage {
            text: "ls -la".to_string(),
            phase: MessagePhase::Delta,
            role: MessageRole::Tool,
            kind: aikit_sdk::MessageKind::Message,
            source: AgentEventStream::Stdout,
            raw_line_seq: 0,
            turn_id: None,
        });
        let (tag, data) = tag_and_inner(&ev);
        assert_eq!(tag, "stream_message");
        assert_eq!(data["role"], "tool");
        assert_eq!(data["kind"], "message");
        assert_eq!(data["text"], "ls -la");
    }

    #[test]
    fn serve_error_event_shape() {
        let sse = serve_event_to_sse(&ServeEvent::Error {
            code: "run_timeout".into(),
            message: "Run exceeded timeout".into(),
        });
        let rendered = format!("{:?}", sse);
        assert!(rendered.contains("run_timeout"));
    }

    #[test]
    fn is_error_signal_covers_serve_error_and_quota_exceeded() {
        let err = ServeEvent::Error {
            code: "run_timeout".into(),
            message: "x".into(),
        };
        assert!(is_error_signal(&err));

        let quota = ServeEvent::Agent(event(AgentEventPayload::QuotaExceeded {
            info: aikit_sdk::QuotaExceededInfo {
                agent_key: "codex".into(),
                category: aikit_sdk::QuotaCategory::Hourly,
                raw_message: "rate limited".into(),
            },
            raw_agent_line_seq: 0,
        }));
        assert!(is_error_signal(&quota));

        let text = ServeEvent::Agent(event(AgentEventPayload::AikitTextDelta {
            content: "hi".into(),
            turn_id: None,
        }));
        assert!(!is_error_signal(&text));
    }

    #[test]
    fn payload_kind_name_covers_external_tool_variants() {
        // Regression guard: the old helper's fallback `_ => "other"` silently
        // swallowed ToolUse/ToolResult (never given explicit arms because
        // nothing depended on their debug label before). Now that debug
        // logging is the only consumer, still keep it accurate.
        let use_ev = AgentEventPayload::ToolUse {
            call_id: "c1".into(),
            tool_name: "Bash".into(),
            input: serde_json::Value::Null,
        };
        assert_eq!(payload_kind_name(&use_ev), "tool_use");
        let res_ev = AgentEventPayload::ToolResult {
            call_id: "c1".into(),
            output: serde_json::Value::Null,
            is_error: false,
        };
        assert_eq!(payload_kind_name(&res_ev), "tool_result");
    }
}
