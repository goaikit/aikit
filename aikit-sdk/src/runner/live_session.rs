//! ARCH-3: the `LiveSession` control-surface trait.
//!
//! Bidirectional sessions (`claude_session`, `codex_session`) expose near-identical
//! control handles. Callers — `aikit serve`'s live-session registry and the
//! `aikit session` REPL — drive them through the same three core operations
//! (`send_turn` / `interrupt` / `disconnect`) plus two Claude-only operations
//! (`set_model` / `get_context_usage`). This trait unifies that surface so both
//! call sites store a `Box<dyn LiveSession>` instead of matching on the agent,
//! and so the session/serve lifecycle can be exercised against a fake in tests
//! without a real `claude`/`codex` binary.
//!
//! Backend-specific operations are **default methods** that return
//! [`ControlError::Unsupported`]; a backend that supports one overrides it. This
//! mirrors the `try_*` + `not_supported` shape the serve layer already used.
//!
//! The trait is HTTP-free: it returns a domain [`ControlError`]; the serve layer
//! maps `Unsupported` → 422 and `Backend` → 500, the CLI maps to `anyhow`.

/// Error from a [`LiveSession`] control operation.
#[derive(Debug)]
pub enum ControlError {
    /// The backend does not support this operation (e.g. `set_model` on Codex).
    /// The `&'static str` is the operation name, for the caller's message.
    Unsupported(&'static str),
    /// The backend rejected the operation or its control channel is closed.
    Backend(String),
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ControlError::Unsupported(op) => write!(f, "{op} is not supported by this agent"),
            ControlError::Backend(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ControlError {}

/// The control surface of a live bidirectional agent session.
///
/// Object-safe: methods take owned `String` (not `impl Into<String>`) so the
/// trait can be used as `Box<dyn LiveSession>` / `Arc<dyn LiveSession>`.
pub trait LiveSession: Send {
    /// Send a follow-up user turn on the same session.
    fn send_turn(&self, text: String) -> Result<(), ControlError>;

    /// Interrupt the current turn.
    fn interrupt(&self) -> Result<(), ControlError>;

    /// Disconnect and tear down the session.
    fn disconnect(&self) -> Result<(), ControlError>;

    /// Switch the model mid-session. Unsupported by default.
    fn set_model(&self, _model: Option<String>) -> Result<(), ControlError> {
        Err(ControlError::Unsupported("set_model"))
    }

    /// Fetch context-window usage. Unsupported by default.
    fn get_context_usage(&self) -> Result<serde_json::Value, ControlError> {
        Err(ControlError::Unsupported("get_context_usage"))
    }
}

#[cfg(feature = "claude-control")]
impl LiveSession for super::claude_session::ControlHandle {
    fn send_turn(&self, text: String) -> Result<(), ControlError> {
        self.send_turn(text)
            .map_err(|e| ControlError::Backend(e.to_string()))
    }
    fn interrupt(&self) -> Result<(), ControlError> {
        super::claude_session::ControlHandle::interrupt(self)
            .map_err(|e| ControlError::Backend(e.to_string()))
    }
    fn disconnect(&self) -> Result<(), ControlError> {
        super::claude_session::ControlHandle::disconnect(self)
            .map_err(|e| ControlError::Backend(e.to_string()))
    }
    fn set_model(&self, model: Option<String>) -> Result<(), ControlError> {
        super::claude_session::ControlHandle::set_model(self, model)
            .map_err(|e| ControlError::Backend(e.to_string()))
    }
    fn get_context_usage(&self) -> Result<serde_json::Value, ControlError> {
        super::claude_session::ControlHandle::get_context_usage(self)
            .map_err(|e| ControlError::Backend(e.to_string()))
    }
}

#[cfg(feature = "codex-app-server")]
impl LiveSession for super::codex_session::CodexControlHandle {
    fn send_turn(&self, text: String) -> Result<(), ControlError> {
        self.send_turn(text)
            .map_err(|e| ControlError::Backend(e.to_string()))
    }
    fn interrupt(&self) -> Result<(), ControlError> {
        super::codex_session::CodexControlHandle::interrupt(self)
            .map_err(|e| ControlError::Backend(e.to_string()))
    }
    fn disconnect(&self) -> Result<(), ControlError> {
        super::codex_session::CodexControlHandle::disconnect(self)
            .map_err(|e| ControlError::Backend(e.to_string()))
    }
    // set_model / get_context_usage fall through to the default `Unsupported`.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A `LiveSession` double: records calls, never touches a real process.
    #[derive(Default)]
    struct FakeSession {
        turns: AtomicUsize,
        interrupts: AtomicUsize,
        disconnects: AtomicUsize,
    }
    impl LiveSession for FakeSession {
        fn send_turn(&self, _text: String) -> Result<(), ControlError> {
            self.turns.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn interrupt(&self) -> Result<(), ControlError> {
            self.interrupts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn disconnect(&self) -> Result<(), ControlError> {
            self.disconnects.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn core_ops_dispatch_through_boxed_trait() {
        let s: Box<dyn LiveSession> = Box::new(FakeSession::default());
        assert!(s.send_turn("hi".into()).is_ok());
        assert!(s.interrupt().is_ok());
        assert!(s.disconnect().is_ok());
    }

    #[test]
    fn backend_specific_ops_default_to_unsupported() {
        let s: Box<dyn LiveSession> = Box::new(FakeSession::default());
        assert!(matches!(
            s.set_model(None),
            Err(ControlError::Unsupported("set_model"))
        ));
        assert!(matches!(
            s.get_context_usage(),
            Err(ControlError::Unsupported("get_context_usage"))
        ));
    }

    #[test]
    fn arc_dyn_is_usable_from_multiple_holders() {
        let s: Arc<dyn LiveSession> = Arc::new(FakeSession::default());
        let a = Arc::clone(&s);
        let b = Arc::clone(&s);
        assert!(a.send_turn("one".into()).is_ok());
        assert!(b.interrupt().is_ok());
    }

    #[test]
    fn control_error_display_names_the_op() {
        assert_eq!(
            ControlError::Unsupported("set_model").to_string(),
            "set_model is not supported by this agent"
        );
        assert_eq!(ControlError::Backend("boom".into()).to_string(), "boom");
    }
}
