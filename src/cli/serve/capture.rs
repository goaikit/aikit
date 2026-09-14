//! `aikit serve` capture routes — the HTTP surface for passive session
//! capture (spec 010 §14).
//!
//! Seven routes under `/api/v1/capture`:
//! - `GET  /capture`                          — list detected adapters
//! - `GET  /capture/{backend}/sessions`       — list parsed sessions
//! - `GET  /capture/{backend}/sessions/{id}/actions` — action stream
//! - `GET  /capture/{backend}/sessions/{id}/brief`   — the stored session brief
//! - `GET  /capture/{backend}/briefs`         — stored briefs, newest first
//! - `POST /capture/scan`                     — trigger async scan job
//! - `GET  /capture/scan/{job_id}`            — scan job status
//!
//! Briefs are read-only here: `aikit session summarize` generates them
//! (ADR 0022).
//!
//! All routes are registered only when the `agent-adapters` feature is on
//! (compile-time gate). A runtime `409 passive_capture_unsupported` is
//! returned for a Backend whose adapter isn't compiled in.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use aikit_sdk::runner::Backend;
use aikit_session_capture::{Adapter, CursorStore, EventStore, ParseWarning, Registry, ToolKind};

use super::error_response;

// ── CaptureState ──────────────────────────────────────────────────────────────

/// Shared state for all `/api/v1/capture` routes (spec 010 §14.4).
#[derive(Clone)]
pub struct CaptureState {
    pub registry: Arc<Registry>,
    pub event_store: Arc<dyn EventStore>,
    pub cursor_store: Arc<dyn CursorStore>,
    pub scan_jobs: Arc<ScanJobRegistry>,
    /// Last parse timestamp per adapter kind, for the `GET /capture` summary.
    last_parse: Arc<Mutex<HashMap<ToolKind, i64>>>,
}

