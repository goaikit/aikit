//! The `Adapter` trait: turns one AI tool's on-disk session data into
//! normalized events.
//!
//! See spec 010 §6. Implementations live in sibling modules
//! (`claudecode`, `codex`, `opencode`).

use std::path::Path;

use async_trait::async_trait;

use crate::models::{CacheObservation, TokenEvent, ToolEvent, ToolKind};

/// Turns one AI coding tool's on-disk session data into normalized events.
///
/// Implementations MUST (spec 010 §6):
/// - Scrub raw tool inputs before returning — no secrets escape the adapter.
///   The injected [`SecretScrubber`][crate::SecretScrubber] is the chokepoint.
/// - Emit deterministic `source_event_id`s so re-parsing is idempotent.
/// - Advance `new_offset` past the last fully-parsed byte (or, for SQLite
///   adapters, the last-consumed watermark), so the caller can persist it
///   and skip on the next call.
///
/// The meaning of `from_offset` / `new_offset` is **adapter-defined**: byte
/// offsets for line-oriented JSONL adapters (Claude Code, Codex), or
/// `time_updated` watermarks for the SQLite-based OpenCode adapter. The
/// trait keeps the value opaque as `u64`; the host records `adapter_kind`
/// alongside the cursor so it can disambiguate on re-load.
#[async_trait]
pub trait Adapter: Send + Sync {
    /// Stable identifier; one of [`ToolKind`]. Stored in the `tool` column.
    fn kind(&self) -> ToolKind;

    /// Directories to monitor for new/changed session files. Paths that do
    /// not exist are skipped at registry time — adapters should return their
    /// canonical path regardless of installed state.
    fn watch_paths(&self) -> Vec<std::path::PathBuf>;

    /// Filters watcher events. True if `path` is a session file this adapter
    /// should parse.
    fn is_session_file(&self, path: &Path) -> bool;

    /// Parse `path` from `from_offset` to EOF (or, for SQLite adapters,
    /// rows with `time_updated > from_offset`).
    ///
    /// Malformed records are skipped, not fatal — implementations advance
    /// past them so repeated calls make progress.
    async fn parse_session_file(
        &self,
        path: &Path,
        from_offset: u64,
    ) -> Result<ParseResult, AdapterError>;
}

/// Value returned by [`Adapter::parse_session_file`]. Mirrors
/// `superbased-observer/internal/adapter/adapter.go:40`.
#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    pub tool_events: Vec<ToolEvent>,
    pub token_events: Vec<TokenEvent>,
    pub cache_observations: Vec<CacheObservation>,
    /// Offset to persist for the next call. For JSONL adapters this is a
    /// byte offset; for SQLite adapters it is a `time_updated` watermark.
    pub new_offset: u64,
    pub warnings: Vec<ParseWarning>,
    /// Ask the host to keep the file on the poll loop even when the offset
    /// didn't advance (e.g. a foreign-mount SQLite file that hit
    /// `SQLITE_IOERR_SHORT_READ` and may succeed on retry).
    pub retry_suggested: bool,
}

/// Non-fatal parse issues. Adapters skip the offending record, advance past
/// it, and emit a warning so the host can surface "watcher had trouble with
/// this file" without blocking ingestion.
#[non_exhaustive]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseWarning {
    MalformedLine {
        line_no: u64,
        reason: String,
    },
    UnknownToolVariant {
        raw: String,
    },
    TruncatedRecord {
        line_no: u64,
    },
    /// OpenCode foreign-mount mirror retry — see spec 010 §12.3.
    ForeignMountRetry {
        path: String,
        reason: String,
    },
    Other {
        message: String,
    },
}

/// Fatal conditions only — things that prevent any further progress on this
/// file. Malformed individual records are [`ParseWarning`]s, not errors.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("io error reading {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("malformed session file {path} at byte {offset}: {reason}")]
    Malformed {
        path: std::path::PathBuf,
        offset: u64,
        reason: String,
    },
    #[error("offset {requested} is past EOF ({file_size}) for {path}")]
    OffsetPastEof {
        path: std::path::PathBuf,
        requested: u64,
        file_size: u64,
    },
    #[error("scrubber pattern invalid: {0}")]
    ScrubberPattern(#[from] regex::Error),
    #[error("home resolution failed: {0}")]
    HomeResolution(String),
    #[cfg(feature = "opencode")]
    #[error("sqlite open failed for {path}: {source}")]
    SqliteOpen {
        path: std::path::PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[cfg(feature = "opencode")]
    #[error("foreign-mount mirror failed for {path}: {reason}")]
    ForeignMountMirror {
        path: std::path::PathBuf,
        reason: String,
    },
    #[error("other: {0}")]
    Other(#[from] anyhow::Error),
}
