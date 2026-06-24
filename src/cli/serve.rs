//! `aikit serve` — HTTP server for multi-turn agent sessions.
//!
//! Two response shapes share one endpoint, `POST /api/v1/messages`, selected by
//! the request's `Accept` header (HTTP content negotiation):
//! - `Accept: text/event-stream` → SSE. Event names: `session`, `text`,
//!   `reasoning`, `tool_use`, `tool_result`, `token_usage`, `subagent_spawn`,
//!   `subagent_result`, `context_compressed`, `step_finish`, `error`, then a
//!   terminal `done`. The reasoning/usage/sub-agent/compression/step-finish
//!   events bring the stream to parity with `aikit agent run --events`
//!   (NDJSON). Clients that only read the original `session/text/tool_*/error/
//!   done` set are unaffected — unknown event names are ignored.
//! - `Accept: application/json` → server runs to completion, accumulates the
//!   assistant text frames, sums any `token_usage` frames, and returns a single
//!   JSON body `{session_id, content, exit_code, error?, usage?}` where
//!   `usage = {input_tokens, output_tokens, cache_read_tokens?}` (omitted when
//!   the backend emitted no usage events).
//! - `Accept: */*`, missing, or both types present → SSE (default).
//! - Any other explicit media type → `406 Not Acceptable`.
//!
//! Session model:
//! - Sessions are created **implicitly** on the first `POST /api/v1/messages`
//!   call that omits `session_id`.
//! - In SSE mode the first frame is `event: session` carrying the new id; in
//!   the JSON shape the id appears in the response body. Subsequent calls
//!   quote that id in the request body to resume.
//! - For the `aikit` backend the id is assigned by the SDK (and persisted to
//!   `~/.aikit/sessions/...`). For other backends the id is treated as an
//!   opaque token forwarded to the underlying CLI's `--resume` flag.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{FromRequest, Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware;
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
    RunOptions, UsageSource,
};
use aikit_sdk::{
    open_claude_session, open_codex_session, ClaudeSessionError, ClaudeSessionOptions,
    CodexControlHandle, CodexSessionError, CodexSessionOptions, ControlHandle,
};

use cli_framework::api::{
    ApiServerBuilder, ApiVersion, ApiVersionName, DefaultVersion, ReadinessReport, Stability,
};
use cli_framework::tower::util::BoxCloneLayer;

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

// ── live session (bidirectional) ─────────────────────────────────────────────

/// Wraps either a [`ControlHandle`] (Claude) or a [`CodexControlHandle`] (Codex)
/// so both can be stored in a unified registry.
enum LiveSessionControl {
    Claude(ControlHandle),
    Codex(CodexControlHandle),
}

impl LiveSessionControl {
    fn interrupt(&self) {
        match self {
            LiveSessionControl::Claude(h) => {
                let _ = h.interrupt();
            }
            LiveSessionControl::Codex(h) => {
                let _ = h.interrupt();
            }
        }
    }

    fn disconnect(&self) {
        match self {
            LiveSessionControl::Claude(h) => {
                let _ = h.disconnect();
            }
            LiveSessionControl::Codex(h) => {
                let _ = h.disconnect();
            }
        }
    }
}

#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum LiveSessionStatus {
    Active,
    Closed,
}

struct LiveSessionRecord {
    session_id: String,
    agent_key: String,
    control: LiveSessionControl,
    status: LiveSessionStatus,
    created_at: DateTime<Utc>,
}

/// Registry of open bidirectional sessions, keyed by session_id.
type LiveSessions = Arc<Mutex<HashMap<String, LiveSessionRecord>>>;

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

/// Cached auth-probe results with their capture time. Probing spawns
/// subprocesses (one per CLI backend), so results are cached for
/// [`AUTH_CACHE_TTL`] to keep `GET /api/v1/agents` cheap on repeat calls.
type AuthCache = Arc<Mutex<Option<(Instant, HashMap<String, AuthStatus>)>>>;

#[derive(Clone)]
struct AppState {
    runs: Arc<Mutex<HashMap<String, RunRecord>>>,
    live_sessions: LiveSessions,
    config: ServeConfig,
    run_fn: RunFn,
    auth_cache: AuthCache,
}

