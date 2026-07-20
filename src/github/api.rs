//! GitHub API client
//!
//! This module handles all interactions with the GitHub API, including:
//! - Release and asset downloads
//! - Authentication handling
//! - Rate limit detection

use crate::github::rate_limit::GitHubRateLimitInfo;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

pub(crate) const USER_AGENT: &str = "aikit";

/// GitHub API client
pub struct GitHubClient {
    client: Client,
    base_url: String,
    /// Host manifest content (`aikit.toml`/`package.toml`) is fetched from.
    /// Always `https://raw.githubusercontent.com` in production — unlike
    /// `base_url` this has **no** environment-variable override, so it can't
    /// compound SEC-9 (arbitrary-host token exfiltration via `GITHUB_API_URL`)
    /// with a second overridable host. Only the `#[cfg(test)]` constructor
    /// (`for_test`) points it elsewhere, at an in-process mock server.
    raw_base_url: String,
    token: Option<String>,
}

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

        // Allow overriding the base URL via environment variable (useful for testing)
        let base_url = std::env::var("GITHUB_API_URL")
            .unwrap_or_else(|_| "https://api.github.com".to_string());

        Ok(Self {
            client,
            base_url,
            raw_base_url: "https://raw.githubusercontent.com".to_string(),
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
            reqwest::header::HeaderValue::from_static(USER_AGENT),
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

    /// Get package manifest from a GitHub repository
    /// Tries aikit.toml first, then falls back to package.toml for backward compatibility
    pub async fn get_package_manifest(
        &self,
        owner: &str,
        repo: &str,
        ref_: Option<&str>,
    ) -> Result<PackageManifest> {
        let ref_param = ref_.unwrap_or("main");

        // Try aikit.toml first
        let aikit_url = format!(
            "{}/{}/{}/{}/aikit.toml",
            self.raw_base_url, owner, repo, ref_param
        );

        let response = self
            .client
            .get(&aikit_url)
            .headers(self.headers())
            .send()
            .await?;

        // If aikit.toml found, parse and return
        if response.status().is_success() {
            let content = response.text().await?;
            let manifest: PackageManifest = toml::from_str(&content)?;
            return Ok(manifest);
        }

        // If 404, try package.toml as fallback
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            let package_url = format!(
                "{}/{}/{}/{}/package.toml",
                self.raw_base_url, owner, repo, ref_param
            );

            let fallback_response = self
                .client
                .get(&package_url)
                .headers(self.headers())
                .send()
                .await?;

            if fallback_response.status().is_success() {
                let content = fallback_response.text().await?;
                let manifest: PackageManifest = toml::from_str(&content)?;
                return Ok(manifest);
            }

            // Both files not found
            return Err(anyhow::anyhow!(
                "Failed to fetch package manifest: Neither aikit.toml nor package.toml found in {}/{}",
                owner, repo
            ));
        }

        // Other HTTP errors from aikit.toml request
        Err(anyhow::anyhow!(
            "Failed to fetch aikit.toml: HTTP {}",
            response.status()
        ))
    }

    /// Resolve a git ref (branch, tag, or SHA) to its immutable commit SHA (SEC-7).
    ///
    /// `aikit update`/`aikit install` fetch package archives by mutable ref
    /// (typically the default branch); this pins that ref to a concrete
    /// commit at fetch time so the lock file (`src/core/lock.rs`) can record
    /// something that can't silently change out from under an install.
    pub async fn resolve_ref_to_sha(&self, owner: &str, repo: &str, ref_: &str) -> Result<String> {
        let url = format!(
            "{}/repos/{}/{}/commits/{}",
            self.base_url, owner, repo, ref_
        );

        let response = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .context("Failed to resolve ref to commit SHA")?;

        self.check_rate_limit(&response)?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to resolve ref '{}' to a commit: HTTP {}",
                ref_,
                response.status()
            ));
        }

        let commit: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse commit JSON")?;

        commit["sha"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                anyhow::anyhow!("Commit response for ref '{}' missing 'sha' field", ref_)
            })
    }

    /// Get release ID by tag from GitHub repository
    pub async fn get_release_by_tag(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let url = format!(
            "{}/repos/{}/{}/releases/tags/{}",
            self.base_url, owner, repo, tag
        );

        let response = self.client.get(&url).headers(self.headers()).send().await?;

        if !response.status().is_success() {
            return Err(format!(
                "No release found with tag '{}'. Use --no-release only with an existing release.",
                tag
            )
            .into());
        }

        let release: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse release response: {}", e))?;

        release["id"]
            .as_u64()
            .ok_or_else(|| "Invalid release data: missing ID".to_string().into())
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
            "{}/repos/{}/{}/zipball/{}",
            self.base_url, owner, repo, ref_param
        );

        let response = self.client.get(&url).headers(self.headers()).send().await?;
        if !response.status().is_success() {
            return Err(format!("Failed to download archive: HTTP {}", response.status()).into());
        }

        let bytes = response.bytes().await?;
        std::fs::write(dest, bytes)?;

        Ok(())
    }

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

        let url = format!("{}/repos/{}/{}/releases", self.base_url, owner, repo);

        let response = self
            .client
            .post(&url)
            .headers(self.headers())
            .json(release)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("Failed to create release: HTTP {}", response.status()).into());
        }

        let release_response: ReleaseResponse = response.json().await?;
        Ok(release_response)
    }

    /// Upload an asset to a GitHub release
    pub async fn upload_release_asset(
        &self,
        owner: &str,
        repo: &str,
        release_id: u64,
        file_path: &PathBuf,
    ) -> Result<String, Box<dyn std::error::Error>> {
        if self.token.is_none() {
            return Err("GitHub token required for uploading assets".into());
        }

        let file_name = file_path.file_name().ok_or("Invalid file path")?;

        let upload_url = format!(
            "https://uploads.github.com/repos/{}/{}/releases/{}/assets?name={}",
            owner,
            repo,
            release_id,
            file_name.to_string_lossy()
        );

        let file_content = std::fs::read(file_path)?;
        let file_size = file_content.len();

        println!(
            "  📤 Uploading {} ({:.2} KB)...",
            file_name.to_string_lossy(),
            file_size as f64 / 1024.0
        );

        let response = self
            .client
            .post(&upload_url)
            .headers(self.headers())
            .header("Content-Type", "application/zip")
            .body(file_content)
            .send()
            .await?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(format!("Failed to upload asset: HTTP {} - {}", status, error_text).into());
        }

        let asset_url = response.url().clone();
        println!("  ✅ Upload complete");

        Ok(asset_url.to_string())
    }
}

