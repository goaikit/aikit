//! Bidirectional live-session handlers (`/api/v1/live-sessions`).
//!
//! Covers: Claude and Codex bidirectional sessions opened via `open_*_session`,
//! streamed as SSE, and driven via a control endpoint.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aikit_sdk::{
    open_claude_session, open_codex_session, ClaudeSessionError, ClaudeSessionOptions,
    CodexControlHandle, CodexSessionError, CodexSessionOptions, ControlHandle,
};
use uuid::Uuid;

use super::{
    agent_event_to_frame, error_response, spawn_frame_forwarder, sse_response_with_headers,
    AppState, StreamFrame,
};

// ── control abstraction ───────────────────────────────────────────────────────

/// Wraps either a [`ControlHandle`] (Claude) or [`CodexControlHandle`] (Codex)
/// so both can be stored in a unified registry.
pub(super) enum LiveSessionControl {
    Claude(ControlHandle),
    Codex(CodexControlHandle),
}

impl LiveSessionControl {
    pub(super) fn interrupt(&self) {
        match self {
            LiveSessionControl::Claude(h) => {
                let _ = h.interrupt();
            }
            LiveSessionControl::Codex(h) => {
                let _ = h.interrupt();
            }
        }
    }

    pub(super) fn disconnect(&self) {
        match self {
            LiveSessionControl::Claude(h) => {
                let _ = h.disconnect();
            }
            LiveSessionControl::Codex(h) => {
                let _ = h.disconnect();
            }
        }
    }

    /// Switch model mid-session. Returns `Err(response)` for backends that
    /// do not support this action.
    #[allow(clippy::result_large_err)]
    pub(super) fn try_set_model(&self, model: Option<String>) -> Result<(), Response> {
        match self {
            LiveSessionControl::Claude(h) => {
                let _ = h.set_model(model);
                Ok(())
            }
            LiveSessionControl::Codex(_) => Err(error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "not_supported",
                "set_model is only supported for Claude sessions",
            )),
        }
    }

    /// Send a follow-up turn on the same session. Both backends support this.
    pub(super) fn send_turn(&self, text: String) {
        match self {
            LiveSessionControl::Claude(h) => {
                let _ = h.send_turn(text);
            }
            LiveSessionControl::Codex(h) => {
                let _ = h.send_turn(text);
            }
        }
    }

    /// Get context-window usage (Claude only). Returns `Err(response)` for
    /// backends that do not support this action.
    #[allow(clippy::result_large_err)]
    pub(super) fn try_get_context_usage(&self) -> Result<serde_json::Value, Response> {
        match self {
            LiveSessionControl::Claude(h) => h.get_context_usage().map_err(|e| {
                error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "context_usage_error",
                    &e.to_string(),
                )
            }),
            LiveSessionControl::Codex(_) => Err(error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "not_supported",
                "get_context_usage is only supported for Claude sessions",
            )),
        }
    }
}

// ── registry types ────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum LiveSessionStatus {
    Active,
    Closed,
}

pub(super) struct LiveSessionRecord {
    pub session_id: String,
    pub agent_key: String,
    pub control: LiveSessionControl,
    pub status: LiveSessionStatus,
    pub created_at: DateTime<Utc>,
}

pub(super) type LiveSessions = Arc<Mutex<HashMap<String, LiveSessionRecord>>>;