// ── HTTP body types ───────────────────────────────────────────────────────────

/// B4: Custom extractor that wraps `Json<SendMessageRequest>` and maps
/// `JsonRejection` errors into the standard `{"error":{"code","message"}}`
/// envelope instead of the plain-text axum default.
struct JsonBody(SendMessageRequest);

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

// B8: deny_unknown_fields so unrecognised keys are rejected at parse time.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateLiveSessionRequest {
    agent: String,
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    /// Codex approval policy: `never`, `on-failure`, `on-request`, `untrusted`.
    #[serde(default)]
    approval_policy: Option<String>,
    /// Codex sandbox mode: `read-only`, `workspace-write`, `danger-full-access`.
    #[serde(default)]
    sandbox: Option<String>,
    /// Claude: MCP servers forwarded to `--mcp-config`. Keys = server names.
    #[serde(default)]
    mcp_servers: std::collections::BTreeMap<String, serde_json::Map<String, serde_json::Value>>,
    /// Claude: fork the resumed session into a new branch.
    #[serde(default)]
    fork_session: bool,
    /// Claude: resume an existing session by id.
    #[serde(default)]
    resume: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveSessionControlRequest {
    /// Action: `interrupt`, `set_model`, `send_turn`, `disconnect`.
    action: String,
    /// Model to switch to; required when `action = "set_model"`.
    #[serde(default)]
    model: Option<String>,
    /// Prompt for the next turn; required when `action = "send_turn"`.
    #[serde(default)]
    text: Option<String>,
}

#[derive(Serialize)]
struct LiveSessionSummary {
    session_id: String,
    agent: String,
    status: LiveSessionStatus,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct ListLiveSessionsResponse {
    sessions: Vec<LiveSessionSummary>,
}

#[derive(Serialize)]
struct DeleteLiveSessionResponse {
    session_id: String,
    status: LiveSessionStatus,
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

/// Per-backend authentication status reported on `GET /api/v1/agents`.
///
/// Distinct from `available` (which means "binary on PATH"): `auth` reports
/// whether the backend is actually authenticated and ready to run a turn.
/// - `Ok` — credentials present (env var set, or the backend's status command
///   exited 0).
/// - `Unauthenticated` — the backend's status command exited cleanly non-zero
///   (logged out / invalid credentials).
/// - `Unknown` — no reliable non-interactive probe (e.g. `opencode`), the probe
///   timed out, the spawn failed, or the env var was unset for an env-only
///   backend (e.g. `gemini`).
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AuthStatus {
    Ok,
    Unauthenticated,
    Unknown,
}

#[derive(Serialize)]
struct AgentInfo {
    key: String,
    name: String,
    /// Binary is on PATH (or, for `aikit`, always true — it's built in).
    available: bool,
    /// Whether the backend is authenticated. See [`AuthStatus`]. `available`
    /// backends with no probe mechanism report `unknown`.
    auth: AuthStatus,
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
    /// Aggregated token usage accumulated from `TokenUsage` frames seen during
    /// the run. `None` when the backend emitted no usage events.
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<UsageSummary>,
}

/// Aggregated token usage for a single sync run.
#[derive(Serialize)]
struct UsageSummary {
    input_tokens: u64,
    output_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_read_tokens: Option<u64>,
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
        StreamFrame::Reasoning { content } => {
            let data = serde_json::json!({ "content": content }).to_string();
            Event::default().event("reasoning").data(data)
        }
        StreamFrame::ToolUse { name, input } => {
            let data = serde_json::json!({ "name": name, "input": input }).to_string();
            Event::default().event("tool_use").data(data)
        }
        StreamFrame::ToolResult {
            name,
            output,
            is_error,
        } => {
            let data = serde_json::json!({ "name": name, "output": output, "is_error": is_error })
                .to_string();
            Event::default().event("tool_result").data(data)
        }
        StreamFrame::TokenUsage {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            source,
        } => {
            let data = serde_json::json!({
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "cache_read_tokens": cache_read_tokens,
                "source": source,
            })
            .to_string();
            Event::default().event("token_usage").data(data)
        }
        StreamFrame::SubagentSpawn {
            subagent_id,
            workdir,
        } => {
            let data =
                serde_json::json!({ "subagent_id": subagent_id, "workdir": workdir }).to_string();
            Event::default().event("subagent_spawn").data(data)
        }
        StreamFrame::SubagentResult {
            subagent_id,
            status,
            changed_files,
            key_findings,
        } => {
            let data = serde_json::json!({
                "subagent_id": subagent_id,
                "status": status,
                "changed_files": changed_files,
                "key_findings": key_findings,
            })
            .to_string();
            Event::default().event("subagent_result").data(data)
        }
        StreamFrame::ContextCompressed {
            original_tokens,
            compressed_tokens,
            turns_summarized,
        } => {
            let data = serde_json::json!({
                "original_tokens": original_tokens,
                "compressed_tokens": compressed_tokens,
                "turns_summarized": turns_summarized,
            })
            .to_string();
            Event::default().event("context_compressed").data(data)
        }
        StreamFrame::StepFinish {
            iteration,
            finish_reason,
        } => {
            let data =
                serde_json::json!({ "iteration": iteration, "finish_reason": finish_reason })
                    .to_string();
            Event::default().event("step_finish").data(data)
        }
        StreamFrame::Error { code, message } => {
            let data = serde_json::json!({ "code": code, "message": message }).to_string();
            Event::default().event("error").data(data)
        }
    }
}

// ── handlers ─────────────────────────────────────────────────────────────────

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
                // Filled in by `agents_handler` from the auth probe cache.
                auth: AuthStatus::Unknown,
            }
        })
        .collect();

    agents.sort_by(|a, b| a.key.cmp(&b.key));
    agents
}

