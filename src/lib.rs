//! AIKIT - Universal Package Manager for AI Agent Extensions
//!
//! This library provides the core functionality for AIKIT's universal package system,
//! enabling the creation, distribution, and installation of reusable content
//! (prompts, templates, scripts, configurations) across different AI agents.

#![allow(dead_code)]

pub mod cli;
pub mod core;
pub mod error;
pub mod fs;
pub mod git;
pub mod github;
pub mod models;
pub mod tui;

pub use core::registry::RegistryManager;
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
