//! Error types for the AIKIT CLI
//!
//! This module defines comprehensive error types using thiserror

//! for better error handling throughout the application.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AikError {
    #[error("Configuration error: {0}")]
    #[allow(dead_code)]
    Config(String),

    #[error("Invalid source: {0}")]
    InvalidSource(String),

    #[error("GitHub API error: {0}")]
    GitHubApi(#[from] reqwest::Error),

    #[error("Package validation error: {0}")]
    PackageValidation(String),

    #[error("Installation error: {0}")]
    Installation(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parsing error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("TOML serialization error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("ZIP archive error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Invalid GitHub URL format: {0}")]
    InvalidGitHubUrl(String),

    #[error("Package not found: {0}")]
    PackageNotFound(String),

    #[error("Version format error: {0}")]
    InvalidVersion(String),

    #[error("Generic error: {0}")]
    Generic(String),

    #[error("{0}")]
    Llm(#[from] crate::core::llm_http::LlmError),

    #[error("command not found in registry: {0}")]
    #[allow(dead_code)]
    CommandNotFound(String),

    #[error("argument validation failed: {0}")]
    #[allow(dead_code)]
    ValidationError(String),

    #[error("risk policy blocked command '{0}': set ALLOW_DESTRUCTIVE_COMMANDS=1 to proceed")]
    #[allow(dead_code)]
    RiskPolicyBlocked(String),

    #[error("no LLM provider configured: set AIKIT_LLM_URL + AIKIT_MODEL, or OPENAI_API_KEY / ANTHROPIC_API_KEY")]
    #[allow(dead_code)]
    LlmNotConfigured,

    #[error(
        "ailoop server unreachable at {0}: verify AILOOP_SERVER is set and the server is running"
    )]
    #[allow(dead_code)]
    AiloopUnreachable(String),

    #[error("MCP serve startup failed: {0}")]
    #[allow(dead_code)]
    McpServeError(String),
}

impl From<Box<dyn std::error::Error>> for AikError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        AikError::Generic(err.to_string())
    }
}

/// Attach path and operation context to an I/O error for clearer diagnostics.
pub fn io_context(operation: &str, path: &std::path::Path, err: std::io::Error) -> AikError {
    AikError::Generic(format!("{} (path: {}): {}", operation, path.display(), err))
}
