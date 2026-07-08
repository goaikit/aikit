//! Production SQLite storage backends for session capture (spec 010 §11.2).
//!
//! [`SqliteEventStore`] and [`SqliteCursorStore`] are the host-implemented
//! production backends for the `EventStore` and `CursorStore` traits defined in
//! `aikit-session-capture`. Both share one SQLite DB file with three tables:
//!
//! - `capture_tool_events`   — normalized tool-call rows
//! - `capture_token_events`  — per-turn token-usage rows
//! - `capture_cursors`       — one row per source file (resume offset)
//!
//! The `(source_file, source_event_id)` uniqueness invariant on both event
//! tables is the idempotency contract: `INSERT OR IGNORE` makes a full re-walk
//! safe (previously-seen rows are silently deduplicated).

pub mod cursor_store;
pub mod event_store;
pub mod schema;

pub use cursor_store::SqliteCursorStore;
pub use event_store::SqliteEventStore;
