//! GitHub integration for package distribution
//!
//! This module handles GitHub API interactions for package discovery,
//! downloading, and publishing.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// GitHub API client for package operations
pub struct GitHubClient {
    client: Client,
    token: Option<String>,
}

impl GitHubClient {
    /// Create a new GitHub client
    pub fn new(token: Option<String>) -> Self {
        Self {
            client: Client::new(),
            token,
        }
    }

    /// Get package manifest from a GitHub repository
    /// Tries aikit.toml first, then falls back to package.toml for backward compatibility
    pub async fn get_package_manifest(
        &self,
        owner: &str,
        repo: &str,
        ref_: Option<&str>,
    ) -> Result<PackageManifest, Box<dyn std::error::Error>> {
        let ref_param = ref_.unwrap_or("main");

        // Try aikit.toml first
        let aikit_url = format!(
            "https://raw.githubusercontent.com/{}/{}/{}/aikit.toml",
            owner, repo, ref_param
        );

        let mut request = self.client.get(&aikit_url);
        if let Some(token) = &self.token {
            request = request.header("Authorization", format!("token {}", token));
        }

        let response = request.send().await?;

        // If aikit.toml found, parse and return
        if response.status().is_success() {
            let content = response.text().await?;
            let manifest: PackageManifest = toml::from_str(&content)?;
            return Ok(manifest);
        }

        // If 404, try package.toml as fallback
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            let package_url = format!(
                "https://raw.githubusercontent.com/{}/{}/{}/package.toml",
                owner, repo, ref_param
            );

            let mut fallback_request = self.client.get(&package_url);
            if let Some(token) = &self.token {
                fallback_request =
                    fallback_request.header("Authorization", format!("token {}", token));
            }

            let fallback_response = fallback_request.send().await?;

            if fallback_response.status().is_success() {
                let content = fallback_response.text().await?;
                let manifest: PackageManifest = toml::from_str(&content)?;
                return Ok(manifest);
            }

            // Both files not found
            return Err(format!(
                "Failed to fetch package manifest: Neither aikit.toml nor package.toml found in {}/{}",
                owner, repo
            ).into());
        }

        // Other HTTP errors from aikit.toml request
        Err(format!("Failed to fetch aikit.toml: HTTP {}", response.status()).into())
    }

    /// Download repository archive (ZIP)
    pub async fn download_archive(
        &self,
        owner: &str,
        repo: &str,
        ref_: Option<&str>,
        dest: &PathBuf,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ref_param = ref_.unwrap_or("main");
        let url = format!(
            "https://api.github.com/repos/{}/{}/zipball/{}",
            owner, repo, ref_param
        );

        let mut request = self.client.get(&url);

        if let Some(token) = &self.token {
            request = request.header("Authorization", format!("token {}", token));
            request = request.header("User-Agent", "AIKIT-Package-Manager/1.0");
        }

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(format!("Failed to download archive: HTTP {}", response.status()).into());
        }

        let bytes = response.bytes().await?;
        std::fs::write(dest, bytes)?;

        Ok(())
    }

    /// Search repositories for packages

    /// Create a GitHub release
    pub async fn create_release(
        &self,
        owner: &str,
        repo: &str,
        release: &ReleaseInfo,
    ) -> Result<ReleaseResponse, Box<dyn std::error::Error>> {
        if self.token.is_none() {
            return Err("GitHub token required for creating releases".into());
        }

        let url = format!("https://api.github.com/repos/{}/{}/releases", owner, repo);

        let response = self
            .client
            .post(&url)
            .header(
                "Authorization",
                format!("token {}", self.token.as_ref().unwrap()),
            )
            .header("User-Agent", "AIKIT-Package-Manager/1.0")
            .json(release)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("Failed to create release: HTTP {}", response.status()).into());
        }

        let release_response: ReleaseResponse = response.json().await?;
        Ok(release_response)
    }
}

impl Default for GitHubClient {
    fn default() -> Self {
        Self::new(None)
    }
}

/// Package manifest from package.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub package: PackageInfo,
    #[serde(default)]
    pub commands: std::collections::HashMap<String, CommandInfo>,
    #[serde(default)]
    pub artifacts: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInfo {
    pub description: String,
    pub template: Option<String>,
}


/// Release creation information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub name: String,
    pub body: String,
    pub draft: bool,
    pub prerelease: bool,
}

/// Release creation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseResponse {
    pub id: u64,
    pub tag_name: String,
    pub name: String,
    pub body: String,
    pub html_url: String,
    pub upload_url: String,
}
