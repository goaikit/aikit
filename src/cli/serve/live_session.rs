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
    CodexSessionError, CodexSessionOptions, ControlError, LiveSession,
};
use uuid::Uuid;

use super::{
    error_response, spawn_frame_forwarder, sse_response_with_headers, AppState, ServeEvent,
};

// ── control-error mapping ─────────────────────────────────────────────────────

/// Map a [`ControlError`] from a [`LiveSession`] operation onto an HTTP response.
///
/// `Unsupported` (e.g. `set_model` on Codex) → 422 `not_supported`; a backend
/// error → 500 with the caller-supplied `backend_code`. The per-backend match
/// this replaced is gone: the `LiveSession` trait already reports "unsupported"
/// uniformly via its default methods.
#[allow(clippy::result_large_err)]
fn control_error_response(e: ControlError, backend_code: &'static str) -> Response {
    match e {
        ControlError::Unsupported(op) => error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "not_supported",
            &format!("{op} is not supported by this agent"),
        ),
        ControlError::Backend(msg) => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, backend_code, &msg)
        }
    }
}

// ── registry types ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum LiveSessionStatus {
    Active,
    Closed,
}

pub(super) struct LiveSessionRecord {
    pub session_id: String,
    pub agent_key: String,
    pub control: Box<dyn LiveSession>,
    pub status: LiveSessionStatus,
    pub created_at: DateTime<Utc>,
}

pub(super) type LiveSessions = Arc<Mutex<HashMap<String, LiveSessionRecord>>>;