impl CaptureState {
    pub fn new(
        registry: Registry,
        event_store: Arc<dyn EventStore>,
        cursor_store: Arc<dyn CursorStore>,
    ) -> Self {
        Self {
            registry: Arc::new(registry),
            event_store,
            cursor_store,
            scan_jobs: Arc::new(ScanJobRegistry::default()),
            last_parse: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn touch_last_parse(&self, kind: ToolKind) {
        self.last_parse.lock().unwrap().insert(kind, now_ms());
    }

    pub fn last_parse_ms(&self, kind: ToolKind) -> Option<i64> {
        self.last_parse.lock().unwrap().get(&kind).copied()
    }

    /// Spawn a background task that drains a [`WatchDriver`] into the same
    /// `parse_and_store_file` pipeline the manual `POST /capture/scan` route
    /// uses (spec 010 §14.3 / §14.5). The watcher fires on file changes;
    /// each event triggers a parse + upsert. Idempotency is identical.
    #[cfg(feature = "watcher")]
    pub fn spawn_watcher(
        self: &Arc<Self>,
        driver: Box<dyn aikit_session_capture::watch::WatchDriver>,
    ) {
        let cs = Arc::clone(self);
        tokio::spawn(async move {
            let mut driver = driver;
            while let Some(path) = driver.next_event().await {
                // Find the adapter that claims this path.
                let adapters = cs.registry.all();
                let adapter = match adapters.iter().find(|a| a.is_session_file(&path)).copied() {
                    Some(a) => a,
                    None => continue,
                };
                cs.touch_last_parse(adapter.kind());
                if let Err(e) = parse_and_store_file(
                    adapter,
                    &path,
                    cs.event_store.as_ref(),
                    cs.cursor_store.as_ref(),
                    false,
                )
                .await
                {
                    tracing::warn!(
                        target: "aikit::serve::capture::watcher",
                        path = %path.display(),
                        error = %e,
                        "watcher parse failed"
                    );
                }
            }
        });
    }
}

/// Build a capture router bound to `state`. Merge into the main domain router
/// under `/api/v1/capture`.
pub fn build_router(state: CaptureState) -> Router {
    let router = Router::new()
        .route("/capture", get(list_adapters))
        .route("/capture/{backend}", get(adapter_summary))
        .route("/capture/{backend}/sessions", get(list_sessions))
        .route(
            "/capture/{backend}/sessions/{session_id}/actions",
            get(list_actions),
        )
        .route(
            "/capture/{backend}/sessions/{session_id}/brief",
            get(get_brief),
        )
        .route("/capture/{backend}/briefs", get(list_briefs))
        .route("/capture/scan", post(start_scan))
        .route("/capture/scan/{job_id}", get(scan_status));

    #[cfg(feature = "mcp-tools")]
    let router = router
        .route(
            "/capture/tools/check_file_freshness",
            post(mcp_check_file_freshness),
        )
        .route(
            "/capture/tools/search_past_outputs",
            post(mcp_search_past_outputs),
        )
        .route(
            "/capture/tools/get_session_summary",
            post(mcp_get_session_summary),
        )
        .route(
            "/capture/tools/list_actions_around",
            post(mcp_list_actions_around),
        );

    router.with_state(state)
}

// ── Backend ↔ ToolKind mapping ────────────────────────────────────────────────

/// Map a Backend to the ToolKind an adapter would carry. Returns `None` for
/// Backends whose adapter feature is not compiled in — the caller returns a
/// `409 passive_capture_unsupported`.
fn backend_to_tool_kind(b: Backend) -> Option<ToolKind> {
    match b {
        #[cfg(feature = "claudecode")]
        Backend::Claude => Some(ToolKind::ClaudeCode),
        #[cfg(feature = "codex")]
        Backend::Codex => Some(ToolKind::Codex),
        #[cfg(feature = "opencode")]
        Backend::OpenCode => Some(ToolKind::OpenCode),
        _ => None,
    }
}

// ── GET /capture ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct DetectedAdapter {
    backend: &'static str,
    tool_kind: &'static str,
    watch_paths: Vec<String>,
    detected: bool,
    last_parse_at_ms: Option<i64>,
}

#[derive(Serialize)]
struct DetectedAdapters {
    adapters: Vec<DetectedAdapter>,
}

async fn list_adapters(State(cs): State<CaptureState>) -> Response {
    let adapters = cs.registry.all();
    if adapters.is_empty() {
        return error_response(
            StatusCode::CONFLICT,
            "passive_capture_unsupported",
            "no adapters registered",
        );
    }
    let body: Vec<DetectedAdapter> = adapters
        .iter()
        .map(|a| {
            let paths = a.watch_paths();
            let detected = paths.iter().any(|p| p.is_dir());
            DetectedAdapter {
                backend: tool_kind_to_backend_key(a.kind()),
                tool_kind: a.kind().as_str(),
                watch_paths: paths.iter().map(|p| p.display().to_string()).collect(),
                detected,
                last_parse_at_ms: cs.last_parse_ms(a.kind()),
            }
        })
        .collect();
    json_ok(StatusCode::OK, &DetectedAdapters { adapters: body })
}

async fn adapter_summary(
    State(cs): State<CaptureState>,
    AxumPath(backend_key): AxumPath<String>,
) -> Response {
    let (_, adapter) = match resolve_backend(&cs, &backend_key) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let paths = adapter.watch_paths();
    let body = DetectedAdapter {
        backend: tool_kind_to_backend_key(adapter.kind()),
        tool_kind: adapter.kind().as_str(),
        watch_paths: paths.iter().map(|p| p.display().to_string()).collect(),
        detected: paths.iter().any(|p| p.is_dir()),
        last_parse_at_ms: cs.last_parse_ms(adapter.kind()),
    };
    json_ok(StatusCode::OK, &body)
}

// ── GET /capture/{backend}/sessions ───────────────────────────────────────────

#[derive(Deserialize)]
struct SessionsQuery {
    cwd: Option<String>,
    #[serde(default = "default_limit")]
    limit: u32,
    #[serde(default)]
    offset: u32,
}

fn default_limit() -> u32 {
    100
}

async fn list_sessions(
    State(cs): State<CaptureState>,
    AxumPath(backend_key): AxumPath<String>,
    Query(q): Query<SessionsQuery>,
) -> Response {
    let (tool, _adapter) = match resolve_backend(&cs, &backend_key) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let cwd = q.cwd.as_deref().map(Path::new);
    match cs
        .event_store
        .sessions_for(tool, cwd, q.limit, q.offset)
        .await
    {
        Ok(sessions) => {
            let body: Vec<CapturedSession> = sessions
                .into_iter()
                .map(|s| CapturedSession {
                    backend: tool_kind_to_backend_key(s.tool),
                    session_id: s.session_id,
                    source_file: s.source_file.display().to_string(),
                    first_event_at_ms: s.first_event_at_ms,
                    last_event_at_ms: s.last_event_at_ms,
                    action_count: s.action_count,
                    tool_kinds: s.tool_kinds.iter().map(|k| k.as_str()).collect(),
                    git_root: s.git_root.map(|p| p.display().to_string()),
                })
                .collect();
            json_ok(StatusCode::OK, &body)
        }
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "store_error",
            &e.to_string(),
        ),
    }
}

