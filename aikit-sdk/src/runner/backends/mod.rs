//! Per-Backend modules: everything about one agent lives in one file
//! (decode + token-usage + quota + argv + capabilities). Dispatched by the
//! [`Backend`](crate::runner::backend::Backend) enum. See spec 006.

pub(crate) mod argv_spec;
pub(crate) mod quota_match;

pub(crate) mod aikit;
pub(crate) mod claude;
pub(crate) mod codex;
pub(crate) mod cursor;
pub(crate) mod gemini;
pub(crate) mod opencode;
