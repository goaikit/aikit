//! Error types for the AIKIT CLI
//!
//! This module defines comprehensive error types using thiserror
//! for better error handling throughout the application.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AikError {
    #[error("Configuration error: {0}")]
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
}

impl From<Box<dyn std::error::Error>> for AikError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        AikError::Generic(err.to_string())
    }
}
