//! Core functionality for AIKIT

pub mod agent;
#[allow(dead_code)] // parsers used by RFC #6 CLI path; exercised in unit tests and lib re-exports
pub mod agent_definition;
pub mod fallback;
pub mod filesystem;
pub mod git;
pub mod llm_http;
pub mod lock;
pub mod package;
pub mod registry;
pub mod template;
pub mod tools;
pub mod ux;
pub mod validation;