#[derive(Serialize)]
struct CapturedSession {
    backend: &'static str,
    session_id: String,
    source_file: String,
    first_event_at_ms: i64,
    last_event_at_ms: i64,
    action_count: u64,
    tool_kinds: Vec<&'static str>,
    git_root: Option<String>,
}

// ── GET /capture/{backend}/sessions/{id}/actions ──────────────────────────────

#[derive(Deserialize)]
struct ActionsQuery {
    #[serde(default = "default_limit")]
    limit: u32,
    #[serde(default)]
    offset: u32,
}

async fn list_actions(
    State(cs): State<CaptureState>,
    AxumPath((backend_key, session_id)): AxumPath<(String, String)>,
    Query(q): Query<ActionsQuery>,
) -> Response {
    let (tool, _) = match resolve_backend(&cs, &backend_key) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    match cs
        .event_store
        .actions_for_session(tool, &session_id, q.limit, q.offset)
        .await
    {
        Ok(actions) => {
            if actions.is_empty() {
                // Distinguish "session exists but no actions yet" from
                // "session not found." Query the sessions list to check.
                match cs.event_store.sessions_for(tool, None, u32::MAX, 0).await {
                    Ok(sessions) if sessions.iter().any(|s| s.session_id == session_id) => {
                        json_ok(StatusCode::OK, &Vec::<()>::new())
                    }
                    _ => error_response(
                        StatusCode::NOT_FOUND,
                        "not_found",
                        &format!("session {session_id} not found"),
                    ),
                }
            } else {
                json_ok(StatusCode::OK, &actions)
            }
        }
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "store_error",
            &e.to_string(),
        ),
    }
}

// ── GET /capture/{backend}/sessions/{id}/brief ────────────────────────────────

async fn get_brief(
    State(cs): State<CaptureState>,
    AxumPath((backend_key, session_id)): AxumPath<(String, String)>,
) -> Response {
    let (tool, _) = match resolve_backend(&cs, &backend_key) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    match cs.event_store.brief_for(tool, &session_id).await {
        Ok(Some(brief)) => json_ok(StatusCode::OK, &brief),
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("no brief for session {session_id}"),
        ),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "store_error",
            &e.to_string(),
        ),
    }
}

// ── GET /capture/{backend}/briefs ─────────────────────────────────────────────

#[derive(Deserialize)]
struct BriefsQuery {
    #[serde(default = "default_limit")]
    limit: u32,
    #[serde(default)]
    offset: u32,
}