// ── per-backend auth probe (E2) ───────────────────────────────────────────────

/// TTL for cached auth-probe results.
const AUTH_CACHE_TTL: Duration = Duration::from_secs(60);

/// Hard timeout for a single subprocess auth probe. Probes that don't return
/// within this window are treated as `Unknown` (never block the handler).
const AUTH_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Probe a single backend's authentication status.
///
/// Detection per backend (verified live):
/// - `aikit`  — env check only: `OPENAI_API_KEY` or `AIKIT_API_KEY` set &
///   non-empty (mirrors `aikit-agent`'s `resolve_api_key`).
/// - `codex`  — `codex login status`, exit 0 → ok.
/// - `cursor` — `agent status` exit 0, OR `CURSOR_API_KEY` set & non-empty
///   (Cursor's spawn binary is `agent`).
/// - `claude` — `claude auth status`, exit 0 → ok. CAVEAT: can report ok even
///   when a headless `claude -p` run fails (an invalid `ANTHROPIC_API_KEY` can
///   override valid OAuth).
/// - `gemini` — env check ONLY: `GEMINI_API_KEY` or `GOOGLE_API_KEY`, else
///   `unknown`. The `gemini` status command HANGS in non-interactive mode and
///   is NEVER spawned.
/// - anything else (e.g. `opencode`) — `unknown` (no reliable probe).
async fn probe_backend_auth(key: &str) -> AuthStatus {
    match key {
        "aikit" => env_auth(&["OPENAI_API_KEY", "AIKIT_API_KEY"]),
        "gemini" => env_auth(&["GEMINI_API_KEY", "GOOGLE_API_KEY"]),
        "codex" => spawn_status_probe("codex", &["login", "status"]).await,
        "claude" => spawn_status_probe("claude", &["auth", "status"]).await,
        "cursor" => {
            // Cursor: env var OR `agent status` exit 0 (spawn binary is `agent`).
            if env_auth(&["CURSOR_API_KEY"]) == AuthStatus::Ok {
                AuthStatus::Ok
            } else {
                spawn_status_probe("agent", &["status"]).await
            }
        }
        _ => AuthStatus::Unknown,
    }
}

