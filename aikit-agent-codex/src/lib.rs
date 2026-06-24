//! Async Rust client for the OpenAI Codex `app-server` JSON-RPC protocol.
//!
//! Drives a Codex session over stdio: spawn `codex app-server`, complete the
//! `initialize` handshake, create threads, send turns, and stream agent output
//! as JSON-RPC notifications. See `examples/chat.rs` for an end-to-end demo
//! and [`CodexClient`] for the entry point.

pub mod client;
pub mod error;
pub mod events;
pub mod protocol;

pub use client::{CodexClient, SpawnOptions};
pub use error::CodexError;
pub use events::{ServerMessage, ServerNotification, ServerNotificationKind, ServerRequest};
pub use protocol::{RequestId, ThreadId, TurnId};

pub use error::Result;
