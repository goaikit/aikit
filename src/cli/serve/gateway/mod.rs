//! Durable host-bound session API. Client subscriptions never own agent lifetimes.
mod acp;
mod adapters;
pub(super) mod auth;
mod errors;
mod opencode;
mod rpc;
mod store;

use adapters::{Ask, Emit};
use aikit_sdk::runner::session::*;
#[cfg(test)]
use axum::http::StatusCode;
use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::HeaderMap,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    convert::Infallible,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use tokio_stream::wrappers::ReceiverStream;

type Factory = Arc<
    dyn Fn(
            &CreateSession,
            Emit,
            Ask,
        ) -> anyhow::Result<(Box<dyn adapters::Driver>, SessionCapabilities)>
        + Send
        + Sync,
>;
struct PendingReply {
    tx: mpsc::SyncSender<RequestResponse>,
    expires_at_ms: u64,
}
#[derive(Default)]
struct Metrics {
    queue_rejections: AtomicU64,
    persistence_failures: AtomicU64,
    slow_subscribers: AtomicU64,
}
struct Live {
    tx: mpsc::SyncSender<SessionCommand>,
    acceptance: Mutex<()>,
    requests: Mutex<HashMap<String, PendingReply>>,
    policy: PermissionPolicy,
    closing: AtomicBool,
}
pub(super) struct Host {
    store: store::Store,
    live: Mutex<HashMap<String, Arc<Live>>>,
    creation: Mutex<()>,
    roots: Vec<PathBuf>,
    max_sessions: usize,
    notify: tokio::sync::Notify,
    draining: AtomicBool,
    factory: Factory,
    subscriptions: Arc<tokio::sync::Semaphore>,
    metrics: Metrics,
    request_timeout: Duration,
    subscriber_timeout: Duration,
}