/// Pure env check: `Ok` when any of `vars` is set & non-empty, else `Unknown`.
/// (Absence of an env var doesn't prove "logged out", only "not via env".)
fn env_auth(vars: &[&str]) -> AuthStatus {
    for v in vars {
        if std::env::var(v).map(|s| !s.is_empty()).unwrap_or(false) {
            return AuthStatus::Ok;
        }
    }
    AuthStatus::Unknown
}

/// Spawn `bin args...` as a status preflight with a hard timeout.
/// - exit 0 → `Ok`
/// - clean non-zero exit → `Unauthenticated`
/// - spawn error or timeout → `Unknown`
///
/// stdout/stderr are discarded (piped to null); the process is never inherited.
async fn spawn_status_probe(bin: &str, args: &[&str]) -> AuthStatus {
    use std::process::Stdio;

    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let fut = cmd.status();
    match tokio::time::timeout(AUTH_PROBE_TIMEOUT, fut).await {
        Ok(Ok(status)) if status.success() => AuthStatus::Ok,
        Ok(Ok(_)) => AuthStatus::Unauthenticated,
        // spawn failed (binary missing, etc.) or non-zero-with-no-code.
        Ok(Err(_)) => AuthStatus::Unknown,
        // timed out — kill best-effort by dropping; report unknown.
        Err(_) => AuthStatus::Unknown,
    }
}

/// Probe the given backends concurrently and return a key→status map. Worst
/// case is bounded by [`AUTH_PROBE_TIMEOUT`] (~5s), not the sum of all probes.
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

/// Return cached auth statuses if still within TTL, otherwise probe all
/// `available` backends concurrently and refresh the cache.
async fn auth_statuses(state: &AppState, keys: &[String]) -> HashMap<String, AuthStatus> {
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

async fn agents_handler(State(state): State<AppState>) -> impl IntoResponse {
    let mut agents = build_runnable_agents();
    let keys: Vec<String> = agents.iter().map(|a| a.key.clone()).collect();
    let statuses = auth_statuses(&state, &keys).await;
    for a in &mut agents {
        a.auth = statuses.get(&a.key).copied().unwrap_or(AuthStatus::Unknown);
    }
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
    // B3: find and close ALL non-closed records matching this session_id, not
    // just the first one (a multi-turn session has one record per turn).
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
                // B6: CLI backends have no persistent store; an unknown
                // session_id cannot be resumed.
                return Some(error_response(
                    StatusCode::NOT_FOUND,
                    "session_not_found",
                    &format!("Session '{}' not found", sid),
                ));
            }
        }
    }

    // session_busy and max_sessions are checked atomically inside spawn_run (B9).
    None
}

