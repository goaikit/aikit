//! GitHub API client
//!
//! This module handles all interactions with the GitHub API, including:
//! - Release and asset downloads
//! - Authentication handling
//! - Rate limit detection

use crate::github::rate_limit::GitHubRateLimitInfo;
use anyhow::{Context, Result};
use reqwest::Client;
use std::time::Duration;

/// GitHub API client
#[allow(dead_code)]
pub struct GitHubClient {
    client: Client,
    base_url: String,
    token: Option<String>,
}

#[allow(dead_code)]
impl GitHubClient {
    /// Create a new GitHub API client
    pub fn new(token: Option<String>) -> Result<Self> {
        Self::with_skip_tls(token, false)
    }

    /// Create a new GitHub API client with optional TLS skipping
    ///
    /// Note: TLS skipping is unsafe and should only be used for troubleshooting.
    /// The current reqwest implementation with rustls-tls doesn't support skipping
    /// TLS verification easily. This flag is accepted but currently has no effect.
    pub fn with_skip_tls(token: Option<String>, skip_tls: bool) -> Result<Self> {
        if skip_tls {
            eprintln!("[WARNING] --skip-tls is not fully supported with rustls-tls backend");
            eprintln!("[WARNING] TLS verification will still be performed");
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            client,
            base_url: "https://api.github.com".to_string(),
            token,
        })
    }

    /// Resolve GitHub token from multiple sources
    ///
    /// Precedence order:
    /// 1. CLI argument (provided token)
    /// 2. GH_TOKEN environment variable
    /// 3. GITHUB_TOKEN environment variable
    pub fn resolve_token(cli_token: Option<String>) -> Option<String> {
        cli_token
            .or_else(|| std::env::var("GH_TOKEN").ok())
            .or_else(|| std::env::var("GITHUB_TOKEN").ok())
    }

    /// Create request headers with authentication if available
    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static("aikit/0.1.0"),
        );

        if let Some(token) = &self.token {
            let auth_value = format!("token {}", token);
            if let Ok(header_value) = reqwest::header::HeaderValue::from_str(&auth_value) {
                headers.insert(reqwest::header::AUTHORIZATION, header_value);
            }
        }

        headers
    }

    /// Check for rate limit errors in response
    pub fn check_rate_limit(&self, response: &reqwest::Response) -> Result<()> {
        if response.status() == reqwest::StatusCode::FORBIDDEN {
            if let Some(rate_limit_info) = GitHubRateLimitInfo::from_headers(response.headers()) {
                if rate_limit_info.is_exceeded() {
                    let is_authenticated = self.token.is_some();
                    let error_msg = rate_limit_info.format_error_message(is_authenticated);
                    return Err(anyhow::anyhow!(error_msg));
                }
            }
        }

        Ok(())
    }

    /// Get latest release from GitHub repository
    pub async fn get_latest_release(&self, owner: &str, repo: &str) -> Result<serde_json::Value> {
        let url = format!("{}/repos/{}/{}/releases/latest", self.base_url, owner, repo);

        let response = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .context("Failed to fetch latest release")?;

        // Check for rate limit errors
        self.check_rate_limit(&response)?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "GitHub API returned status {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ));
        }

        let release: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse release JSON")?;

        Ok(release)
    }

    /// Download a file from a URL
    pub async fn download_file(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .client
            .get(url)
            .headers(self.headers())
            .send()
            .await
            .context("Failed to download file")?;

        // Check for rate limit errors
        self.check_rate_limit(&response)?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Download failed with status {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ));
        }

        let bytes = response
            .bytes()
            .await
            .context("Failed to read response bytes")?;

        Ok(bytes.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_resolution() {
        // Test that token resolution works (will use env vars if set)
        let _ = GitHubClient::resolve_token(None);
        // Just verify it doesn't panic
    }
}
