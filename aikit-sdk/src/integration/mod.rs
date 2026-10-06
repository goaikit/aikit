//! Reusable integration services for application consumers.
//!
//! The managed gateway store is extracted from the existing serving code. It
//! retains its recovery semantics and single-host ownership lock. External hook
//! invocations must not open it as independent gateway hosts: doing so would
//! interrupt managed sessions. External-session support is developed separately.

/// Existing managed-session persistence used by the HTTP gateway and embeddings.
pub mod gateway_store;