/// RAII guard for a reserved live-session slot (SEC-3). Incrementing happens under the
/// `live_sessions` lock together with the capacity check; this guard releases the reservation
/// on every exit path (including the many early `return`s in the open path) via `Drop`. On the
/// success path the inserted `Active` record supersedes the reservation.
struct PendingReservation(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for PendingReservation {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Reserve one live-session slot if capacity allows, returning a guard that releases it on
/// drop. Returns `None` when `active` (existing sessions) plus already-reserved in-flight
/// opens would meet or exceed `max`.
///
/// The caller MUST hold the `live_sessions` lock across this call: that lock is what makes the
/// load-check-increment atomic, so concurrent opens cannot all observe spare capacity and
/// overshoot `max` (SEC-3 TOCTOU).
fn try_reserve_live_slot(
    active: usize,
    pending: &Arc<std::sync::atomic::AtomicUsize>,
    max: usize,
) -> Option<PendingReservation> {
    use std::sync::atomic::Ordering;
    if active + pending.load(Ordering::Relaxed) >= max {
        return None;
    }
    pending.fetch_add(1, Ordering::Relaxed);
    Some(PendingReservation(pending.clone()))
}

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
    // subprocesses (resource-exhaustion DoS).
    //
    // The check and the slot reservation happen under a single `live_sessions`
    // lock: counting active records plus already-reserved (in-flight) opens, then
    // incrementing the reservation before releasing the lock. Without this, N
    // concurrent requests could all pass the check while the lock is dropped for
    // the expensive open below, then each insert its record — overshooting the cap
    // (a TOCTOU race). `_reservation` releases the slot on every exit path.
    let _reservation = {
        let live = state.live_sessions.lock().unwrap();
        let active = live
            .values()
            .filter(|r| r.status == LiveSessionStatus::Active)
            .count();
        match try_reserve_live_slot(
            active,
            &state.pending_live_sessions,
            state.config.max_sessions,
        ) {
            Some(guard) => guard,
            None => {
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
    };

    let session_id = Uuid::new_v4().to_string();

    let (control, events_rx): (Box<dyn LiveSession>, _) = match body.agent.as_str() {
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
                    (Box::new(ctrl), evts)
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
                    (Box::new(ctrl), evts)
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

    // Blocking forwarder: sync event channel → tokio item channel → SSE.
    // ARCH-4 / ADR 0016: every canonical `AgentEvent` from the underlying
    // session engine is forwarded as-is — no serve-side re-map — so
    // tool/reasoning/usage/subagent/compression/step-finish events reach the
    // client with their native shape instead of being lossily squashed or
    // dropped.
    let (item_tx, item_rx) = tokio::sync::mpsc::channel::<ServeEvent>(64);
    let agent_key = body.agent.clone();
    let live_ref = Arc::clone(&state.live_sessions);
    let sid_for_cleanup = session_id.clone();
    let sid_for_session_event = session_id.clone();
    tokio::task::spawn_blocking(move || {
        // Synthetic first event: the locally-minted live-session id, so
        // clients have a stable, resolvable id before any backend events
        // arrive (mirrors the one-shot run path's B5 behaviour).
        let synthetic = aikit_sdk::AgentEvent {
            agent_key,
            seq: 0,
            stream: aikit_sdk::AgentEventStream::Stdout,
            payload: aikit_sdk::AgentEventPayload::SessionStarted {
                session_id: sid_for_session_event,
            },
        };
        if item_tx.blocking_send(ServeEvent::Agent(synthetic)).is_err() {
            return;
        }
        while let Ok(event) = events_rx.recv() {
            if item_tx.blocking_send(ServeEvent::Agent(event)).is_err() {
                break;
            }
        }
        if let Ok(mut live) = live_ref.lock() {
            if let Some(r) = live.get_mut(&sid_for_cleanup) {
                r.status = LiveSessionStatus::Closed;
            }
        }
    });

    let stream = spawn_frame_forwarder(item_rx, |_| 0);
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
            let _ = record.control.interrupt();
            ok_action("interrupt")
        }
        "disconnect" => {
            let _ = record.control.disconnect();
            ok_action("disconnect")
        }
        "set_model" => match record.control.set_model(body.model.clone()) {
            Ok(_) => ok_action("set_model"),
            Err(e) => control_error_response(e, "session_error"),
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
            let _ = record.control.send_turn(text);
            ok_action("send_turn")
        }
        "get_context_usage" => match record.control.get_context_usage() {
            Ok(usage) => (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                usage.to_string(),
            )
                .into_response(),
            Err(e) => control_error_response(e, "context_usage_error"),
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
    let _ = record.control.disconnect();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex as StdMutex;

    #[test]
    fn try_reserve_live_slot_respects_capacity() {
        let pending = Arc::new(AtomicUsize::new(0));
        // active=0, max=2: first two reservations succeed, third is refused.
        let g1 = try_reserve_live_slot(0, &pending, 2);
        let g2 = try_reserve_live_slot(0, &pending, 2);
        assert!(g1.is_some() && g2.is_some());
        assert_eq!(pending.load(Ordering::Relaxed), 2);
        assert!(try_reserve_live_slot(0, &pending, 2).is_none());
        // Dropping a guard frees its slot.
        drop(g1);
        assert_eq!(pending.load(Ordering::Relaxed), 1);
        assert!(try_reserve_live_slot(0, &pending, 2).is_some());
    }

    #[test]
    fn try_reserve_live_slot_counts_existing_active_sessions() {
        let pending = Arc::new(AtomicUsize::new(0));
        // One session already Active, max=1 → no spare capacity.
        assert!(try_reserve_live_slot(1, &pending, 1).is_none());
        assert_eq!(pending.load(Ordering::Relaxed), 0);
    }

    // SEC-3: the TOCTOU fix. Many threads race to reserve a slot; the reservation must be
    // performed while holding the same lock as the capacity check (here a stand-in Mutex for
    // `live_sessions`). The number of slots simultaneously held must never exceed `max`.
    #[test]
    fn concurrent_reservations_never_exceed_capacity() {
        const MAX: usize = 3;
        const THREADS: usize = 64;

        let pending = Arc::new(AtomicUsize::new(0));
        let lock = Arc::new(StdMutex::new(())); // stands in for the live_sessions lock
        let live_held = Arc::new(AtomicUsize::new(0)); // slots currently held
        let peak = Arc::new(AtomicUsize::new(0)); // max simultaneously held
        let granted = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..THREADS {
            let (pending, lock, live_held, peak, granted) = (
                pending.clone(),
                lock.clone(),
                live_held.clone(),
                peak.clone(),
                granted.clone(),
            );
            handles.push(std::thread::spawn(move || {
                // Check-and-reserve under the lock, exactly like the handler does.
                let reservation = {
                    let _guard = lock.lock().unwrap();
                    try_reserve_live_slot(0, &pending, MAX)
                };
                if let Some(_r) = reservation {
                    granted.fetch_add(1, Ordering::SeqCst);
                    let held = live_held.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(held, Ordering::SeqCst);
                    // Hold the slot briefly to widen the race window.
                    std::thread::yield_now();
                    live_held.fetch_sub(1, Ordering::SeqCst);
                    // `_r` drops here, releasing the reservation.
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        assert!(
            peak.load(Ordering::SeqCst) <= MAX,
            "held {} slots at once, exceeds max {MAX}",
            peak.load(Ordering::SeqCst)
        );
        assert!(
            granted.load(Ordering::SeqCst) >= MAX,
            "no reservations granted"
        );
        assert_eq!(
            pending.load(Ordering::SeqCst),
            0,
            "all reservations should be released"
        );
    }

    // ── ARCH-3: control-endpoint & lifecycle coverage via a fake session ────────
    //
    // Before ARCH-3 the registry held concrete `ControlHandle`/`CodexControlHandle`
    // values, each backed by a real `claude`/`codex` subprocess — so the control
    // and eviction handlers were untestable without a live binary. With
    // `Box<dyn LiveSession>` we insert a `FakeSession` record directly and drive
    // the handlers.

    #[derive(Default)]
    struct Counters {
        turns: AtomicUsize,
        interrupts: AtomicUsize,
        disconnects: AtomicUsize,
        set_models: AtomicUsize,
    }

    struct FakeSession {
        c: Arc<Counters>,
        supports_set_model: bool,
    }

    impl LiveSession for FakeSession {
        fn send_turn(&self, _text: String) -> Result<(), ControlError> {
            self.c.turns.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn interrupt(&self) -> Result<(), ControlError> {
            self.c.interrupts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn disconnect(&self) -> Result<(), ControlError> {
            self.c.disconnects.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn set_model(&self, _model: Option<String>) -> Result<(), ControlError> {
            if self.supports_set_model {
                self.c.set_models.fetch_add(1, Ordering::SeqCst);
                Ok(())
            } else {
                Err(ControlError::Unsupported("set_model"))
            }
        }
        fn get_context_usage(&self) -> Result<serde_json::Value, ControlError> {
            Ok(serde_json::json!({ "used_tokens": 1 }))
        }
    }

    fn state_with_session(
        control: Box<dyn LiveSession>,
        status: LiveSessionStatus,
    ) -> (crate::cli::serve::AppState, String) {
        use crate::cli::serve::ServeConfig;
        let sid = "sess-1".to_string();
        let live: LiveSessions = Arc::new(std::sync::Mutex::new(HashMap::new()));
        live.lock().unwrap().insert(
            sid.clone(),
            LiveSessionRecord {
                session_id: sid.clone(),
                agent_key: "claude".into(),
                control,
                status,
                created_at: Utc::now(),
            },
        );
        let state = crate::cli::serve::AppState {
            runs: Arc::new(std::sync::Mutex::new(HashMap::new())),
            live_sessions: live,
            pending_live_sessions: Arc::new(AtomicUsize::new(0)),
            config: ServeConfig {
                host: "127.0.0.1".into(),
                port: 8787,
                run_timeout_secs: 30,
                max_sessions: 10,
                api_key: None,
                insecure: false,
            },
            run_fn: crate::cli::serve::run_session::make_stub_run_fn(),
            auth_cache: Arc::new(std::sync::Mutex::new(None)),
        };
        (state, sid)
    }

    fn control_req(action: &str, text: Option<&str>) -> LiveSessionControlRequest {
        LiveSessionControlRequest {
            action: action.into(),
            model: None,
            text: text.map(Into::into),
        }
    }

    async fn control_status(
        state: &crate::cli::serve::AppState,
        sid: &str,
        req: LiveSessionControlRequest,
    ) -> StatusCode {
        live_session_control_handler(State(state.clone()), Path(sid.to_string()), Json(req))
            .await
            .into_response()
            .status()
    }

    #[tokio::test]
    async fn control_dispatches_core_ops_to_the_session() {
        let c = Arc::new(Counters::default());
        let (state, sid) = state_with_session(
            Box::new(FakeSession {
                c: Arc::clone(&c),
                supports_set_model: true,
            }),
            LiveSessionStatus::Active,
        );

        assert_eq!(
            control_status(&state, &sid, control_req("interrupt", None)).await,
            StatusCode::OK
        );
        assert_eq!(
            control_status(&state, &sid, control_req("send_turn", Some("hello"))).await,
            StatusCode::OK
        );
        assert_eq!(
            control_status(&state, &sid, control_req("set_model", None)).await,
            StatusCode::OK
        );

        assert_eq!(c.interrupts.load(Ordering::SeqCst), 1);
        assert_eq!(c.turns.load(Ordering::SeqCst), 1);
        assert_eq!(c.set_models.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn send_turn_with_blank_text_is_422_and_never_reaches_the_session() {
        let c = Arc::new(Counters::default());
        let (state, sid) = state_with_session(
            Box::new(FakeSession {
                c: Arc::clone(&c),
                supports_set_model: true,
            }),
            LiveSessionStatus::Active,
        );
        assert_eq!(
            control_status(&state, &sid, control_req("send_turn", Some("   "))).await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(c.turns.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn set_model_on_unsupported_backend_is_422_not_supported() {
        let c = Arc::new(Counters::default());
        let (state, sid) = state_with_session(
            Box::new(FakeSession {
                c: Arc::clone(&c),
                supports_set_model: false,
            }),
            LiveSessionStatus::Active,
        );
        assert_eq!(
            control_status(&state, &sid, control_req("set_model", None)).await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(c.set_models.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn get_context_usage_returns_200_json_from_the_session() {
        let c = Arc::new(Counters::default());
        let (state, sid) = state_with_session(
            Box::new(FakeSession {
                c,
                supports_set_model: true,
            }),
            LiveSessionStatus::Active,
        );
        assert_eq!(
            control_status(&state, &sid, control_req("get_context_usage", None)).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn control_on_unknown_session_is_404() {
        let c = Arc::new(Counters::default());
        let (state, _sid) = state_with_session(
            Box::new(FakeSession {
                c,
                supports_set_model: true,
            }),
            LiveSessionStatus::Active,
        );
        assert_eq!(
            control_status(&state, "does-not-exist", control_req("interrupt", None)).await,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn control_on_closed_session_is_410_gone() {
        let c = Arc::new(Counters::default());
        let (state, sid) = state_with_session(
            Box::new(FakeSession {
                c: Arc::clone(&c),
                supports_set_model: true,
            }),
            LiveSessionStatus::Closed,
        );
        assert_eq!(
            control_status(&state, &sid, control_req("interrupt", None)).await,
            StatusCode::GONE
        );
        assert_eq!(c.interrupts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn delete_disconnects_evicts_and_hides_from_list() {
        let c = Arc::new(Counters::default());
        let (state, sid) = state_with_session(
            Box::new(FakeSession {
                c: Arc::clone(&c),
                supports_set_model: true,
            }),
            LiveSessionStatus::Active,
        );

        // Session shows in the listing while Active.
        let listed = list_live_sessions_handler(State(state.clone()))
            .await
            .into_response();
        assert_eq!(listed.status(), StatusCode::OK);

        // Delete → disconnect called once, record marked Closed.
        let del = delete_live_session_handler(State(state.clone()), Path(sid.clone()))
            .await
            .into_response();
        assert_eq!(del.status(), StatusCode::OK);
        assert_eq!(c.disconnects.load(Ordering::SeqCst), 1);
        assert_eq!(
            state.live_sessions.lock().unwrap()[&sid].status,
            LiveSessionStatus::Closed
        );

        // A closed session is filtered out of the listing.
        let listed_after = list_live_sessions_handler(State(state.clone()))
            .await
            .into_response();
        let bytes = axum::body::to_bytes(listed_after.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["sessions"].as_array().unwrap().len(), 0);

        // And further control on it is 410 Gone.
        assert_eq!(
            control_status(&state, &sid, control_req("send_turn", Some("x"))).await,
            StatusCode::GONE
        );
    }
}