async fn list_briefs(
    State(cs): State<CaptureState>,
    AxumPath(backend_key): AxumPath<String>,
    Query(q): Query<BriefsQuery>,
) -> Response {
    let (tool, _) = match resolve_backend(&cs, &backend_key) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    match cs.event_store.briefs_for(tool, q.limit, q.offset).await {
        Ok(briefs) => json_ok(StatusCode::OK, &briefs),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "store_error",
            &e.to_string(),
        ),
    }
}

// ── POST /capture/scan ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ScanRequest {
    #[serde(default)]
    force: bool,
}

#[derive(Serialize)]
struct ScanAccepted {
    job_id: String,
}

async fn start_scan(State(cs): State<CaptureState>, body: Option<Json<ScanRequest>>) -> Response {
    if cs.registry.all().is_empty() {
        return error_response(
            StatusCode::CONFLICT,
            "passive_capture_unsupported",
            "no adapters registered",
        );
    }
    let force = body.map(|Json(b)| b.force).unwrap_or(false);
    let job_id = format!("scan_{}", Uuid::new_v4().simple());
    cs.scan_jobs.create(&job_id);

    let cs2 = cs.clone();
    let jid = job_id.clone();
    tokio::spawn(async move {
        run_scan(cs2, jid, force).await;
    });

    json_ok(StatusCode::ACCEPTED, &ScanAccepted { job_id })
}

// ── GET /capture/scan/{job_id} ────────────────────────────────────────────────

async fn scan_status(
    State(cs): State<CaptureState>,
    AxumPath(job_id): AxumPath<String>,
) -> Response {
    match cs.scan_jobs.get(&job_id) {
        Some(snapshot) => json_ok(StatusCode::OK, &snapshot),
        None => error_response(
            StatusCode::NOT_FOUND,
            "unknown_job",
            &format!("job {job_id} not found"),
        ),
    }
}

// ── ScanJobRegistry ───────────────────────────────────────────────────────────

#[derive(Default)]
pub struct ScanJobRegistry {
    jobs: Mutex<HashMap<String, ScanJob>>,
}

impl ScanJobRegistry {
    pub fn create(&self, job_id: &str) {
        let job = ScanJob {
            job_id: job_id.to_string(),
            state: ScanJobState::Queued,
            started_at_ms: now_ms(),
            completed_at_ms: None,
            files_scanned: 0,
            files_skipped: 0,
            events_upserted: 0,
            deduplicated_count: 0,
            warnings: Vec::new(),
            error: None,
        };
        self.jobs.lock().unwrap().insert(job_id.to_string(), job);
    }

    pub fn get(&self, job_id: &str) -> Option<ScanJobSnapshot> {
        self.jobs
            .lock()
            .unwrap()
            .get(job_id)
            .map(|j| ScanJobSnapshot {
                job_id: j.job_id.clone(),
                state: j.state,
                started_at_ms: j.started_at_ms,
                completed_at_ms: j.completed_at_ms,
                files_scanned: j.files_scanned,
                files_skipped: j.files_skipped,
                events_upserted: j.events_upserted,
                deduplicated_count: j.deduplicated_count,
                warnings: j.warnings.clone(),
                error: j.error.clone(),
            })
    }

    fn update<F>(&self, job_id: &str, f: F)
    where
        F: FnOnce(&mut ScanJob),
    {
        if let Some(j) = self.jobs.lock().unwrap().get_mut(job_id) {
            f(j);
        }
    }
}

#[derive(Debug, Clone)]
struct ScanJob {
    job_id: String,
    state: ScanJobState,
    started_at_ms: i64,
    completed_at_ms: Option<i64>,
    files_scanned: u64,
    files_skipped: u64,
    events_upserted: u64,
    deduplicated_count: u64,
    warnings: Vec<ParseWarning>,
    error: Option<String>,
}

