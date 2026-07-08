//! `aikit-session-capture`: on-disk session adapters for AI coding tools.
//!
//! Turns one AI tool's persisted session data (Claude Code JSONL, Codex
//! rollout JSONL, OpenCode SQLite, …) into normalized `ToolEvent` /
//! `TokenEvent` rows that the host can store, query, and surface over MCP.
//!
//! The crate is **read-only and side-effect-free**: it parses what the tool
//! already wrote; it never spawns or drives the tool. Spawning remains the
//! runner's job (spec 006).
//!
//! # Status
//!
//! Scaffold — types and traits land per spec 010 phasing. This module
//! re-exports the public surface as it is implemented; today everything is
//! a stub that compiles green with `--no-default-features`.
//!
//! # Contracts (spec 010 §6)
//!
//! Implementations MUST:
//! - Scrub raw tool inputs before returning — no secrets escape the adapter.
//!   The injected [`SecretScrubber`] is the single chokepoint.
//! - Emit deterministic `source_event_id`s so re-parsing is idempotent.
//! - Advance `new_offset` past the last fully-parsed byte (or, for SQLite
//!   adapters, the last-consumed watermark), so the caller can persist it
//!   and skip on the next call.
//!
//! [`SecretScrubber`]: scrub::SecretScrubber

pub mod adapter;
pub mod cursor_offset;
pub mod event_store;
pub mod homes;
pub mod models;
pub mod registry;
pub mod scrub;

#[cfg(feature = "claudecode")]
pub mod claudecode;
#[cfg(feature = "codex")]
pub mod codex;
#[cfg(feature = "opencode")]
pub mod opencode;

#[cfg(feature = "mcp-tools")]
pub mod mcp;

#[cfg(feature = "watcher")]
pub mod watch;

pub use adapter::{Adapter, AdapterError, ParseResult, ParseWarning};
pub use cursor_offset::{CursorStore, InMemoryCursorStore, JsonSidecarCursorStore, ParseCursor};
pub use event_store::{
    EventBatch, EventStore, FileTouch, InMemoryEventStore, SessionSummary, StoreError,
};
pub use homes::{DefaultHomeResolver, HomeOs, HomeResolver, HomeRoot};
pub use models::{
    ActionKind, ActionStatus, CacheObservation, CaptureSource, TokenEvent, ToolEvent, ToolKind,
};
pub use registry::Registry;
pub use scrub::SecretScrubber;
