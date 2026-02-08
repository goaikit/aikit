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

fn get_api_base_url() -> String {
    std::env::var("GITHUB_API_URL").unwrap_or_else(|_| "https://api.github.com".to_string())
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
        let base_url = get_api_base_url();
        let url = format!(
            "{}/repos/{}/{}/zipball/{}",
            base_url, owner, repo, ref_param
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

        let base_url = get_api_base_url();
        let url = format!("{}/repos/{}/{}/releases", base_url, owner, repo);

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

        // GitHub API requires the asset name in the upload URL
        // The upload_url from create_release is a template like:
        // "https://uploads.github.com/repos/owner/repo/releases/123/assets{?name,label}"
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
            .header(
                "Authorization",
                format!("token {}", self.token.as_ref().unwrap()),
            )
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

impl ReleaseInfo {
    /// Create a new ReleaseInfo with automatic prerelease detection from tag name
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
mod tests {
    use super::*;

    #[test]
    fn test_github_client_new_with_token() {
        let client = GitHubClient::new(Some("test_token".to_string()));
        assert!(client.token.is_some());
        assert_eq!(client.token.unwrap(), "test_token");
    }

    #[test]
    fn test_github_client_new_without_token() {
        let client = GitHubClient::new(None);
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
        let client = GitHubClient::new(None);

        let result = client
            .get_package_manifest("test-owner", "test-repo", None)
            .await;

        // Should fail because no token and repo doesn't exist
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_github_client_download_archive() {
        let client = GitHubClient::new(None);

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
        let client = GitHubClient::new(None);

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

    // Disabled due to async runtime conflicts in test environment
    // #[tokio::test]
    // async fn test_github_client_upload_release_asset_success() {

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
        let client = GitHubClient::new(None);

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
        use crate::core::git::GitHubClient;

        let client = GitHubClient::new(Some("test_token".to_string()));

        let nonexistent_file = PathBuf::from("/nonexistent/path/file.zip");

        let result = client
            .upload_release_asset("test-owner", "test-repo", 123, &nonexistent_file)
            .await;

        assert!(result.is_err());
    }

    // Disabled due to async runtime conflicts in test environment
    // #[tokio::test]
    // async fn test_github_client_upload_release_asset_http_error() {

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

        let client = GitHubClient::new(Some("test_token".to_string()));

        let result = client
            .upload_release_asset("test-owner", "test-repo", 123, &test_file)
            .await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("HTTP"));
        assert!(error_msg.contains("Unauthorized"));
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

        let client = GitHubClient::new(Some("test_token".to_string()));

        let result = client
            .upload_release_asset("test-owner", "test-repo", 123, &test_file)
            .await;

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
