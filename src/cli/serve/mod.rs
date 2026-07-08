//! `aikit serve` — HTTP server for multi-turn agent sessions.
//!
//! Two response shapes share one endpoint, `POST /api/v1/messages`, selected
//! by the `Accept` header:
//! - `text/event-stream` → SSE. Events: `session`, `text`, `reasoning`,
//!   `tool_use`, `tool_result`, `token_usage`, `subagent_spawn`,
//!   `subagent_result`, `context_compressed`, `step_finish`, `error`, `done`.
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
    AgentEventPayload, MessageKind, MessagePhase, MessageRole, RunOptions, UsageSource,
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
}

// ── shared config ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub(super) struct ServeConfig {
    pub host: String,
    pub port: u16,
    pub run_timeout_secs: u64,
    pub max_sessions: usize,
    pub api_key: Option<String>,
}

// ── app state ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
#[allow(private_interfaces)]
pub(super) struct AppState {
    pub(super) runs: Arc<Mutex<HashMap<String, run_session::RunRecord>>>,
    pub(super) live_sessions: live_session::LiveSessions,
    pub(super) config: ServeConfig,
    pub(super) run_fn: RunFn,
    pub(super) auth_cache: run_session::AuthCache,
}

// ── shared types ──────────────────────────────────────────────────────────────