/// Serializable view of a ScanJob for the `GET /scan/{job_id}` response.
#[derive(Serialize, Debug, Clone)]
pub struct ScanJobSnapshot {
    pub job_id: String,
    pub state: ScanJobState,
    pub started_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub files_scanned: u64,
    pub files_skipped: u64,
    pub events_upserted: u64,
    pub deduplicated_count: u64,
    pub warnings: Vec<ParseWarning>,
    pub error: Option<String>,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScanJobState {
    Queued,
    Running,
    Complete,
    #[allow(dead_code)]
    Failed,
}

// ── Shared scan pipeline ──────────────────────────────────────────────────────

/// The per-file pipeline lives in `aikit_session_capture::ingest` so this
/// route, the watch driver and the `aikit session` commands share one code
/// path (spec 010 §14.3). Re-exported under its historical name here.
pub use aikit_session_capture::ingest::{parse_and_store_file, IngestOutcome as ScanFileOutcome};

/// Run a full scan: walk each adapter's watch paths, parse every session file
/// found, aggregate results into the job.
async fn run_scan(cs: CaptureState, job_id: String, force: bool) {
    cs.scan_jobs.update(&job_id, |j| {
        j.state = ScanJobState::Running;
    });

    let mut outcome = ScanFileOutcome::default();
    for adapter in cs.registry.all() {
        cs.touch_last_parse(adapter.kind());
        outcome.absorb(
            aikit_session_capture::ingest::scan_adapter(
                adapter,
                cs.event_store.as_ref(),
                cs.cursor_store.as_ref(),
                force,
            )
            .await,
        );
    }

    cs.scan_jobs.update(&job_id, |j| {
        j.state = ScanJobState::Complete;
        j.completed_at_ms = Some(now_ms());
        j.files_scanned = outcome.files_scanned;
        j.files_skipped = outcome.files_skipped;
        j.events_upserted = outcome.events_upserted;
        j.deduplicated_count = outcome.deduplicated_count;
        j.warnings = outcome.warnings;
    });
}

// ── MCP tool routes (Phase 5, behind `mcp-tools` feature) ────────────────────

#[cfg(feature = "mcp-tools")]
async fn mcp_check_file_freshness(
    State(cs): State<CaptureState>,
    body: Option<Json<aikit_session_capture::mcp::CheckFileFreshnessArgs>>,
) -> Response {
    let args = match body {
        Some(Json(b)) => b,
        None => return error_response(StatusCode::BAD_REQUEST, "invalid_request", "missing body"),
    };
    match aikit_session_capture::mcp::check_file_freshness(cs.event_store.as_ref(), args).await {
        Ok(result) => json_ok(StatusCode::OK, &result),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "tool_error",
            &e.to_string(),
        ),
    }
}

#[cfg(feature = "mcp-tools")]
async fn mcp_search_past_outputs(
    State(cs): State<CaptureState>,
    body: Option<Json<aikit_session_capture::mcp::SearchPastOutputsArgs>>,
) -> Response {
    let args = match body {
        Some(Json(b)) => b,
        None => return error_response(StatusCode::BAD_REQUEST, "invalid_request", "missing body"),
    };
    match aikit_session_capture::mcp::search_past_outputs(cs.event_store.as_ref(), args).await {
        Ok(result) => json_ok(StatusCode::OK, &result),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "tool_error",
            &e.to_string(),
        ),
    }
}

#[cfg(feature = "mcp-tools")]
async fn mcp_get_session_summary(
    State(cs): State<CaptureState>,
    body: Option<Json<aikit_session_capture::mcp::GetSessionSummaryArgs>>,
) -> Response {
    let args = match body {
        Some(Json(b)) => b,
        None => return error_response(StatusCode::BAD_REQUEST, "invalid_request", "missing body"),
    };
    match aikit_session_capture::mcp::get_session_summary(cs.event_store.as_ref(), args).await {
        Ok(result) => json_ok(StatusCode::OK, &result),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "tool_error",
            &e.to_string(),
        ),
    }
}

