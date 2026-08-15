//! History (transcript) backend — spec 008.
//!
//! A Backend-agnostic API for discovering and reading past agent sessions
//! without spawning any subprocess. Only Backends that truly persist
//! browsable history implement [`HistoryReader`]; every other Backend is
//! "unsupported," never merely empty (see [`HistoryError::Unsupported`]).
//!
//! Callers gate on the capability first, then construct:
//! - read: `if backend.capabilities().history_store { backend.history_reader() }`
//! - mutate: gate `history_store`, then `backend.history_mutator()`.
//!
//! See `specs/008-history-backend/spec.md` (gitignored) and ADR 0018 for the
//! full design rationale.

mod types;

#[cfg(feature = "claude-sdk")]
pub(crate) mod claude;

#[cfg(test)]
mod tests;

pub use types::{
    HistoryBlock, HistoryContent, HistoryError, HistoryMessage, HistoryQuery, HistorySession,
    MessagesQuery,
};

use std::path::Path;

use crate::runner::backend::Backend;

/// Discover and read past sessions for one Backend, without spawning a
/// subprocess.
pub trait HistoryReader: Send + Sync {
    /// Which Backend this reader serves.
    fn backend(&self) -> Backend;

    /// List sessions for a directory (or across all projects when
    /// `q.cwd` is `None`), newest-first, paged.
    fn list(&self, q: &HistoryQuery) -> Result<Vec<HistorySession>, HistoryError>;

    /// Metadata for one session, without scanning the whole project
    /// directory. `Ok(None)` when the session does not exist.
    fn info(&self, id: &str, cwd: Option<&Path>) -> Result<Option<HistorySession>, HistoryError>;

    /// Read a session's messages, paged. Returns
    /// [`HistoryError::NotFound`] if the session does not exist, and
    /// `Ok(vec![])` if it exists but has zero messages — the two must never
    /// be conflated (spec 008 §7).
    fn messages(
        &self,
        id: &str,
        q: &MessagesQuery,
        cwd: Option<&Path>,
    ) -> Result<Vec<HistoryMessage>, HistoryError>;
}

/// Metadata mutations (rename/tag) on top of a [`HistoryReader`].
///
/// A separate super-trait, not a method on [`HistoryReader`], because a
/// history store can be read-only (e.g. a future remote mirror): such a
/// store implements only `HistoryReader`, and `Backend::history_mutator`
/// returns `None` for it even though `Backend::history_reader` returns
/// `Some`.
pub trait HistoryMutator: HistoryReader {
    /// Set a custom display title for a session. Idempotent.
    fn rename(&self, id: &str, title: &str, cwd: Option<&Path>) -> Result<(), HistoryError>;

    /// Set or clear a session's tag. `None` clears; `Some(tag)` sets.
    /// Idempotent.
    fn tag(&self, id: &str, tag: Option<&str>, cwd: Option<&Path>) -> Result<(), HistoryError>;
}

impl Backend {
    /// Construct a [`HistoryReader`] for this Backend, if it has one.
    ///
    /// `None` when this Backend has no history store compiled in — the
    /// exhaustive match below is the single source of truth callers should
    /// gate on via `capabilities().history_store` first (spec 008 §6): the
    /// invariant `capabilities().history_store == true` iff
    /// `history_reader().is_some()` holds in every build configuration.
    pub fn history_reader(self) -> Option<Box<dyn HistoryReader>> {
        match self {
            #[cfg(feature = "claude-sdk")]
            Backend::Claude => Some(Box::new(claude::ClaudeHistory)),
            _ => None,
        }
    }

    /// Construct a [`HistoryMutator`] for this Backend, if its history store
    /// supports metadata mutations.
    ///
    /// A separate constructor from [`Backend::history_reader`], not a
    /// downcast: `Box<dyn HistoryReader>` cannot be upcast to
    /// `Box<dyn HistoryMutator>` in stable Rust. A read-only store would
    /// return `Some` here from `history_reader()` but `None` from this
    /// method.
    ///
    pub fn history_mutator(self) -> Option<Box<dyn HistoryMutator>> {
        match self {
            #[cfg(feature = "claude-sdk")]
            Backend::Claude => Some(Box::new(claude::ClaudeHistory)),
            _ => None,
        }
    }
}
