//! `aikit-session-summarize`: session briefs for captured coding-agent
//! sessions (ADR 0023).
//!
//! Given sessions that `aikit-session-capture` has parsed into an
//! [`EventStore`](aikit_session_capture::EventStore), this crate builds a
//! bounded, scrubbed **digest** per session, decides the **mechanical tags**
//! in code, asks a model for one completion, validates the tags it returns
//! against the fixed list, and stores the resulting
//! [`SessionBrief`](aikit_session_capture::SessionBrief).
//!
//! The crate never spawns a tool and never reads a transcript. It only
//! reads the tools' session files and never writes to them: its one write is
//! the brief, through a host's `BriefStore`. It mirrors
//! `aikit-session-sync` as a sibling consumer of the capture crate.
//!
//! Modules:
//! - [`locate`]: which adapters and roots to scan, from `--tool` / `--path`.
//! - [`select`]: which captured sessions to act on (`--session`, `--since`, `--all`).
//! - [`brief`]: the `SessionBrief` record and the `BriefStore` trait a host implements.
//! - [`areas`]: deterministic area grouping with time attribution.
//! - [`tags`]: the tag list, the mechanical rules, model-tag validation.
//! - [`digest`]: the bounded model input built from events.
//! - [`prompt`]: the messages sent and the reply parsed.
//! - [`engine`]: the batch summarizer with bounded concurrency.

pub mod areas;
pub mod brief;
pub mod digest;
pub mod engine;
pub mod locate;
pub mod prompt;
pub mod select;
pub mod tags;

pub use areas::{group_areas, AreaMapping, AreaRule};
pub use brief::{
    AreaTouch, BriefStore, InMemoryBriefStore, SessionBrief, TagAssignment, TagSource,
};
pub use digest::{build_digest, Digest, DigestOptions, PromptsInput, RenderReport};
pub use engine::{
    is_transient, ModelConfig, Outcome, ProgressFn, PromptSource, SessionOutcome, SummarizeOptions,
    Summarizer,
};
pub use locate::{adapters_for, parse_location, parse_tool_kind, LocateError, LocationSpec};
pub use select::{parse_since, select_sessions, SelectError, Selection};
pub use tags::{
    mechanical_tags, validate_model_tags, Evidence, ModelTag, TagDefinition, TagList, TagListError,
    TagRule,
};