#[cfg(feature = "mcp-tools")]
async fn mcp_list_actions_around(
    State(cs): State<CaptureState>,
    body: Option<Json<aikit_session_capture::mcp::ListActionsAroundArgs>>,
) -> Response {
    let args = match body {
        Some(Json(b)) => b,
        None => return error_response(StatusCode::BAD_REQUEST, "invalid_request", "missing body"),
    };
    match aikit_session_capture::mcp::list_actions_around(cs.event_store.as_ref(), args).await {
        Ok(result) => json_ok(StatusCode::OK, &result),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "tool_error",
            &e.to_string(),
        ),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Resolve a backend path parameter to a (ToolKind, &dyn Adapter) pair.
/// Returns a pre-built HTTP error Response on failure.
#[allow(clippy::result_large_err)]
fn resolve_backend<'a>(
    cs: &'a CaptureState,
    key: &str,
) -> Result<(ToolKind, &'a dyn Adapter), Response> {
    let backend = match Backend::from_key(key) {
        Some(b) => b,
        None => {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                "unknown_backend",
                &format!("backend '{key}' is not a known Backend"),
            ))
        }
    };
    let tool = match backend_to_tool_kind(backend) {
        Some(t) => t,
        None => {
            return Err(error_response(
                StatusCode::CONFLICT,
                "passive_capture_unsupported",
                &format!("backend '{key}' has no adapter registered"),
            ))
        }
    };
    match cs.registry.get(tool) {
        Some(a) => Ok((tool, a)),
        None => Err(error_response(
            StatusCode::CONFLICT,
            "passive_capture_unsupported",
            &format!("backend '{key}' has no adapter registered"),
        )),
    }
}