// ── HTTP body types ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateLiveSessionRequest {
    pub agent: String,
    pub prompt: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub approval_policy: Option<String>,
    #[serde(default)]
    pub sandbox: Option<String>,
    #[serde(default)]
    pub mcp_servers: std::collections::BTreeMap<String, serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    pub fork_session: bool,
    #[serde(default)]
    pub resume: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LiveSessionControlRequest {
    action: String,
    #[serde(default)]
    model: Option<String>,
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

// ── handlers ──────────────────────────────────────────────────────────────────

/// `POST /api/v1/live-sessions` — create and stream a bidirectional session.
///
/// The `X-Session-Id` response header carries the session_id so the client can
/// associate it before reading the stream body.
pub(super) async fn create_live_session_handler(
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
    // BUG-7: `.is_empty()` let whitespace-only prompts through.
    if body.prompt.trim().is_empty() {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            "prompt must not be empty",
        );
    }

    // SEC-3: live sessions open a real bidirectional subprocess and, unlike
    // one-shot runs, are never bounded by a run timeout — enforce the same
    // `max_sessions` cap one-shot runs already have, or an unauthenticated
    // (or merely careless) caller can open unbounded long-lived agent
    // subprocesses (resource-exhaustion DoS). Checked before doing any of
    // the expensive session-opening work below.
    {
        let live = state.live_sessions.lock().unwrap();
        let active = live
            .values()
            .filter(|r| r.status == LiveSessionStatus::Active)
            .count();
        if active >= state.config.max_sessions {
            return error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "session_limit_reached",
                &format!(
                    "Maximum of {} concurrent live sessions reached",
                    state.config.max_sessions
                ),
            );
        }
    }

    let session_id = Uuid::new_v4().to_string();

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
            let opts = CodexSessionOptions::default()
                .with_approval_policy(body.approval_policy.clone())
                .with_sandbox(body.sandbox.clone());
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

    // Blocking forwarder: sync event channel → tokio frame channel → SSE.
    let (frame_tx, frame_rx) = tokio::sync::mpsc::channel::<StreamFrame>(64);
    let agent_key = body.agent.clone();
    let live_ref = Arc::clone(&state.live_sessions);
    let sid_for_cleanup = session_id.clone();
    let sid_for_frame = session_id.clone();
    tokio::task::spawn_blocking(move || {
        if frame_tx
            .blocking_send(StreamFrame::Session {
                session_id: sid_for_frame,
            })
            .is_err()
        {
            return;
        }
        while let Ok(event) = events_rx.recv() {
            if let Some(frame) = agent_event_to_frame(&event, &agent_key) {
                if frame_tx.blocking_send(frame).is_err() {
                    break;
                }
            }
        }
        if let Ok(mut live) = live_ref.lock() {
            if let Some(r) = live.get_mut(&sid_for_cleanup) {
                r.status = LiveSessionStatus::Closed;
            }
        }
    });

    let stream = spawn_frame_forwarder(frame_rx, |_| 0);
    sse_response_with_headers(stream, Some(("x-session-id", &session_id)))
}

/// `POST /api/v1/live-sessions/{session_id}/control` — send a control command.
pub(super) async fn live_session_control_handler(
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
            ok_action("interrupt")
        }
        "disconnect" => {
            record.control.disconnect();
            ok_action("disconnect")
        }
        "set_model" => match record.control.try_set_model(body.model.clone()) {
            Ok(_) => ok_action("set_model"),
            Err(resp) => resp,
        },
        "send_turn" => {
            let text = body.text.as_deref().unwrap_or("").trim().to_string();
            if text.is_empty() {
                return error_response(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_request",
                    "send_turn requires a non-empty 'text' field",
                );
            }
            record.control.send_turn(text);
            ok_action("send_turn")
        }
        "get_context_usage" => match record.control.try_get_context_usage() {
            Ok(usage) => (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                usage.to_string(),
            )
                .into_response(),
            Err(resp) => resp,
        },
        other => error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_action",
            &format!(
                "Unknown action '{other}'. Supported: interrupt, disconnect, set_model, send_turn, get_context_usage"
            ),
        ),
    }
}

/// `GET /api/v1/live-sessions`
pub(super) async fn list_live_sessions_handler(State(state): State<AppState>) -> impl IntoResponse {
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

/// `DELETE /api/v1/live-sessions/{session_id}`
pub(super) async fn delete_live_session_handler(
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

// ── helpers ───────────────────────────────────────────────────────────────────

fn ok_action(action: &'static str) -> Response {
    let body = serde_json::json!({ "ok": true, "action": action }).to_string();
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}