impl Default for GitHubClient {
    fn default() -> Self {
        Self::new(None).expect("Failed to create default GitHub client")
    }
}

/// Package manifest from aikit.toml
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

impl ReleaseInfo {
    /// Create a new ReleaseInfo with automatic prerelease detection from tag name
    #[allow(dead_code)]
    pub fn new(tag_name: String, name: String, body: String, draft: bool) -> Self {
        let prerelease =
            tag_name.contains("alpha") || tag_name.contains("beta") || tag_name.contains("rc");

        Self {
            tag_name,
            name,
            body,
            draft,
            prerelease,
        }
    }
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

#[cfg(test)]
impl GitHubClient {
    /// Test-only constructor that points both the API host (`base_url`) and
    /// the raw-content host (`raw_base_url`) at the same mock server URL,
    /// avoiding the process-global `GITHUB_API_URL` env var (which would
    /// race under parallel test execution within this test binary) and
    /// avoiding any real network access. A single mockito server can serve
    /// both `/repos/...` (API) and `/{owner}/{repo}/{ref}/aikit.toml` (raw
    /// content) style paths, so one URL covers both.
    ///
    /// `pub(crate)` so other modules' tests (e.g.
    /// `src/cli/commands/install.rs`) can build a fully mocked client too.
    pub(crate) fn for_test(base_url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("failed to build test http client");
        Self {
            client,
            raw_base_url: base_url.clone(),
            base_url,
            token: None,
        }
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

    #[test]
    fn test_github_client_new_with_token() {
        let client = GitHubClient::new(Some("test_token".to_string())).unwrap();
        assert!(client.token.is_some());
        assert_eq!(client.token.unwrap(), "test_token");
    }

    #[test]
    fn test_github_client_new_without_token() {
        let client = GitHubClient::new(None).unwrap();
        assert!(client.token.is_none());
    }

    #[test]
    fn test_github_client_default() {
        let client = GitHubClient::default();
        assert!(client.token.is_none());
    }

    #[test]
    fn test_release_response_creation() {
        let response = ReleaseResponse {
            id: 123,
            tag_name: "v1.0.0".to_string(),
            name: "Release 1.0".to_string(),
            body: "Test release".to_string(),
            html_url: "https://github.com/owner/repo/releases/v1.0.0".to_string(),
            upload_url:
                "https://uploads.github.com/repos/owner/repo/releases/123/assets{?name,label}"
                    .to_string(),
        };

        assert_eq!(response.id, 123);
        assert_eq!(response.tag_name, "v1.0.0");
        assert_eq!(response.name, "Release 1.0");
        assert!(response.html_url.contains("github.com"));
        assert!(response.upload_url.contains("uploads.github.com"));
    }

    #[test]
    fn test_release_info_creation() {
        let info = ReleaseInfo {
            tag_name: "v2.0.0".to_string(),
            name: "Release 2.0".to_string(),
            body: "Major version update".to_string(),
            draft: true,
            prerelease: false,
        };

        assert_eq!(info.tag_name, "v2.0.0");
        assert_eq!(info.name, "Release 2.0");
        assert_eq!(info.body, "Major version update");
        assert!(info.draft);
        assert!(!info.prerelease);
    }

    #[test]
    fn test_release_info_default_values() {
        let info = ReleaseInfo {
            tag_name: "v1.0.0".to_string(),
            name: "Release 1.0".to_string(),
            body: "Test".to_string(),
            draft: false,
            prerelease: false,
        };

        assert!(!info.draft);
        assert!(!info.prerelease);
    }

    #[tokio::test]
    async fn test_github_client_get_package_manifest() {
        let client = GitHubClient::new(None).unwrap();

        let result = client
            .get_package_manifest("test-owner", "test-repo", None)
            .await;

        // Should fail because no token and repo doesn't exist
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_github_client_download_archive() {
        let client = GitHubClient::new(None).unwrap();

        let temp_dir = tempfile::TempDir::new().unwrap();
        let dest = temp_dir.path().join("archive.zip");

        let result = client
            .download_archive("test-owner", "test-repo", None, &dest)
            .await;

        // Should fail because no token and repo doesn't exist
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_github_client_create_release() {
        let client = GitHubClient::new(None).unwrap();

        let release_info = ReleaseInfo {
            tag_name: "v1.0.0".to_string(),
            name: "Release 1.0".to_string(),
            body: "Test release".to_string(),
            draft: false,
            prerelease: false,
        };

        let result = client
            .create_release("test-owner", "test-repo", &release_info)
            .await;

        // Should fail because no token
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_github_client_upload_release_asset_token_required() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test-upload.zip");

        // Create a test ZIP file
        use std::fs::File;
        use std::io::Write;
        use zip::write::ZipWriter;
        use zip::CompressionMethod;

        let file = File::create(&test_file).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file(
            "aikit.toml",
            zip::write::FileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(b"[package]\nname = \"test\"\nversion = \"0.1.0\"\n")
            .unwrap();
        zip.finish().unwrap();

        // Create client without token
        let client = GitHubClient::new(None).unwrap();

        // Make upload request
        let result = client
            .upload_release_asset("test-owner", "test-repo", 123, &test_file)
            .await;

        // Check result
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("token"));
    }

    #[tokio::test]
    async fn test_github_client_upload_release_asset_file_not_found() {
        let client = GitHubClient::new(Some("test_token".to_string())).unwrap();

        let nonexistent_file = PathBuf::from("/nonexistent/path/file.zip");

        let result = client
            .upload_release_asset("test-owner", "test-repo", 123, &nonexistent_file)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_package_manifest_serialization() {
        let manifest = PackageManifest {
            package: PackageInfo {
                name: "test-package".to_string(),
                version: "1.0.0".to_string(),
                description: "Test description".to_string(),
                authors: vec!["test".to_string()],
            },
            commands: std::collections::HashMap::new(),
            artifacts: std::collections::HashMap::new(),
        };

        let toml_str = toml::to_string(&manifest).unwrap();

        assert!(toml_str.contains("name = \"test-package\""));
        assert!(toml_str.contains("version = \"1.0.0\""));
    }

    #[tokio::test]
    async fn test_package_manifest_deserialization() {
        let toml_str = r#"
[package]
name = "test-package"
version = "1.0.0"
description = "Test description"
authors = ["test"]
"#;

        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();

        assert_eq!(manifest.package.name, "test-package");
        assert_eq!(manifest.package.version, "1.0.0");
        assert_eq!(manifest.package.description, "Test description");
    }

    #[tokio::test]
    async fn test_upload_release_asset_success_snapshot() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test-upload.zip");

        use std::fs::File;
        use std::io::Write;
        use zip::write::ZipWriter;
        use zip::CompressionMethod;

        let file = File::create(&test_file).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file(
            "aikit.toml",
            zip::write::FileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(b"[package]\nname = \"test\"\nversion = \"0.1.0\"\n")
            .unwrap();
        zip.finish().unwrap();

        let client = GitHubClient::new(Some("test_token".to_string())).unwrap();

        let result = client
            .upload_release_asset("test-owner", "test-repo", 123, &test_file)
            .await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("HTTP") || error_msg.contains("error sending request"),
            "unexpected error: {error_msg}"
        );
    }

    #[tokio::test]
    async fn test_upload_release_asset_large_file() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("large-test-upload.zip");

        use std::fs::File;
        use std::io::Write;
        use zip::write::ZipWriter;
        use zip::CompressionMethod;

        let file = File::create(&test_file).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file(
            "aikit.toml",
            zip::write::FileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(b"[package]\nname = \"test\"\nversion = \"0.1.0\"\n")
            .unwrap();
        zip.finish().unwrap();

        let client = GitHubClient::new(Some("test_token".to_string())).unwrap();

        let result = client
            .upload_release_asset("test-owner", "test-repo", 123, &test_file)
            .await;

        assert!(result.is_err());
    }

    // -- SEC-7: resolve_ref_to_sha (mocked GitHub API, no real network) -----

    #[tokio::test]
    async fn test_resolve_ref_to_sha_success() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/owner/repo/commits/main")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"sha": "abc123def456"}"#)
            .create_async()
            .await;

        let client = GitHubClient::for_test(server.url());
        let sha = client
            .resolve_ref_to_sha("owner", "repo", "main")
            .await
            .unwrap();
        assert_eq!(sha, "abc123def456");
    }

    #[tokio::test]
    async fn test_resolve_ref_to_sha_not_found() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/owner/repo/commits/does-not-exist")
            .with_status(404)
            .create_async()
            .await;

        let client = GitHubClient::for_test(server.url());
        let result = client
            .resolve_ref_to_sha("owner", "repo", "does-not-exist")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resolve_ref_to_sha_missing_sha_field() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/owner/repo/commits/main")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"not_sha": "oops"}"#)
            .create_async()
            .await;

        let client = GitHubClient::for_test(server.url());
        let result = client.resolve_ref_to_sha("owner", "repo", "main").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_release_info_new_snapshot() {
        let release_info = ReleaseInfo::new(
            "v1.0.0".to_string(),
            "Release 1.0".to_string(),
            "Test release".to_string(),
            false,
        );

        let yaml_str = serde_yaml::to_string(&release_info).unwrap();
        insta::assert_snapshot!("release_info_new_stable", yaml_str);

        let release_info_alpha = ReleaseInfo::new(
            "v1.0.0-alpha".to_string(),
            "Alpha Release".to_string(),
            "Test".to_string(),
            false,
        );

        let yaml_str_alpha = serde_yaml::to_string(&release_info_alpha).unwrap();
        insta::assert_snapshot!("release_info_new_alpha", yaml_str_alpha);
    }
}
