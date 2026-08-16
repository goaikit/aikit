//! `aikit serve` history routes — the HTTP surface for spec 008 (history /
//! transcript backend).
//!
//! Four routes under `/api/v1/history`:
//! - `GET   /history/{backend}`                    — list sessions
//! - `GET   /history/{backend}/{session_id}`        — one session's metadata
//! - `GET   /history/{backend}/{session_id}/messages` — a session's messages
//! - `PATCH /history/{backend}/{session_id}`        — rename/tag mutations
//!
//! Status-code contract (authoritative:
//! `specs/008-history-backend/contracts/history-api-contract.md`, gitignored):
//! unknown `{backend}` key → `404 unknown_backend`; a known Backend with no
//! history store → `409 history_unsupported`; a store with no mutator (or
//! `history_mutations=false`) → `409 mutations_unsupported` on `PATCH`;
//! `HistoryError::InvalidId` → `400 invalid_id`; `HistoryError::NotFound` →
//! `404 not_found`; every other `HistoryError` → `500`. `limit`/`offset` are
//! typed `usize` — an unparseable query value fails axum's `Query` extractor
//! (→ its own `400`) before this module ever runs; `0`/absent and oversized
//! values are coerced by the adapter (spec 008 §8), never rejected here.
//!
//! No shared state: [`Backend::history_reader`]/[`Backend::history_mutator`]
//! are constructed fresh per request, so this router carries no `AppState`
//! and merges directly into the stateless domain router (mirrors
//! `capture.rs`'s pattern, minus the `CaptureState`).

use std::path::PathBuf;

use axum::extract::{Path as AxumPath, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use aikit_sdk::history::{
    HistoryError, HistoryMutator, HistoryQuery, HistoryReader, MessagesQuery,
};
use aikit_sdk::runner::Backend;

/// Build the history router. Stateless (`Router<()>`) — merge directly into
/// the domain router alongside the other stateless/stateful sub-routers.
pub fn build_router() -> Router {
    Router::new()
        .route("/history/{backend}", get(list_history_handler))
        .route(
            "/history/{backend}/{session_id}",
            get(get_session_handler).patch(patch_session_handler),
        )
        .route(
            "/history/{backend}/{session_id}/messages",
            get(list_messages_handler),
        )
}

// ── GET /history/{backend} ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ListHistoryQuery {
    cwd: Option<String>,
    tag: Option<String>,
    limit: Option<usize>,
    #[serde(default)]
    offset: usize,
}

async fn list_history_handler(
    AxumPath(backend_key): AxumPath<String>,
    Query(q): Query<ListHistoryQuery>,
) -> Response {
    let backend = match resolve_backend(&backend_key) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let reader = match resolve_reader(backend) {
        Ok(r) => r,
        Err(r) => return r,
    };
    // `HistoryQuery` is `#[non_exhaustive]`, which forbids struct-literal
    // construction (even with `..Default::default()`) from outside its
    // crate — build via `Default` then assign the `pub` fields.
    let mut query = HistoryQuery::default();
    query.cwd = q.cwd.map(PathBuf::from);
    query.tag = q.tag;
    query.limit = q.limit;
    query.offset = q.offset;
    match reader.list(&query) {
        Ok(sessions) => json_ok(StatusCode::OK, &sessions),
        Err(e) => map_history_error(e),
    }
}

// ── GET /history/{backend}/{session_id} ───────────────────────────────────────

#[derive(Deserialize)]
struct CwdQuery {
    cwd: Option<String>,
}

async fn get_session_handler(
    AxumPath((backend_key, session_id)): AxumPath<(String, String)>,
    Query(q): Query<CwdQuery>,
) -> Response {
    let backend = match resolve_backend(&backend_key) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let reader = match resolve_reader(backend) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let cwd = q.cwd.map(PathBuf::from);
    match reader.info(&session_id, cwd.as_deref()) {
        Ok(Some(session)) => json_ok(StatusCode::OK, &session),
        Ok(None) => not_found(&session_id),
        Err(e) => map_history_error(e),
    }
}

// ── GET /history/{backend}/{session_id}/messages ──────────────────────────────

#[derive(Deserialize)]
struct MessagesQueryParams {
    cwd: Option<String>,
    limit: Option<usize>,
    #[serde(default)]
    offset: usize,
}

async fn list_messages_handler(
    AxumPath((backend_key, session_id)): AxumPath<(String, String)>,
    Query(q): Query<MessagesQueryParams>,
) -> Response {
    let backend = match resolve_backend(&backend_key) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let reader = match resolve_reader(backend) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let cwd = q.cwd.map(PathBuf::from);
    // Same `#[non_exhaustive]` construction constraint as `HistoryQuery` above.
    let mut query = MessagesQuery::default();
    query.limit = q.limit;
    query.offset = q.offset;
    match reader.messages(&session_id, &query, cwd.as_deref()) {
        Ok(messages) => json_ok(StatusCode::OK, &messages),
        Err(e) => map_history_error(e),
    }
}

// ── PATCH /history/{backend}/{session_id} ─────────────────────────────────────