/// One structured event from an agent run. Handlers convert these to SSE
/// for stream mode and accumulate them for sync mode.
#[derive(Debug, Clone)]
pub enum StreamFrame {
    Session {
        session_id: String,
    },
    Text {
        content: String,
    },
    Reasoning {
        content: String,
    },
    ToolUse {
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        name: String,
        output: String,
        is_error: bool,
    },
    TokenUsage {
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: Option<u64>,
        source: String,
    },
    SubagentSpawn {
        subagent_id: String,
        workdir: String,
    },
    SubagentResult {
        subagent_id: String,
        status: String,
        changed_files: Vec<String>,
        key_findings: String,
    },
    ContextCompressed {
        original_tokens: u64,
        compressed_tokens: u64,
        turns_summarized: u64,
    },
    StepFinish {
        iteration: u32,
        finish_reason: String,
    },
    Error {
        code: String,
        message: String,
    },
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

// ── frame → SSE ───────────────────────────────────────────────────────────────

pub(super) fn frame_to_sse(frame: &StreamFrame) -> Event {
    match frame {
        StreamFrame::Session { session_id } => Event::default()
            .event("session")
            .data(serde_json::json!({ "session_id": session_id }).to_string()),
        StreamFrame::Text { content } => Event::default()
            .event("text")
            .data(serde_json::json!({ "content": content }).to_string()),
        StreamFrame::Reasoning { content } => Event::default()
            .event("reasoning")
            .data(serde_json::json!({ "content": content }).to_string()),
        StreamFrame::ToolUse { name, input } => Event::default()
            .event("tool_use")
            .data(serde_json::json!({ "name": name, "input": input }).to_string()),
        StreamFrame::ToolResult {
            name,
            output,
            is_error,
        } => Event::default().event("tool_result").data(
            serde_json::json!({ "name": name, "output": output, "is_error": is_error }).to_string(),
        ),
        StreamFrame::TokenUsage {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            source,
        } => Event::default().event("token_usage").data(
            serde_json::json!({
                "input_tokens": input_tokens, "output_tokens": output_tokens,
                "cache_read_tokens": cache_read_tokens, "source": source,
            })
            .to_string(),
        ),
        StreamFrame::SubagentSpawn {
            subagent_id,
            workdir,
        } => Event::default().event("subagent_spawn").data(
            serde_json::json!({ "subagent_id": subagent_id, "workdir": workdir }).to_string(),
        ),
        StreamFrame::SubagentResult {
            subagent_id,
            status,
            changed_files,
            key_findings,
        } => Event::default().event("subagent_result").data(
            serde_json::json!({
                "subagent_id": subagent_id, "status": status,
                "changed_files": changed_files, "key_findings": key_findings,
            })
            .to_string(),
        ),
        StreamFrame::ContextCompressed {
            original_tokens,
            compressed_tokens,
            turns_summarized,
        } => Event::default().event("context_compressed").data(
            serde_json::json!({
                "original_tokens": original_tokens, "compressed_tokens": compressed_tokens,
                "turns_summarized": turns_summarized,
            })
            .to_string(),
        ),
        StreamFrame::StepFinish {
            iteration,
            finish_reason,
        } => Event::default().event("step_finish").data(
            serde_json::json!({ "iteration": iteration, "finish_reason": finish_reason })
                .to_string(),
        ),
        StreamFrame::Error { code, message } => Event::default()
            .event("error")
            .data(serde_json::json!({ "code": code, "message": message }).to_string()),
    }
}

// ── shared SSE utilities ──────────────────────────────────────────────────────

/// Spawn an async forwarder that pumps `frame_rx` into a new SSE event channel,
/// then emits a terminal `done` event. `get_exit_code(saw_error)` is called
/// once after the stream closes to determine the done payload.
///
/// Returns the receiver stream ready to be wrapped in `Sse::new(...)`.
pub(super) fn spawn_frame_forwarder(
    mut frame_rx: tokio::sync::mpsc::Receiver<StreamFrame>,
    get_exit_code: impl FnOnce(bool) -> i32 + Send + 'static,
) -> ReceiverStream<Result<Event, Infallible>> {
    let (out_tx, out_rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);
    tokio::spawn(async move {
        let mut saw_error = false;
        loop {
            tokio::select! {
                maybe = frame_rx.recv() => match maybe {
                    Some(frame) => {
                        if matches!(frame, StreamFrame::Error { .. }) {
                            saw_error = true;
                        }
                        if out_tx.send(Ok(frame_to_sse(&frame))).await.is_err() {
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

// ── event → frame ─────────────────────────────────────────────────────────────

/// Map an SDK [`AgentEvent`] to a [`StreamFrame`], or `None` to suppress.
pub(super) fn agent_event_to_frame(
    event: &aikit_sdk::AgentEvent,
    agent_key: &str,
) -> Option<StreamFrame> {
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
            // CLI backends (codex, opencode) emit tool invocations as (Tool, Message).
            (MessageRole::Tool, MessageKind::Message, _) => Some(StreamFrame::ToolUse {
                name: msg.text.clone(),
                input: serde_json::Value::Null,
            }),
            (MessageRole::Tool, MessageKind::ToolOutput, _) => Some(StreamFrame::ToolResult {
                name: agent_key.to_string(),
                output: msg.text.clone(),
                is_error: false,
            }),
            (_, MessageKind::Reasoning, _) => Some(StreamFrame::Reasoning {
                content: msg.text.clone(),
            }),
            _ => None,
        },
        AgentEventPayload::ToolUse {
            tool_name, input, ..
        } => Some(StreamFrame::ToolUse {
            name: tool_name.clone(),
            input: input.clone(),
        }),
        AgentEventPayload::ToolResult {
            call_id,
            output,
            is_error,
        } => Some(StreamFrame::ToolResult {
            name: call_id.clone(),
            output: match output {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            },
            is_error: *is_error,
        }),
        AgentEventPayload::QuotaExceeded { info, .. } => Some(StreamFrame::Error {
            code: "quota_exceeded".to_string(),
            message: info.raw_message.clone(),
        }),
        AgentEventPayload::TokenUsageLine { usage, source, .. } => Some(StreamFrame::TokenUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            source: usage_source_name(source).to_string(),
        }),
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
        AgentEventPayload::AikitToolResult {
            call_id,
            output,
            is_error,
        } => Some(StreamFrame::ToolResult {
            name: call_id.clone(),
            output: output.clone(),
            is_error: *is_error,
        }),
        AgentEventPayload::AikitSubagentSpawn {
            subagent_id,
            workdir,
        } => Some(StreamFrame::SubagentSpawn {
            subagent_id: subagent_id.clone(),
            workdir: workdir.clone(),
        }),
        AgentEventPayload::AikitSubagentResult {
            subagent_id,
            status,
            changed_files,
            key_findings,
            ..
        } => Some(StreamFrame::SubagentResult {
            subagent_id: subagent_id.clone(),
            status: status.clone(),
            changed_files: changed_files.clone(),
            key_findings: key_findings.clone(),
        }),
        AgentEventPayload::AikitContextCompressed {
            original_tokens,
            compressed_tokens,
            turns_summarized,
        } => Some(StreamFrame::ContextCompressed {
            original_tokens: *original_tokens,
            compressed_tokens: *compressed_tokens,
            turns_summarized: *turns_summarized,
        }),
        AgentEventPayload::AikitStepFinish {
            iteration,
            finish_reason,
        } => Some(StreamFrame::StepFinish {
            iteration: *iteration,
            finish_reason: finish_reason.clone(),
        }),
        _ => None,
    }
}

fn usage_source_name(source: &UsageSource) -> &'static str {
    match source {
        UsageSource::Codex => "codex",
        UsageSource::Claude => "claude",
        UsageSource::Gemini => "gemini",
        UsageSource::OpenCode => "opencode",
        UsageSource::Cursor => "cursor",
        UsageSource::Aikit => "aikit",
    }
}

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
        StreamFrame::Reasoning { .. } => "reasoning",
        StreamFrame::ToolUse { .. } => "tool_use",
        StreamFrame::ToolResult { .. } => "tool_result",
        StreamFrame::TokenUsage { .. } => "token_usage",
        StreamFrame::SubagentSpawn { .. } => "subagent_spawn",
        StreamFrame::SubagentResult { .. } => "subagent_result",
        StreamFrame::ContextCompressed { .. } => "context_compressed",
        StreamFrame::StepFinish { .. } => "step_finish",
        StreamFrame::Error { .. } => "error",
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
            let saw_assistant_delta = Arc::new(Mutex::new(false));
            let saw_delta_for_cb = Arc::clone(&saw_assistant_delta);
            let agent_for_cb = agent.clone();

            let result = run_agent_events(&agent, &prompt, options, move |event| {
                let payload_kind = payload_kind_name(&event.payload);
                if let AgentEventPayload::StreamMessage(msg) = &event.payload {
                    // B5: capture first session_id from turn_id.
                    if let Some(ref tid) = msg.turn_id {
                        let mut cap = captured_for_cb.lock().unwrap();
                        if cap.is_none() {
                            *cap = Some(tid.clone());
                            drop(cap);
                            let _ = tx_cb.blocking_send(StreamFrame::Session {
                                session_id: tid.clone(),
                            });
                        }
                    }
                    // Dedup Final-after-Delta for non-aikit assistant messages.
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
                                "suppressing Final assistant StreamMessage"
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
                                error = %e,
                                "frame channel send failed"
                            );
                        }
                    }
                    None => {
                        tracing::trace!(
                            target: "aikit::serve::run",
                            agent = %agent_for_cb,
                            payload = payload_kind,
                            "SDK event suppressed"
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
    };

    let state = AppState {
        runs: Arc::new(Mutex::new(HashMap::new())),
        live_sessions: Arc::new(Mutex::new(HashMap::new())),
        config: config.clone(),
        run_fn,
        auth_cache: Arc::new(Mutex::new(None)),
    };

    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid address {}:{}: {}", config.host, config.port, e))?;

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
    if !addr.ip().is_loopback() {
        eprintln!(
            "Warning: server is bound to a non-loopback address. Set --api-key or restrict access via network ACLs."
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

    #[test]
    fn maps_external_tool_use_and_result() {
        let use_ev = event(AgentEventPayload::ToolUse {
            call_id: "tu_1".into(),
            tool_name: "Bash".into(),
            input: serde_json::json!({"command": "ls"}),
        });
        match agent_event_to_frame(&use_ev, "claude") {
            Some(StreamFrame::ToolUse { name, input }) => {
                assert_eq!(name, "Bash");
                assert_eq!(input["command"], "ls");
            }
            other => panic!("expected ToolUse frame, got {other:?}"),
        }

        let res_str = event(AgentEventPayload::ToolResult {
            call_id: "tu_1".into(),
            output: serde_json::json!("file.txt\n"),
            is_error: false,
        });
        match agent_event_to_frame(&res_str, "claude") {
            Some(StreamFrame::ToolResult {
                name,
                output,
                is_error,
            }) => {
                assert_eq!(name, "tu_1");
                assert_eq!(output, "file.txt\n");
                assert!(!is_error);
            }
            other => panic!("expected ToolResult frame, got {other:?}"),
        }

        let res_obj = event(AgentEventPayload::ToolResult {
            call_id: "tu_2".into(),
            output: serde_json::json!({"ok": true}),
            is_error: true,
        });
        match agent_event_to_frame(&res_obj, "claude") {
            Some(StreamFrame::ToolResult {
                output, is_error, ..
            }) => {
                assert_eq!(output, "{\"ok\":true}");
                assert!(is_error);
            }
            other => panic!("expected ToolResult frame, got {other:?}"),
        }
    }

    #[test]
    fn maps_token_usage_line() {
        let ev = event(AgentEventPayload::TokenUsageLine {
            usage: TokenUsage {
                input_tokens: 12,
                output_tokens: 34,
                total_tokens: Some(46),
                cache_read_tokens: Some(5),
                cache_creation_tokens: None,
                reasoning_tokens: None,
            },
            source: UsageSource::Aikit,
            raw_agent_line_seq: 0,
        });
        match agent_event_to_frame(&ev, "aikit") {
            Some(StreamFrame::TokenUsage {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                source,
            }) => {
                assert_eq!(input_tokens, 12);
                assert_eq!(output_tokens, 34);
                assert_eq!(cache_read_tokens, Some(5));
                assert_eq!(source, "aikit");
            }
            other => panic!("expected TokenUsage frame, got {other:?}"),
        }
    }

    #[test]
    fn maps_reasoning_stream_message() {
        let ev = event(AgentEventPayload::StreamMessage(StreamMessage {
            text: "thinking...".to_string(),
            phase: MessagePhase::Delta,
            role: MessageRole::Assistant,
            kind: aikit_sdk::MessageKind::Reasoning,
            source: AgentEventStream::Stdout,
            raw_line_seq: 0,
            turn_id: None,
        }));
        match agent_event_to_frame(&ev, "aikit") {
            Some(StreamFrame::Reasoning { content }) => assert_eq!(content, "thinking..."),
            other => panic!("expected Reasoning frame, got {other:?}"),
        }
    }

    #[test]
    fn maps_aikit_tool_result_is_error() {
        let ev = event(AgentEventPayload::AikitToolResult {
            call_id: "call-1".to_string(),
            output: "boom".to_string(),
            is_error: true,
        });
        match agent_event_to_frame(&ev, "aikit") {
            Some(StreamFrame::ToolResult {
                name,
                output,
                is_error,
            }) => {
                assert_eq!(name, "call-1");
                assert_eq!(output, "boom");
                assert!(is_error);
            }
            other => panic!("expected ToolResult frame, got {other:?}"),
        }
    }

    #[test]
    fn cli_tool_message_becomes_tool_use() {
        // codex emits (Tool, Message) for command text — should become ToolUse
        let ev = event(AgentEventPayload::StreamMessage(StreamMessage {
            text: "ls -la".to_string(),
            phase: MessagePhase::Delta,
            role: MessageRole::Tool,
            kind: aikit_sdk::MessageKind::Message,
            source: AgentEventStream::Stdout,
            raw_line_seq: 0,
            turn_id: None,
        }));
        match agent_event_to_frame(&ev, "codex") {
            Some(StreamFrame::ToolUse { name, input }) => {
                assert_eq!(name, "ls -la");
                assert_eq!(input, serde_json::Value::Null);
            }
            other => panic!("expected ToolUse frame, got {other:?}"),
        }
    }

    #[test]
    fn cli_tool_output_becomes_tool_result() {
        // codex/opencode emit (Tool, ToolOutput) for command output — should become ToolResult
        let ev = event(AgentEventPayload::StreamMessage(StreamMessage {
            text: "Cargo.toml\nsrc/".to_string(),
            phase: MessagePhase::Delta,
            role: MessageRole::Tool,
            kind: aikit_sdk::MessageKind::ToolOutput,
            source: AgentEventStream::Stdout,
            raw_line_seq: 0,
            turn_id: None,
        }));
        match agent_event_to_frame(&ev, "codex") {
            Some(StreamFrame::ToolResult {
                output, is_error, ..
            }) => {
                assert_eq!(output, "Cargo.toml\nsrc/");
                assert!(!is_error);
            }
            other => panic!("expected ToolResult frame, got {other:?}"),
        }
    }
}