/// Classify a backend failure's stderr/content into a stable error code.
///
/// E2: when the text matches a known authentication-failure signature
/// (case-insensitive substring), return `"unauthenticated"`; otherwise the
/// generic `"agent_error"`. The message text is preserved by callers as-is.
fn classify_error_code(stderr_or_content: &str) -> &'static str {
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
    // "please run ... login" (the two tokens may be separated by other text).
    if hay.contains("please run") && hay.contains("login") {
        return "unauthenticated";
    }
    "agent_error"
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
#[allow(clippy::result_large_err)]
fn spawn_run(
    state: &AppState,
    body: &SendMessageRequest,
) -> Result<(tokio::sync::mpsc::Receiver<StreamFrame>, String), Response> {
    let request_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    {
        let mut runs = state.runs.lock().unwrap();
        // B9: atomic busy-check + capacity-check + insert under one lock,
        // closing the TOCTOU window that existed when validate_request and
        // spawn_run held separate locks.
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

    // B11: Use a relay channel. The run_fn sends to `inner_tx`; a relay task
    // intercepts Session frames to update the record immediately (so concurrent
    // requests can find the session_id before the run completes), then forwards
    // all frames to `outer_tx` → `rx` (which goes to the response handlers).
    let (inner_tx, mut inner_rx) = tokio::sync::mpsc::channel::<StreamFrame>(64);
    let (outer_tx, rx) = tokio::sync::mpsc::channel::<StreamFrame>(64);
    let relay_runs = Arc::clone(&runs_ref);
    let relay_request_id = request_id.clone();
    let relay_outer_tx = outer_tx.clone();
    tokio::spawn(async move {
        while let Some(frame) = inner_rx.recv().await {
            // Immediately write session_id into the record when first seen so
            // that concurrent requests can find it before the run finishes.
            if let StreamFrame::Session { ref session_id } = frame {
                let mut runs = relay_runs.lock().unwrap();
                if let Some(r) = runs.get_mut(&relay_request_id) {
                    if r.session_id.is_none() {
                        r.session_id = Some(session_id.clone());
                    }
                }
            }
            if relay_outer_tx.send(frame).await.is_err() {
                break; // client disconnected
            }
        }
    });
    // `tx` is the sender the async task uses to detect client-disconnect.
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

        // B12: wrap blocking_handle in a server-level wall-clock timeout so the
        // native aikit in-process path is bounded even if the SDK doesn't cancel
        // internally. The subprocess path also benefits from the extra guarantee.
        let outcome: Result<RunFnOutcome, RunError> = tokio::select! {
            timed_out = tokio::time::timeout(timeout, blocking_handle) => {
                match timed_out {
                    // Wall-clock timeout elapsed before the blocking task finished.
                    Err(_elapsed) => {
                        // Abort the blocking task so it doesn't keep running.
                        // (The JoinHandle was consumed by timeout; we already have
                        // the abort_handle stored on the record via the outer
                        // task's AbortHandle — but aborting spawn_blocking is
                        // best-effort. We still signal the error to the client.)
                        let _ = tx
                            .send(StreamFrame::Error {
                                code: "run_timeout".to_string(),
                                message: "Run exceeded timeout".to_string(),
                            })
                            .await;
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
                    // Blocking task finished within the timeout.
                    Ok(join_result) => match join_result {
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
                    },
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
                {
                    let mut runs = runs_ref.lock().unwrap();
                    if let Some(r) = runs.get_mut(&request_id_clone) {
                        // B11: backfill session_id from the outcome if we don't
                        // have one yet (fresh session whose id was assigned by
                        // the SDK).
                        if r.session_id.is_none() {
                            r.session_id = out.session_id.clone();
                        }
                        r.stderr_tail = out.stderr_tail.clone();
                        r.last_exit_code = Some(out.exit_code);
                    }
                }
                // E2: on a non-zero exit with captured stderr, emit an explicit
                // Error frame so SSE clients see a terminal error (not just a
                // `done` exit code). The code is normalized to "unauthenticated"
                // for known auth-failure signatures, else "agent_error". (Sync
                // mode would synthesize the same from the record; emitting it
                // here keeps both paths consistent.)
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

            // B2: after each run completes, prune stale records: remove all
            // `Closed` records and any `Idle` records older than 1 hour.
            let one_hour_ago = Utc::now() - chrono::Duration::hours(1);
            runs.retain(|_, r| {
                r.status != RunStatus::Closed
                    && !(r.status == RunStatus::Idle && r.last_active_at < one_hour_ago)
            });
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

    Ok((rx, request_id))
}

async fn messages_handler(
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
    runs: Arc<Mutex<HashMap<String, RunRecord>>>,
    request_id: String,
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
        // B1: read the real exit code from the record; fall back to saw_error
        // only when no code was recorded (e.g. run aborted before completion).
        let exit_code = {
            let runs_guard = runs.lock().unwrap();
            runs_guard
                .get(&request_id)
                .and_then(|r| r.last_exit_code)
                .unwrap_or(if saw_error { 1 } else { 0 })
        };
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
    // Accumulated usage across all TokenUsage frames. `seen` distinguishes
    // "no usage events" (→ omit the field) from a genuine all-zero run.
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
            StreamFrame::Text { content: c } => {
                content.push_str(&c);
            }
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
                    error = Some(ErrorDetail { code, message });
                }
            }
            // Reasoning, tool, sub-agent, compression, and step-finish frames
            // are intentionally dropped in sync mode — clients that need that
            // visibility should use stream mode.
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
    //   - If an explicit Error frame arrived → use the recorded exit code when
    //     present (e.g. an agent that exited 2), else 1 (e.g. a RunError that
    //     carries no process exit code).
    //   - Else use the recorded exit code (0 if missing).
    //   - B7: when exit_code != 0, ALWAYS populate `error` (using the explicit
    //     error frame if one arrived, otherwise synthesize from stderr/exit
    //     code). `content` is left as-is — error info must not be non-
    //     deterministically placed in either `content` or `error`.
    let (exit_code, error) = if let Some(e) = error {
        let code = record_exit.filter(|c| *c != 0).unwrap_or(1);
        (code, Some(e))
    } else {
        let code = record_exit.unwrap_or(0);
        if code != 0 {
            let message = if !record_stderr.is_empty() {
                record_stderr.clone()
            } else if !content.is_empty() {
                // There was text content but no stderr — surface the exit code.
                format!("Agent exited with code {}", code)
            } else {
                format!("Agent exited with code {}", code)
            };
            (
                code,
                Some(ErrorDetail {
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

// ── live session handlers ─────────────────────────────────────────────────────

/// `POST /api/v1/live-sessions` — create a bidirectional Claude or Codex
/// session and stream its events as SSE.
///
/// The `X-Session-Id` response header carries the session_id before the first
/// SSE frame so clients can associate it even before reading the stream.
/// The first SSE frame is also `event: session` with `{"session_id": ...}`.
async fn create_live_session_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateLiveSessionRequest>,
) -> Response {
    if body.agent.trim().is_empty() {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            "agent field is required",
        );
    }
    if body.prompt.is_empty() {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            "prompt must not be empty",
        );
    }

    let session_id = Uuid::new_v4().to_string();

    // Open the session synchronously — connect/handshake errors surface here.
    let (control, events_rx) = match body.agent.as_str() {
        "claude" => {
            let opts = ClaudeSessionOptions {
                model: body.model.clone(),
                resume: body.resume.clone(),
                fork_session: body.fork_session,
                mcp_servers: body.mcp_servers.clone(),
                ..ClaudeSessionOptions::default()
            };
            match open_claude_session(&body.prompt, opts) {
                Ok(s) => {
                    let (ctrl, evts) = s.into_parts();
                    (LiveSessionControl::Claude(ctrl), evts)
                }
                Err(ClaudeSessionError::Connect(msg)) => {
                    return error_response(
                        StatusCode::BAD_GATEWAY,
                        "session_connect_failed",
                        &format!("Failed to connect to claude: {msg}"),
                    );
                }
                Err(e) => {
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "session_error",
                        &e.to_string(),
                    );
                }
            }
        }
        "codex" => {
            let default_opts = CodexSessionOptions::default();
            let opts = CodexSessionOptions {
                approval_policy: body
                    .approval_policy
                    .clone()
                    .unwrap_or(default_opts.approval_policy),
                sandbox: body.sandbox.clone().unwrap_or(default_opts.sandbox),
                ..default_opts
            };
            match open_codex_session(&body.prompt, opts) {
                Ok(s) => {
                    let (ctrl, evts) = s.into_parts();
                    (LiveSessionControl::Codex(ctrl), evts)
                }
                Err(CodexSessionError::Connect(msg)) => {
                    return error_response(
                        StatusCode::BAD_GATEWAY,
                        "session_connect_failed",
                        &format!("Failed to connect to codex: {msg}"),
                    );
                }
                Err(e) => {
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "session_error",
                        &e.to_string(),
                    );
                }
            }
        }
        other => {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "agent_not_supported",
                &format!("Live sessions require agent 'claude' or 'codex', got '{other}'"),
            );
        }
    };

    // Register the session in the live registry.
    {
        let mut live = state.live_sessions.lock().unwrap();
        live.insert(
            session_id.clone(),
            LiveSessionRecord {
                session_id: session_id.clone(),
                agent_key: body.agent.clone(),
                control,
                status: LiveSessionStatus::Active,
                created_at: Utc::now(),
            },
        );
    }

    // Spawn a blocking forwarder: sync mpsc → tokio mpsc → SSE.
    let (frame_tx, frame_rx) = tokio::sync::mpsc::channel::<StreamFrame>(64);
    let agent_key = body.agent.clone();
    let live_ref = Arc::clone(&state.live_sessions);
    let sid_for_cleanup = session_id.clone();
    let sid_for_frame = session_id.clone();
    tokio::task::spawn_blocking(move || {
        // First frame: announce the session_id.
        if frame_tx
            .blocking_send(StreamFrame::Session {
                session_id: sid_for_frame,
            })
            .is_err()
        {
            return;
        }
        // Forward agent events until the session closes or the client disconnects.
        while let Ok(event) = events_rx.recv() {
            if let Some(frame) = agent_event_to_frame(&event, &agent_key) {
                if frame_tx.blocking_send(frame).is_err() {
                    break; // client disconnected
                }
            }
        }
        // Mark session closed in the registry.
        if let Ok(mut live) = live_ref.lock() {
            if let Some(r) = live.get_mut(&sid_for_cleanup) {
                r.status = LiveSessionStatus::Closed;
            }
        }
    });

    // Build the SSE response with the session_id header.
    let sid_header = session_id.clone();
    let (out_tx, out_rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(64);
    let mut frame_rx = frame_rx;
    tokio::spawn(async move {
        loop {
            tokio::select! {
                maybe = frame_rx.recv() => {
                    match maybe {
                        Some(frame) => {
                            if out_tx.send(Ok(frame_to_sse(&frame))).await.is_err() {
                                return;
                            }
                        }
                        None => break,
                    }
                }
                _ = out_tx.closed() => return,
            }
        }
        let data = serde_json::json!({ "exit_code": 0 }).to_string();
        let _ = out_tx
            .send(Ok(Event::default().event("done").data(data)))
            .await;
    });

    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    );
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    if let Ok(val) = HeaderValue::from_str(&sid_header) {
        headers.insert("x-session-id", val);
    }

    (headers, Sse::new(ReceiverStream::new(out_rx))).into_response()
}

/// `POST /api/v1/live-sessions/{session_id}/control` — send a control command.
async fn live_session_control_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<LiveSessionControlRequest>,
) -> impl IntoResponse {
    let live = state.live_sessions.lock().unwrap();
    let Some(record) = live.get(&session_id) else {
        return error_response(
            StatusCode::NOT_FOUND,
            "session_not_found",
            "Live session not found",
        );
    };
    if record.status == LiveSessionStatus::Closed {
        return error_response(
            StatusCode::GONE,
            "session_closed",
            "Live session is already closed",
        );
    }
    match body.action.as_str() {
        "interrupt" => {
            record.control.interrupt();
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                r#"{"ok":true,"action":"interrupt"}"#,
            )
                .into_response()
        }
        "disconnect" => {
            record.control.disconnect();
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                r#"{"ok":true,"action":"disconnect"}"#,
            )
                .into_response()
        }
        "set_model" => {
            // Only supported for Claude sessions.
            match &record.control {
                LiveSessionControl::Claude(h) => {
                    let _ = h.set_model(body.model.clone());
                    (
                        StatusCode::OK,
                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                        r#"{"ok":true,"action":"set_model"}"#,
                    )
                        .into_response()
                }
                LiveSessionControl::Codex(_) => error_response(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "not_supported",
                    "set_model is only supported for Claude sessions",
                ),
            }
        }
        "send_turn" => {
            let text = body.text.as_deref().unwrap_or("").trim().to_string();
            if text.is_empty() {
                return error_response(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_request",
                    "send_turn requires a non-empty 'text' field",
                );
            }
            match &record.control {
                LiveSessionControl::Claude(h) => {
                    let _ = h.send_turn(text);
                }
                LiveSessionControl::Codex(h) => {
                    let _ = h.send_turn(text);
                }
            }
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                r#"{"ok":true,"action":"send_turn"}"#,
            )
                .into_response()
        }
        other => error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_action",
            &format!(
                "Unknown action '{other}'. Supported: interrupt, disconnect, set_model, send_turn"
            ),
        ),
    }
}