impl Host {
    pub fn ready(&self) -> bool {
        !self.draining.load(Ordering::SeqCst)
    }
    pub async fn finish_shutdown(&self) {
        self.drain();
        for _ in 0..100 {
            if self.live.lock().unwrap().is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        tracing::warn!(
            "gateway drain deadline exceeded; process supervisor must stop remaining agents"
        );
    }
    pub fn open(max_sessions: usize) -> anyhow::Result<Arc<Self>> {
        let data = std::env::var_os("AIKIT_GATEWAY_DATA")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?.join(".aikit/host"));
        std::fs::create_dir_all(&data)?;
        let roots = match std::env::var("AIKIT_GATEWAY_WORKSPACES") {
            Ok(s) => serde_json::from_str::<Vec<PathBuf>>(&s)?,
            Err(_) => vec![std::env::current_dir()?],
        }
        .into_iter()
        .map(std::fs::canonicalize)
        .collect::<Result<Vec<_>, _>>()?;
        anyhow::ensure!(!roots.is_empty(), "at least one workspace root is required");
        Ok(Arc::new(Self {
            store: store::Store::open(&data.join("sessions.sqlite"))?,
            live: Mutex::new(HashMap::new()),
            creation: Mutex::new(()),
            roots,
            max_sessions,
            notify: tokio::sync::Notify::new(),
            draining: AtomicBool::new(false),
            factory: Arc::new(adapters::open),
            metrics: Metrics::default(),
            request_timeout: Duration::from_secs(120),
            subscriber_timeout: Duration::from_secs(15),
            subscriptions: Arc::new(tokio::sync::Semaphore::new(
                max_sessions.saturating_mul(4).clamp(4, 1024),
            )),
        }))
    }
    fn persist<T>(&self, result: anyhow::Result<T>) -> anyhow::Result<T> {
        if let Err(ref error) = result {
            self.metrics
                .persistence_failures
                .fetch_add(1, Ordering::Relaxed);
            self.draining.store(true, Ordering::SeqCst);
            tracing::error!(%error,"session persistence failed; refusing new work");
        }
        result
    }
    fn emit(&self, id: &str, event: SessionEventKind) {
        if let Err(e) = self.store.append(id, event) {
            self.metrics
                .persistence_failures
                .fetch_add(1, Ordering::Relaxed);
            self.draining.store(true, Ordering::SeqCst);
            tracing::error!(session_id=id,error=%e,"session persistence failed; refusing new work");
        }
        self.notify.notify_waiters();
    }
    fn create(self: &Arc<Self>, mut req: CreateSession) -> anyhow::Result<CommandReceipt> {
        let _gate = self.creation.lock().unwrap();
        validate_id(&req.command_id)?;
        anyhow::ensure!(!req.prompt.trim().is_empty(), "prompt_required");
        anyhow::ensure!(req.prompt.len() <= 128 * 1024, "prompt_too_large");
        req.cwd = std::fs::canonicalize(&req.cwd)?;
        anyhow::ensure!(
            req.cwd.is_dir() && self.roots.iter().any(|root| req.cwd.starts_with(root)),
            "workspace_not_allowed"
        );
        let body = serde_json::to_string(&req)?;
        if let Some(receipt) = self.store.receipt("create", &req.command_id, &body)? {
            return Ok(receipt);
        }
        anyhow::ensure!(!self.draining.load(Ordering::SeqCst), "host_draining");
        anyhow::ensure!(
            self.live.lock().unwrap().len() < self.max_sessions,
            "session_capacity_exceeded"
        );
        anyhow::ensure!(
            self.store.list()?.len() < 10_000,
            "session_history_capacity_exceeded"
        );
        let id = uuid::Uuid::new_v4().to_string();
        let turn = uuid::Uuid::new_v4().to_string();
        let receipt = CommandReceipt {
            command_id: req.command_id.clone(),
            session_id: id.clone(),
            status: CommandStatus::Accepted,
            result: None,
            error: None,
            failure: None,
        };
        let info = SessionInfo {
            host_id: self.store.host_id.clone(),
            session_id: id.clone(),
            backend: req.backend,
            cwd: req.cwd.clone(),
            status: SessionStatus::Opening,
            capabilities: adapters::capabilities(req.backend),
            native_session_id: req.resume.clone(),
            active_turn_id: Some(turn),
            last_sequence: 0,
        };
        self.persist(self.store.create(&info, &body, &receipt))?;
        let (tx, rx) = mpsc::sync_channel(32);
        let live = Arc::new(Live {
            tx,
            acceptance: Mutex::new(()),
            requests: Mutex::new(HashMap::new()),
            policy: req.permission_policy,
            closing: AtomicBool::new(false),
        });
        self.live.lock().unwrap().insert(id.clone(), live.clone());
        self.emit(&id, SessionEventKind::Command(receipt.clone()));
        self.emit(
            &id,
            SessionEventKind::Input {
                text: req.prompt.clone(),
            },
        );
        if self.draining.load(Ordering::SeqCst) {
            self.live.lock().unwrap().remove(&id);
            anyhow::bail!("persistence_unavailable");
        }
        let host = self.clone();
        let returned = receipt.clone();
        std::thread::spawn(move || {
            let emit: Emit = Arc::new({
                let host = host.clone();
                let id = id.clone();
                move |event| host.emit(&id, event)
            });
            let ask: Ask = Arc::new({
                let host = host.clone();
                let id = id.clone();
                let live = live.clone();
                move |kind, input| host.ask(&id, &live, kind, input)
            });
            let mut create_receipt = receipt;
            let mut driver = match (host.factory)(&req, emit, ask) {
                Ok((driver, caps)) => {
                    let _ = host.store.update(&id, |current| {
                        current.capabilities = caps;
                        if current.status == SessionStatus::Opening {
                            current.status = SessionStatus::Running;
                        }
                    });
                    create_receipt.status = CommandStatus::Dispatched;
                    let _ = host.persist(host.store.finish("create", &create_receipt));
                    host.emit(&id, SessionEventKind::Command(create_receipt));
                    driver
                }
                Err(e) => {
                    create_receipt.status = CommandStatus::Failed;
                    create_receipt.error = Some(e.to_string());
                    create_receipt.failure = Some(CommandFailure {
                        code: "adapter_start_failed".into(),
                        retry: "inspect_receipt".into(),
                    });
                    let _ = host.persist(host.store.finish("create", &create_receipt));
                    host.emit(&id, SessionEventKind::Command(create_receipt));
                    host.emit(&id, SessionEventKind::State(SessionStatus::Failed));
                    host.cancel_requests(&id, &live);
                    host.finish_session(&id, &live, &rx);
                    return;
                }
            };
            loop {
                let command = match rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(c) => c,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if host.draining.load(Ordering::SeqCst) {
                            host.cancel_requests(&id, &live);
                            if driver.execute(&SessionAction::Close).is_ok() {
                                host.emit(&id, SessionEventKind::State(SessionStatus::Closed));
                                break;
                            }
                        }
                        if host.store.get(&id).is_ok_and(|i| {
                            matches!(
                                i.status,
                                SessionStatus::Closed
                                    | SessionStatus::Failed
                                    | SessionStatus::Interrupted
                            )
                        }) {
                            break;
                        }
                        continue;
                    }
                    Err(_) => break,
                };
                let mut receipt = CommandReceipt {
                    command_id: command.command_id.clone(),
                    session_id: id.clone(),
                    status: CommandStatus::Dispatched,
                    result: None,
                    error: None,
                    failure: None,
                };
                let closed = matches!(command.action, SessionAction::Close);
                let mut started_turn = false;
                let result = host.validate_action(&id, &command.action).and_then(|_| {
                    anyhow::ensure!(
                        !host.draining.load(Ordering::SeqCst) || closed,
                        "host_draining"
                    );
                    if matches!(
                        command.action,
                        SessionAction::Interrupt | SessionAction::Close
                    ) {
                        host.cancel_requests(&id, &live);
                    }
                    if matches!(command.action, SessionAction::SendTurn { .. }) {
                        host.store.begin_turn(&id)?;
                        started_turn = true;
                    }
                    if let SessionAction::SendTurn { text } = &command.action {
                        host.emit(&id, SessionEventKind::Input { text: text.clone() });
                    }
                    anyhow::ensure!(host.ready() || closed, "persistence_unavailable");
                    driver.execute(&command.action)
                });
                match result {
                    Ok(value) => receipt.result = value,
                    Err(e) => {
                        receipt.status = CommandStatus::Failed;
                        receipt.error = Some(e.to_string());
                        receipt.failure = Some(errors::command_failure(&e.to_string()));
                        if closed {
                            live.closing.store(false, Ordering::SeqCst);
                        }
                        if started_turn {
                            host.emit(&id, SessionEventKind::State(SessionStatus::Idle));
                        }
                    }
                }
                let _ = host.persist(host.store.finish(&id, &receipt));
                host.emit(&id, SessionEventKind::Command(receipt.clone()));
                if closed && receipt.status != CommandStatus::Failed {
                    host.emit(&id, SessionEventKind::State(SessionStatus::Closed));
                    break;
                }
            }
            host.cancel_requests(&id, &live);
            drop(driver);
            host.finish_session(&id, &live, &rx);
        });
        Ok(returned)
    }
    fn finish_session(&self, id: &str, live: &Live, commands: &mpsc::Receiver<SessionCommand>) {
        let _gate = live.acceptance.lock().unwrap();
        live.closing.store(true, Ordering::SeqCst);
        for command in commands.try_iter() {
            if let Ok(mut receipt) = self.store.lookup(id, &command.command_id) {
                receipt.status = CommandStatus::Failed;
                receipt.error = Some("session_closed_before_dispatch".into());
                receipt.failure = Some(CommandFailure {
                    code: "session_closed_before_dispatch".into(),
                    retry: "never".into(),
                });
                let _ = self.persist(self.store.finish(id, &receipt));
                self.emit(id, SessionEventKind::Command(receipt));
            }
        }
        self.live.lock().unwrap().remove(id);
    }
    fn validate_action(&self, id: &str, a: &SessionAction) -> anyhow::Result<()> {
        let info = self.store.get(id)?;
        let c = &info.capabilities;
        let supported = match a {
            SessionAction::SendTurn { text } => {
                anyhow::ensure!(!text.trim().is_empty(), "text_required");
                anyhow::ensure!(text.len() <= 128 * 1024, "prompt_too_large");
                anyhow::ensure!(info.status == SessionStatus::Idle, "turn_not_idle");
                c.send_turn
            }
            SessionAction::Interrupt => c.interrupt,
            SessionAction::Steer { text } => !text.trim().is_empty() && c.steer,
            SessionAction::FollowUp { text } => !text.trim().is_empty() && c.follow_up,
            SessionAction::SetModel { model } => !model.trim().is_empty() && c.set_model,
            SessionAction::ContextUsage => c.context_usage,
            SessionAction::Respond { .. } | SessionAction::Close => true,
        };
        anyhow::ensure!(supported, "unsupported_operation");
        Ok(())
    }
    fn command(&self, id: &str, command: SessionCommand) -> anyhow::Result<CommandReceipt> {
        validate_id(&command.command_id)?;
        let body = serde_json::to_string(&command)?;
        if let Some(receipt) = self.store.receipt(id, &command.command_id, &body)? {
            return Ok(receipt);
        }
        let live = self
            .live
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("session_not_active"))?;
        let _gate = live.acceptance.lock().unwrap();
        if let Some(receipt) = self.store.receipt(id, &command.command_id, &body)? {
            return Ok(receipt);
        }
        anyhow::ensure!(
            !self.draining.load(Ordering::SeqCst)
                || matches!(
                    command.action,
                    SessionAction::Close | SessionAction::Respond { .. }
                ),
            "host_draining"
        );
        anyhow::ensure!(
            !live.closing.load(Ordering::SeqCst)
                || matches!(command.action, SessionAction::Respond { .. }),
            "session_not_active"
        );
        let mut receipt = CommandReceipt {
            command_id: command.command_id.clone(),
            session_id: id.into(),
            status: CommandStatus::Accepted,
            result: None,
            error: None,
            failure: None,
        };
        self.persist(self.store.accept(id, &body, &receipt))?;
        self.emit(id, SessionEventKind::Command(receipt.clone()));
        anyhow::ensure!(
            self.ready() || matches!(command.action, SessionAction::Close),
            "persistence_unavailable"
        );
        if matches!(
            command.action,
            SessionAction::Interrupt | SessionAction::Close
        ) {
            if matches!(command.action, SessionAction::Close) {
                live.closing.store(true, Ordering::SeqCst);
            }
            self.cancel_requests(id, &live);
        }
        if let SessionAction::Respond {
            request_id,
            response,
        } = &command.action
        {
            let result = live
                .requests
                .lock()
                .unwrap()
                .remove(request_id)
                .ok_or_else(|| anyhow::anyhow!("request_expired_or_resolved"))
                .and_then(|reply| {
                    anyhow::ensure!(
                        now_ms() < reply.expires_at_ms,
                        "request_expired_or_resolved"
                    );
                    reply
                        .tx
                        .try_send(response.clone())
                        .map_err(anyhow::Error::from)
                });
            receipt.status = if result.is_ok() {
                CommandStatus::Dispatched
            } else {
                CommandStatus::Failed
            };
            receipt.error = result.err().map(|e| e.to_string());
            receipt.failure = receipt.error.as_deref().map(errors::command_failure);
            self.persist(self.store.finish(id, &receipt))?;
        } else if let Err(e) = live.tx.try_send(command) {
            self.metrics
                .queue_rejections
                .fetch_add(1, Ordering::Relaxed);
            receipt.status = CommandStatus::Failed;
            receipt.error = Some(
                match e {
                    mpsc::TrySendError::Full(_) => "command_queue_full",
                    mpsc::TrySendError::Disconnected(_) => "session_not_active",
                }
                .into(),
            );
            receipt.failure = receipt.error.as_deref().map(errors::command_failure);
            self.persist(self.store.finish(id, &receipt))?;
        }
        self.emit(id, SessionEventKind::Command(receipt.clone()));
        Ok(receipt)
    }
    fn ask(&self, id: &str, live: &Live, kind: &str, payload: Value) -> RequestResponse {
        if live.closing.load(Ordering::SeqCst) || !self.ready() {
            return RequestResponse::Deny;
        }
        if kind == "permission" {
            match live.policy {
                PermissionPolicy::Allow => return RequestResponse::Allow { option_id: None },
                PermissionPolicy::Deny => return RequestResponse::Deny,
                PermissionPolicy::Ask => {}
            }
        }
        if self.draining.load(Ordering::SeqCst) {
            return RequestResponse::Deny;
        }
        let request = PendingRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            kind: kind.into(),
            payload,
            expires_at_ms: now_ms() + self.request_timeout.as_millis() as u64,
        };
        let (tx, rx) = mpsc::sync_channel(1);
        {
            // Registration, cancellation and deadline checks share this lock.
            let mut pending = live.requests.lock().unwrap();
            if pending.len() >= 32 || live.closing.load(Ordering::SeqCst) || !self.ready() {
                return RequestResponse::Deny;
            }
            if self.store.request(id, &request).is_err() {
                self.metrics
                    .persistence_failures
                    .fetch_add(1, Ordering::Relaxed);
                self.draining.store(true, Ordering::SeqCst);
                return RequestResponse::Deny;
            }
            pending.insert(
                request.request_id.clone(),
                PendingReply {
                    tx,
                    expires_at_ms: request.expires_at_ms,
                },
            );
            self.emit(id, SessionEventKind::Request(request.clone()));
        }
        let result = rx.recv_timeout(self.request_timeout);
        live.requests.lock().unwrap().remove(&request.request_id);
        let _ = self.store.resolve(id, &request.request_id);
        self.emit(
            id,
            SessionEventKind::RequestResolved {
                request_id: request.request_id,
                reason: if result.is_ok() {
                    "responded"
                } else {
                    "expired"
                }
                .into(),
            },
        );
        match result {
            Ok(RequestResponse::Answers { .. }) if kind != "question" => RequestResponse::Deny,
            Ok(RequestResponse::Allow { .. }) if kind == "question" => RequestResponse::Deny,
            Ok(response) => response,
            Err(_) => RequestResponse::Deny,
        }
    }
    fn cancel_requests(&self, id: &str, live: &Live) {
        for (request, reply) in live.requests.lock().unwrap().drain() {
            let _ = reply.tx.try_send(RequestResponse::Deny);
            let _ = self.store.resolve(id, &request);
        }
    }
    pub fn drain(&self) {
        self.draining.store(true, Ordering::SeqCst);
        for (id, live) in self.live.lock().unwrap().iter() {
            self.cancel_requests(id, live);
        }
        self.notify.notify_waiters();
    }
}
fn validate_id(id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_:.".contains(&b)),
        "invalid_command_id"
    );
    Ok(())
}

