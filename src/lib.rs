//! AIKIT: multi-agent template package manager and CLI.
//!
//! Core logic for `aikit.toml` template packages: install from GitHub or local paths,
//! map templates into many coding-assistant layouts, init/scaffold, publish, and registry.
//! Runnable agents and event streaming live in the `aikit-sdk` crate.

pub mod cli;
pub mod core;
pub mod error;
pub mod fs;
pub mod git;
pub mod github;
pub mod models;
pub mod tui;

pub use error::AikError;
/// Re-export commonly used types
pub use models::{
    config::{load_config, save_config, AikConfig},
    package::Package,
    registry::{LocalRegistry, RemoteRegistry},
};

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Get version string
pub fn version() -> &'static str {
    VERSION
}