fn tool_kind_to_backend_key(k: ToolKind) -> &'static str {
    match k {
        ToolKind::ClaudeCode => "claude",
        ToolKind::Codex => "codex",
        ToolKind::OpenCode => "opencode",
        ToolKind::Cursor => "cursor",
        ToolKind::Gemini => "gemini",
        _ => "unknown",
    }
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn json_ok<T: Serialize>(status: StatusCode, body: &T) -> Response {
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(body).unwrap_or_else(|_| "{}".into()),
    )
        .into_response()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_session_capture::{
        ActionKind, ActionStatus, InMemoryCursorStore, InMemoryEventStore, ToolEvent,
    };
    use async_trait::async_trait;
    use std::path::PathBuf;

    fn sample_event(id: &str, sess: &str) -> ToolEvent {
        ToolEvent {
            source_event_id: id.into(),
            source_file: PathBuf::from("/tmp/sess.jsonl"),
            session_id: sess.into(),
            tool: ToolKind::ClaudeCode,
            kind: ActionKind::Read,
            target: None,
            input: None,
            output: None,
            status: ActionStatus::Success,
            error_message: None,
            started_at_ms: Some(1000),
            duration_ms: None,
            git_root: None,
            metadata: serde_json::Value::Null,
        }
    }

    #[tokio::test]
    async fn brief_routes_read_the_store() {
        use aikit_session_capture::{EventBatch, SessionBrief};
        use axum::body::to_bytes;

        struct NoAdapter;
        #[async_trait]
        impl Adapter for NoAdapter {
            fn kind(&self) -> ToolKind {
                ToolKind::ClaudeCode
            }
            fn watch_paths(&self) -> Vec<PathBuf> {
                vec![]
            }
            fn is_session_file(&self, _: &Path) -> bool {
                false
            }
            async fn parse_session_file(
                &self,
                _: &Path,
                _: u64,
            ) -> Result<aikit_session_capture::ParseResult, aikit_session_capture::AdapterError>
            {
                Ok(aikit_session_capture::ParseResult::default())
            }
        }

        let mut registry = Registry::new();
        registry.register(Box::new(NoAdapter));
        let store = Arc::new(InMemoryEventStore::new());
        store
            .upsert_events(EventBatch {
                tool_events: vec![sample_event("1", "s1")],
                ..Default::default()
            })
            .await
            .unwrap();
        store
            .put_brief(&SessionBrief {
                tool: ToolKind::ClaudeCode,
                session_id: "s1".into(),
                summary: "Read a file.".into(),
                areas: vec![],
                tags: vec![],
                model: "m".into(),
                digest_hash: "h".into(),
                generated_at_ms: 5,
                model_reported: None,
                rejected_tags: vec![],
                prompt_source: None,
            })
            .await
            .unwrap();
        let cs = CaptureState::new(registry, store, Arc::new(InMemoryCursorStore::default()));

        async fn body_json(resp: Response) -> (StatusCode, serde_json::Value) {
            let status = resp.status();
            let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
            (status, serde_json::from_slice(&bytes).unwrap())
        }

        let (status, body) = body_json(
            get_brief(
                State(cs.clone()),
                AxumPath(("claude".to_string(), "s1".to_string())),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["summary"], "Read a file.");
        assert_eq!(body["tool"], "claude_code");

        let (status, body) = body_json(
            get_brief(
                State(cs.clone()),
                AxumPath(("claude".to_string(), "missing".to_string())),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");

        let (status, body) = body_json(
            list_briefs(
                State(cs.clone()),
                AxumPath("claude".to_string()),
                Query(BriefsQuery {
                    limit: 10,
                    offset: 0,
                }),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().map(|a| a.len()), Some(1));

        // An unknown backend key is the same 404 the other routes return.
        let (status, _) = body_json(
            list_briefs(
                State(cs),
                AxumPath("nope".to_string()),
                Query(BriefsQuery {
                    limit: 10,
                    offset: 0,
                }),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn pipeline_idempotency_on_force_rescan() {
        // Parse the same file twice with force=true. The second run should
        // report deduplicated_count == events_upserted (idempotency proof).
        struct FakeAdapter;
        #[async_trait]
        impl Adapter for FakeAdapter {
            fn kind(&self) -> ToolKind {
                ToolKind::ClaudeCode
            }
            fn watch_paths(&self) -> Vec<PathBuf> {
                vec![]
            }
            fn is_session_file(&self, _: &Path) -> bool {
                true
            }
            async fn parse_session_file(
                &self,
                _: &Path,
                _: u64,
            ) -> Result<aikit_session_capture::ParseResult, aikit_session_capture::AdapterError>
            {
                Ok(aikit_session_capture::ParseResult {
                    tool_events: vec![sample_event("1", "s1"), sample_event("2", "s1")],
                    token_events: vec![],
                    cache_observations: vec![],
                    new_offset: 100,
                    warnings: vec![],
                    retry_suggested: false,
                })
            }
        }

        let store = Arc::new(InMemoryEventStore::new());
        let cursors = Arc::new(InMemoryCursorStore::default());
        let adapter = FakeAdapter;
        let path = Path::new("/tmp/sess.jsonl");

        let o1 = parse_and_store_file(&adapter, path, store.as_ref(), cursors.as_ref(), true)
            .await
            .unwrap();
        assert_eq!(o1.events_upserted, 2);
        assert_eq!(o1.deduplicated_count, 0);

        let o2 = parse_and_store_file(&adapter, path, store.as_ref(), cursors.as_ref(), true)
            .await
            .unwrap();
        assert_eq!(o2.events_upserted, 0);
        assert_eq!(o2.deduplicated_count, 2);
    }

    #[test]
    fn backend_mapping_returns_none_for_unsupported() {
        // Cursor/Gemini/Aikit never have adapters in this spec.
        assert!(backend_to_tool_kind(Backend::Cursor).is_none());
        assert!(backend_to_tool_kind(Backend::Gemini).is_none());
        assert!(backend_to_tool_kind(Backend::Aikit).is_none());
    }

    #[test]
    fn tool_kind_to_backend_key_roundtrips() {
        assert_eq!(tool_kind_to_backend_key(ToolKind::ClaudeCode), "claude");
        assert_eq!(tool_kind_to_backend_key(ToolKind::Codex), "codex");
        assert_eq!(tool_kind_to_backend_key(ToolKind::OpenCode), "opencode");
    }
}