pub(super) fn router(host: Arc<Host>) -> Router {
    Router::new()
        .route("/gateway", get(discover))
        .route("/gateway/metrics", get(metrics))
        .route("/gateway/schema", get(|| async { Json(schemas()) }))
        .route("/gateway/sessions", post(create).get(list))
        .route("/gateway/sessions/{id}", get(inspect))
        .route("/gateway/sessions/{id}/commands", post(command))
        .route("/gateway/sessions/{id}/commands/{command_id}", get(receipt))
        .route("/gateway/commands/{command_id}", get(creation_receipt))
        .route("/gateway/sessions/{id}/events", get(events))
        .route("/gateway/sessions/{id}/requests", get(requests))
        .route(
            "/gateway/sessions/{id}/requests/{request_id}/response",
            post(respond),
        )
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(host)
}
fn error(e: anyhow::Error) -> Response {
    errors::response(e)
}
async fn blocking<T: serde::Serialize + Send + 'static>(
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Response {
    match tokio::task::spawn_blocking(work).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => error(e),
        Err(e) => error(e.into()),
    }
}
async fn metrics(State(h): State<Arc<Host>>) -> Response {
    blocking(move || {
        let states = h.store.list()?;
        let live = h.live.lock().unwrap();
        Ok(json!({
            "active_sessions": live.len(),
            "opening_sessions": states.iter().filter(|s| s.status == SessionStatus::Opening).count(),
            "pending_requests": live.values().map(|s| s.requests.lock().unwrap().len()).sum::<usize>(),
            "active_subscribers": h.max_sessions.saturating_mul(4).clamp(4,1024) - h.subscriptions.available_permits(),
            "command_queue_rejections_total": h.metrics.queue_rejections.load(Ordering::Relaxed),
            "persistence_failures_total": h.metrics.persistence_failures.load(Ordering::Relaxed),
            "slow_subscriber_disconnects_total": h.metrics.slow_subscribers.load(Ordering::Relaxed)
        }))
    }).await
}
async fn discover(State(h): State<Arc<Host>>) -> Response {
    let active_sessions = h.live.lock().unwrap().len();
    Json(json!({"version":PROTOCOL_VERSION,"host_id":h.store.host_id,"draining":!h.ready(),"active_sessions":active_sessions,"max_sessions":h.max_sessions,"backends":BACKENDS.iter().map(|b|json!({"backend":b,"capabilities":adapters::capabilities(*b)})).collect::<Vec<_>>()})).into_response()
}
async fn create(State(h): State<Arc<Host>>, Json(req): Json<CreateSession>) -> Response {
    blocking(move || h.create(req)).await
}
async fn list(State(h): State<Arc<Host>>) -> Response {
    blocking(move || h.store.list()).await
}
async fn inspect(State(h): State<Arc<Host>>, Path(id): Path<String>) -> Response {
    blocking(move || h.store.get(&id)).await
}
async fn requests(State(h): State<Arc<Host>>, Path(id): Path<String>) -> Response {
    blocking(move || h.store.pending(&id)).await
}
async fn command(
    State(h): State<Arc<Host>>,
    Path(id): Path<String>,
    Json(req): Json<SessionCommand>,
) -> Response {
    blocking(move || h.command(&id, req)).await
}
async fn receipt(
    State(h): State<Arc<Host>>,
    Path((id, command_id)): Path<(String, String)>,
) -> Response {
    blocking(move || h.store.lookup(&id, &command_id)).await
}
async fn creation_receipt(State(h): State<Arc<Host>>, Path(id): Path<String>) -> Response {
    blocking(move || h.store.lookup("create", &id)).await
}
#[derive(Deserialize)]
struct RespondRequest {
    command_id: String,
    response: RequestResponse,
}
async fn respond(
    State(h): State<Arc<Host>>,
    Path((id, request_id)): Path<(String, String)>,
    Json(req): Json<RespondRequest>,
) -> Response {
    blocking(move || {
        h.command(
            &id,
            SessionCommand {
                command_id: req.command_id,
                action: SessionAction::Respond {
                    request_id,
                    response: req.response,
                },
            },
        )
    })
    .await
}
#[derive(Deserialize)]
struct Cursor {
    #[serde(default)]
    after: Option<u64>,
}
async fn events(
    State(h): State<Arc<Host>>,
    Path(id): Path<String>,
    Query(cursor): Query<Cursor>,
    headers: HeaderMap,
) -> Response {
    let header = match headers.get("last-event-id") {
        Some(v) => match v.to_str().ok().and_then(|v| v.parse::<u64>().ok()) {
            Some(n) => Some(n),
            None => return error(anyhow::anyhow!("invalid_cursor")),
        },
        None => None,
    };
    let mut after = cursor.after.or(header).unwrap_or(0);
    let check = h.clone();
    let sid = id.clone();
    match tokio::task::spawn_blocking(move || {
        let info = check.store.get(&sid)?;
        anyhow::ensure!(after <= info.last_sequence, "cursor_ahead");
        check.store.replay(&sid, after)
    })
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => return error(e),
        Err(e) => return error(e.into()),
    }
    let permit = match h.subscriptions.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => return error(anyhow::anyhow!("subscriber_capacity_exceeded")),
    };
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(2);
    tokio::spawn(async move {
        let _permit = permit;
        loop {
            let notified = h.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let check = h.clone();
            let sid = id.clone();
            let page = tokio::task::spawn_blocking(move || check.store.replay(&sid, after)).await;
            match page {
                Ok(Ok(page)) if !page.is_empty() => {
                    for event in page {
                        after = event.sequence;
                        let frame = Event::default()
                            .id(after.to_string())
                            .event("session_event")
                            .json_data(event)
                            .unwrap();
                        if tokio::time::timeout(h.subscriber_timeout, tx.send(Ok(frame)))
                            .await
                            .is_err()
                        {
                            h.metrics.slow_subscribers.fetch_add(1, Ordering::Relaxed);
                            return;
                        }
                        if tx.is_closed() {
                            return;
                        }
                    }
                }
                Ok(Ok(_)) => {
                    tokio::select! {_=notified=>{},_=tokio::time::sleep(Duration::from_secs(1))=>{},_=tx.closed()=>return}
                }
                _ => {
                    let _ = tokio::time::timeout(
                        h.subscriber_timeout,
                        tx.send(Ok(Event::default().event("replay_error").data(
                            "cursor expired or storage unavailable; fetch session state",
                        ))),
                    )
                    .await;
                    return;
                }
            }
        }
    });
    Sse::new(ReceiverStream::new(rx))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake {
        emit: Emit,
    }
    impl adapters::Driver for Fake {
        fn execute(&mut self, a: &SessionAction) -> anyhow::Result<Option<Value>> {
            if let SessionAction::SendTurn { .. } = a {
                adapters::terminal(&self.emit, SessionBackend::Pi, Ok(json!({})));
            }
            Ok(None)
        }
    }
    pub(super) fn host(dir: &std::path::Path) -> Arc<Host> {
        Arc::new(Host {
            store: store::Store::open(&dir.join("store.db")).unwrap(),
            metrics: Metrics::default(),
            request_timeout: Duration::from_millis(250),
            subscriber_timeout: Duration::from_millis(100),
            live: Mutex::new(HashMap::new()),
            creation: Mutex::new(()),
            roots: vec![std::fs::canonicalize(dir).unwrap()],
            max_sessions: 1,
            notify: tokio::sync::Notify::new(),
            draining: AtomicBool::new(false),
            subscriptions: Arc::new(tokio::sync::Semaphore::new(4)),
            factory: Arc::new(|_, emit, _| {
                adapters::terminal(&emit, SessionBackend::Pi, Ok(json!({})));
                Ok((
                    Box::new(Fake { emit }),
                    adapters::capabilities(SessionBackend::Pi),
                ))
            }),
        })
    }
    #[test]
    fn session_survives_no_subscriber_and_creation_retry() {
        let dir = tempfile::tempdir().unwrap();
        let h = host(dir.path());
        let req = CreateSession {
            command_id: "create-1".into(),
            backend: SessionBackend::Pi,
            cwd: dir.path().into(),
            prompt: "hello".into(),
            model: None,
            resume: None,
            permission_policy: PermissionPolicy::Allow,
        };
        let a = h.create(req.clone()).unwrap();
        let b = h.create(req.clone()).unwrap();
        assert_eq!(a.session_id, b.session_id);
        let mut other = req;
        other.command_id = "other".into();
        assert!(h.create(other).is_err());
        let id = a.session_id;
        for _ in 0..100 {
            if h.store.get(&id).unwrap().status == SessionStatus::Idle {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(h.store.get(&id).unwrap().status, SessionStatus::Idle);
        h.drain();
    }
    #[test]
    fn command_ids_are_bounded() {
        assert!(validate_id("").is_err());
        assert!(validate_id(&"a".repeat(129)).is_err());
        assert!(validate_id("client-1:command.2").is_ok());
    }

    #[test]
    fn rejected_turn_preserves_running_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = host(dir.path());
        Arc::get_mut(&mut h).unwrap().factory = Arc::new(|_, emit, _| {
            Ok((
                Box::new(Fake { emit }),
                adapters::capabilities(SessionBackend::Pi),
            ))
        });
        let id = h
            .create(CreateSession {
                command_id: "create".into(),
                backend: SessionBackend::Pi,
                cwd: dir.path().into(),
                prompt: "first".into(),
                model: None,
                resume: None,
                permission_policy: PermissionPolicy::Allow,
            })
            .unwrap()
            .session_id;
        h.command(
            &id,
            SessionCommand {
                command_id: "second".into(),
                action: SessionAction::SendTurn {
                    text: "second".into(),
                },
            },
        )
        .unwrap();
        for _ in 0..200 {
            if h.store.lookup(&id, "second").unwrap().status == CommandStatus::Failed {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            h.store.lookup(&id, "second").unwrap().error.as_deref(),
            Some("turn_not_idle")
        );
        assert_eq!(h.store.get(&id).unwrap().status, SessionStatus::Running);
        h.drain();
    }

    #[test]
    fn all_nine_backends_share_creation_and_idempotent_commands() {
        for backend in BACKENDS {
            let dir = tempfile::tempdir().unwrap();
            let h = host(dir.path());
            let receipt = h
                .create(CreateSession {
                    command_id: "create".into(),
                    backend: *backend,
                    cwd: dir.path().into(),
                    prompt: "hello".into(),
                    model: Some("requested-model".into()),
                    resume: Some("native-resume".into()),
                    permission_policy: PermissionPolicy::Allow,
                })
                .unwrap();
            let id = receipt.session_id;
            for _ in 0..100 {
                if h.store.get(&id).unwrap().status == SessionStatus::Idle {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(h.store.get(&id).unwrap().backend, *backend);
            let command = SessionCommand {
                command_id: "turn-2".into(),
                action: SessionAction::SendTurn {
                    text: "next".into(),
                },
            };
            let first = h.command(&id, command.clone()).unwrap();
            let second = h.command(&id, command).unwrap();
            assert_eq!(first.command_id, second.command_id);
            for _ in 0..100 {
                if h.store.lookup(&id, "turn-2").unwrap().status != CommandStatus::Accepted {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(
                h.store.lookup(&id, "turn-2").unwrap().status,
                CommandStatus::Dispatched
            );
            assert!(h
                .command(
                    &id,
                    SessionCommand {
                        command_id: "turn-2".into(),
                        action: SessionAction::Close
                    }
                )
                .is_err());
            h.drain();
        }
    }

    #[test]
    fn pending_permission_can_resolve_without_session_worker() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = host(dir.path());
        Arc::get_mut(&mut h).unwrap().factory = Arc::new(|_, emit, ask| {
            assert!(matches!(
                ask("permission", json!({"tool":"Read"})),
                RequestResponse::Allow { .. }
            ));
            adapters::terminal(&emit, SessionBackend::Claude, Ok(json!({})));
            Ok((
                Box::new(Fake { emit }),
                adapters::capabilities(SessionBackend::Claude),
            ))
        });
        let id = h
            .create(CreateSession {
                command_id: "c".into(),
                backend: SessionBackend::Claude,
                cwd: dir.path().into(),
                prompt: "hello".into(),
                model: None,
                resume: None,
                permission_policy: PermissionPolicy::Ask,
            })
            .unwrap()
            .session_id;
        let mut pending = vec![];
        for _ in 0..100 {
            pending = h.store.pending(&id).unwrap();
            if !pending.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let request_id = pending[0].request_id.clone();
        let receipt = h
            .command(
                &id,
                SessionCommand {
                    command_id: "answer".into(),
                    action: SessionAction::Respond {
                        request_id: request_id.clone(),
                        response: RequestResponse::Allow { option_id: None },
                    },
                },
            )
            .unwrap();
        assert_eq!(receipt.status, CommandStatus::Dispatched);
        let late = h
            .command(
                &id,
                SessionCommand {
                    command_id: "late".into(),
                    action: SessionAction::Respond {
                        request_id,
                        response: RequestResponse::Deny,
                    },
                },
            )
            .unwrap();
        assert_eq!(late.status, CommandStatus::Failed);
        h.drain();
    }

    #[test]
    #[ignore = "requires explicitly selected installed agents and credentials"]
    fn live_native_gateway_smoke() {
        let selected = std::env::var("AIKIT_LIVE_BACKENDS")
            .expect("set AIKIT_LIVE_BACKENDS to comma-separated backend keys");
        for key in selected.split(',') {
            let backend: SessionBackend = serde_json::from_value(json!(key)).unwrap();
            let dir = tempfile::tempdir().unwrap();
            let mut h = host(dir.path());
            Arc::get_mut(&mut h).unwrap().factory = Arc::new(adapters::open);
            let id = h
                .create(CreateSession {
                    command_id: "live-create".into(),
                    backend,
                    cwd: dir.path().into(),
                    prompt:
                        "Reply with exactly the word ready. Do not use tools or read any files."
                            .into(),
                    model: None,
                    resume: None,
                    permission_policy: PermissionPolicy::Deny,
                })
                .unwrap()
                .session_id;
            let deadline = std::time::Instant::now() + Duration::from_secs(90);
            while std::time::Instant::now() < deadline {
                let info = h.store.get(&id).unwrap();
                if matches!(
                    info.status,
                    SessionStatus::Idle | SessionStatus::Failed | SessionStatus::Closed
                ) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let info = h.store.get(&id).unwrap();
            let receipt = h.store.lookup("create", "live-create").unwrap();
            let events = h.store.replay(&id, 0).unwrap();
            let has_text=events.iter().any(|event|matches!(&event.event,SessionEventKind::Agent(a)if matches!(&a.payload,aikit_sdk::AgentEventPayload::StreamMessage(m)if !m.text.trim().is_empty())));
            h.drain();
            println!(
                "LIVE_GATEWAY {}",
                json!({"backend":key,"state":info.status,"receipt":receipt.status,"error":receipt.error,"text":has_text,"native_session_id_present":info.native_session_id.is_some()})
            );
            assert_eq!(
                info.status,
                SessionStatus::Idle,
                "{key}: {:?}",
                receipt.error
            );
            assert!(has_text, "{key}: no agent text");
        }
    }

    #[test]
    #[ignore = "synthetic fleet: 100 stores and 1000 session workers"]
    fn synthetic_fleet_100_hosts_10_sessions() {
        let started = std::time::Instant::now();
        let mut fleet = Vec::new();
        let mut latencies = Vec::new();
        for _ in 0..100 {
            let dir = tempfile::tempdir().unwrap();
            let mut h = host(dir.path());
            Arc::get_mut(&mut h).unwrap().max_sessions = 10;
            for n in 0..10 {
                let t = std::time::Instant::now();
                h.create(CreateSession {
                    command_id: format!("create-{n}"),
                    backend: SessionBackend::Pi,
                    cwd: dir.path().into(),
                    prompt: "fixture".into(),
                    model: None,
                    resume: None,
                    permission_policy: PermissionPolicy::Allow,
                })
                .unwrap();
                latencies.push(t.elapsed().as_micros() as u64);
            }
            fleet.push((h, dir));
        }
        assert_eq!(
            fleet
                .iter()
                .map(|(h, _)| h.live.lock().unwrap().len())
                .sum::<usize>(),
            1000
        );
        for (h, _) in &fleet {
            for info in h.store.list().unwrap() {
                assert!(!h.store.replay(&info.session_id, 0).unwrap().is_empty());
            }
        }
        latencies.sort_unstable();
        println!(
            "FLEET_METRICS {}",
            json!({"hosts":100,"sessions":1000,"create_p50_us":latencies[500],"create_p95_us":latencies[950],"elapsed_ms":started.elapsed().as_millis(),"type":"synthetic_in_process"})
        );
        for (h, _) in &fleet {
            h.drain();
        }
        for (h, _) in &fleet {
            for _ in 0..200 {
                if h.live.lock().unwrap().is_empty() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(h.live.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn http_commands_and_replay_use_the_shared_wire_contract() {
        use cli_framework::tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let h = host(dir.path());
        let app = router(h.clone());
        let response=app.clone().oneshot(axum::http::Request::builder().uri("/gateway/sessions").method("POST").header("content-type","application/json").body(axum::body::Body::from(json!({"command_id":"http-create","backend":"pi","cwd":dir.path(),"prompt":"hello","permission_policy":"allow"}).to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let receipt: CommandReceipt = serde_json::from_slice(&bytes).unwrap();
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/gateway/sessions/{}/commands", receipt.session_id))
                    .method("POST")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        json!({"command_id":"close","type":"close"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!(
                        "/gateway/sessions/{}/events?after=999999",
                        receipt.session_id
                    ))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        h.drain();
    }
}

#[cfg(test)]
mod hardening_tests;