/// `tag` is three-state (spec 008 §8 contract): absent = leave untouched,
/// `null` = clear, a string = set/replace. A plain `Option<String>` cannot
/// distinguish "absent" from "null" — both deserialize to `None` — so `tag`
/// is `Option<Option<String>>` via [`double_option`]: `None` (outer) = the
/// key was absent from the JSON body; `Some(None)` = present and `null`;
/// `Some(Some(v))` = present with a value.
#[derive(Deserialize)]
struct PatchBody {
    rename: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    tag: Option<Option<String>>,
}

/// Turn a normal one-level-`Option` deserialize into a two-level one: the
/// field is present in the body (its value becoming `Some(Option<T>)`)
/// whenever this function runs at all — `#[serde(default)]` on the field
/// supplies the `None` (absent) case without ever calling this. Standard
/// `serde_with::rust::double_option`-equivalent, written by hand to avoid
/// adding a new workspace dependency for one field.
fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}

#[derive(Deserialize)]
struct PatchQuery {
    cwd: Option<String>,
}

async fn patch_session_handler(
    AxumPath((backend_key, session_id)): AxumPath<(String, String)>,
    Query(q): Query<PatchQuery>,
    body: Option<Json<PatchBody>>,
) -> Response {
    let backend = match resolve_backend(&backend_key) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let mutator = match resolve_mutator(backend) {
        Ok(m) => m,
        Err(r) => return r,
    };

    let body = body.map(|Json(b)| b).unwrap_or(PatchBody {
        rename: None,
        tag: None,
    });
    if body.rename.is_none() && body.tag.is_none() {
        return err_json(StatusCode::BAD_REQUEST, json!({"error": "empty_patch"}));
    }

    let cwd = q.cwd.map(PathBuf::from);

    if let Some(title) = &body.rename {
        if let Err(e) = mutator.rename(&session_id, title, cwd.as_deref()) {
            return map_history_error(e);
        }
    }
    if let Some(tag_value) = body.tag {
        if let Err(e) = mutator.tag(&session_id, tag_value.as_deref(), cwd.as_deref()) {
            return map_history_error(e);
        }
    }

    StatusCode::NO_CONTENT.into_response()
}

// ── Backend / capability resolution ───────────────────────────────────────────

/// Resolve a `{backend}` path segment to a [`Backend`]. Unknown key → `404
/// unknown_backend` (spec 008 §8: distinct from "known but unsupported").
#[allow(clippy::result_large_err)]
fn resolve_backend(key: &str) -> Result<Backend, Response> {
    Backend::from_key(key).ok_or_else(|| {
        err_json(
            StatusCode::NOT_FOUND,
            json!({"error": "unknown_backend", "backend": key}),
        )
    })
}

/// Resolve a reader for `backend`, gating on the capability first per the
/// seam's invariant (`history_store == true` iff `history_reader()` is
/// `Some` — aikit-sdk/src/history/mod.rs) so a `None` reader can never slip
/// through as anything other than `409 history_unsupported`.
#[allow(clippy::result_large_err)]
fn resolve_reader(backend: Backend) -> Result<Box<dyn HistoryReader>, Response> {
    if !backend.capabilities().history_store {
        return Err(history_unsupported(backend));
    }
    backend
        .history_reader()
        .ok_or_else(|| history_unsupported(backend))
}

/// Resolve a mutator for `backend`. Two-stage, matching the contract's PATCH
/// table: no history store at all → `409 history_unsupported`; a store that
/// exists but doesn't support mutations → `409 mutations_unsupported`.
#[allow(clippy::result_large_err)]
fn resolve_mutator(backend: Backend) -> Result<Box<dyn HistoryMutator>, Response> {
    let caps = backend.capabilities();
    if !caps.history_store {
        return Err(history_unsupported(backend));
    }
    if !caps.history_mutations {
        return Err(mutations_unsupported());
    }
    backend.history_mutator().ok_or_else(mutations_unsupported)
}

fn history_unsupported(backend: Backend) -> Response {
    err_json(
        StatusCode::CONFLICT,
        json!({"error": "history_unsupported", "backend": backend.key()}),
    )
}

fn mutations_unsupported() -> Response {
    err_json(
        StatusCode::CONFLICT,
        json!({"error": "mutations_unsupported"}),
    )
}

fn not_found(session_id: &str) -> Response {
    err_json(
        StatusCode::NOT_FOUND,
        json!({"error": "not_found", "session_id": session_id}),
    )
}

/// Map a [`HistoryError`] to its contract-shaped HTTP response.
/// `#[non_exhaustive]` upstream, so this needs a wildcard arm even though
/// every current variant is covered explicitly.
fn map_history_error(e: HistoryError) -> Response {
    match e {
        HistoryError::Unsupported { backend } => history_unsupported(backend),
        HistoryError::NotFound { session_id } => not_found(&session_id),
        HistoryError::InvalidId { session_id } => err_json(
            StatusCode::BAD_REQUEST,
            json!({"error": "invalid_id", "session_id": session_id}),
        ),
        HistoryError::Io { .. } | HistoryError::Decode { .. } | HistoryError::Store { .. } => {
            err_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": "internal_error", "message": e.to_string()}),
            )
        }
        _ => err_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error": "internal_error", "message": e.to_string()}),
        ),
    }
}

// ── response helpers ───────────────────────────────────────────────────────────

fn json_ok<T: Serialize>(status: StatusCode, body: &T) -> Response {
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(body).unwrap_or_else(|_| "{}".into()),
    )
        .into_response()
}

fn err_json(status: StatusCode, body: serde_json::Value) -> Response {
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}
