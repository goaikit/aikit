//! Transport: how a Backend's channel is established and how messages move
//! across it, in both directions.
//!
//! Phase A implements exactly one Transport — [`subprocess`] (spawn the CLI,
//! read newline-delimited stdout) — used by `runner::run_agent_events`. The
//! built-in `aikit` Backend is the *in-process* Transport: it emits canonical
//! events directly via `aikit_agent_adapter` (the `aikit` branch of
//! `run_agent_events`); it has no line Dialect.
//!
//! The trait below is the **seam** for Phase B. It mirrors
//! `claude-agent-sdk-rust`'s `Transport`/`TransportReader`/`TransportWriter`:
//! JSON-RPC-over-stdio (Codex `app-server`), the Claude SDK, websockets, and
//! unix sockets slot in later as additional impls — including a bidirectional
//! writer half (the Control axis: approvals / interrupts / turns), which is
//! *shaped for* here but not built in Phase A. Deliberately minimal: building
//! speculative transports before their backends exist is out of scope (ADR 0007).

pub(crate) mod subprocess;

use crate::runner::types::RunError;

/// Reader half of a Transport: yields inbound payloads from the agent.
///
/// Phase B will implement this for JSON-RPC / websocket / unix transports. The
/// Phase-A subprocess Transport delivers its inbound lines over an mpsc channel
/// (see [`subprocess::SubprocessConnection`]) rather than through this trait;
/// the trait fixes the *shape* that future transports conform to.
pub trait TransportReader: Send {
    /// Block for the next inbound payload. `Ok(None)` signals end of stream.
    fn next_payload(&mut self) -> Result<Option<Vec<u8>>, RunError>;
}

/// Writer half of a Transport: sends outbound payloads to the agent.
///
/// The seam for the Control axis (approvals, interrupts, turn/session drive).
/// Not exercised in Phase A — the current Backends are one-shot and read-only.
pub trait TransportWriter: Send {
    /// Write a raw payload (typically JSON + newline) to the agent.
    fn write_payload(&mut self, payload: &[u8]) -> Result<(), RunError>;
    /// Signal end-of-input (e.g. close stdin).
    fn close(&mut self) -> Result<(), RunError>;
}

/// A bidirectional Transport: establishes a channel and splits into reader and
/// writer halves. Phase B abstraction; Phase A uses [`subprocess::connect`]
/// directly.
pub trait Transport: Send {
    fn split(self: Box<Self>) -> (Box<dyn TransportReader>, Box<dyn TransportWriter>);
}