/// `GET /api/v1/live-sessions` — list active live sessions.
async fn list_live_sessions_handler(State(state): State<AppState>) -> impl IntoResponse {
    let live = state.live_sessions.lock().unwrap();
    let sessions: Vec<LiveSessionSummary> = live
        .values()
        .filter(|r| r.status != LiveSessionStatus::Closed)
        .map(|r| LiveSessionSummary {
            session_id: r.session_id.clone(),
            agent: r.agent_key.clone(),
            status: r.status.clone(),
            created_at: r.created_at,
        })
        .collect();
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&ListLiveSessionsResponse { sessions }).unwrap_or_default(),
    )
}

/// `DELETE /api/v1/live-sessions/{session_id}` — disconnect a live session.
async fn delete_live_session_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let mut live = state.live_sessions.lock().unwrap();
    let Some(record) = live.get_mut(&session_id) else {
        return error_response(
            StatusCode::NOT_FOUND,
            "session_not_found",
            "Live session not found",
        );
    };
    record.control.disconnect();
    record.status = LiveSessionStatus::Closed;
    let resp = DeleteLiveSessionResponse {
        session_id,
        status: LiveSessionStatus::Closed,
    };
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&resp).unwrap_or_default(),
    )
        .into_response()
}

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/agents", get(agents_handler))
        .route("/messages", post(messages_handler))
        .route("/sessions", get(list_runs_handler))
        .route("/sessions/{session_id}", get(get_run_handler))
        .route("/sessions/{session_id}", delete(delete_run_handler))
        .route("/live-sessions", post(create_live_session_handler))
        .route("/live-sessions", get(list_live_sessions_handler))
        .route(
            "/live-sessions/{session_id}/control",
            post(live_session_control_handler),
        )
        .route(
            "/live-sessions/{session_id}",
            delete(delete_live_session_handler),
        )
        .with_state(state)
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
                if let AgentEventPayload::StreamMessage(msg) = &event.payload {
                    // B5: CLI backends (claude, gemini) embed session_id in
                    // StreamMessage.turn_id. Capture the first non-None value
                    // and emit a Session frame so callers can resume.
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
                // The `StreamMessage` ToolOutput path carries no error flag.
                is_error: false,
            }),
            // E1/B13: model reasoning ("thinking") for any role/phase.
            (_, MessageKind::Reasoning, _) => Some(StreamFrame::Reasoning {
                content: msg.text.clone(),
            }),
            _ => None,
        },
        // Structured tool calls from external backends (e.g. Claude via the
        // typed decoder). The in-process aikit agent uses the `Aikit*` variants
        // below; these are the engine-agnostic equivalents.
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
            // `output` is JSON; unwrap a bare string, else compact-encode.
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
        // E1/B13: token usage (input/output/cache tokens) for metering.
        AgentEventPayload::TokenUsageLine { usage, source, .. } => Some(StreamFrame::TokenUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            source: usage_source_name(source).to_string(),
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
        // B10: map AikitToolResult → tool_result frame. The variant has
        // `call_id` (not tool_name) and `output`.
        AgentEventPayload::AikitToolResult {
            call_id,
            output,
            is_error,
        } => Some(StreamFrame::ToolResult {
            name: call_id.clone(),
            output: output.clone(),
            is_error: *is_error,
        }),
        // E1/B13: sub-agent, context-compression, and step-finish visibility
        // for the built-in aikit agent.
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

/// Stable lowercase name for a `UsageSource`, used as the `source` field of a
/// `token_usage` frame.
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
        let bearer_layer = BoxCloneLayer::new(middleware::from_fn(
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

    // Spawn a task that aborts all in-flight RunFn handles when the framework
    // signals shutdown. The token fires ~200 ms before axum's graceful-drain
    // starts, giving SSE handlers time to observe the abort and close cleanly.
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
        // Generic (external-backend) tool frames, e.g. from Claude's decoder.
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

        // String output is unwrapped (not JSON-quoted).
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

        // Structured output is compact-encoded.
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
            kind: MessageKind::Reasoning,
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
        // Each documented auth-failure signature → "unauthenticated".
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
}
