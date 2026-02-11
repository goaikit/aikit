use crate::install::InstallError;
use crate::manifest::TemplateManifest;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Source for fetching a template package
#[derive(Debug, Clone)]
pub enum TemplateSource {
    /// GitHub repository
    GitHub {
        owner: String,
        repo: String,
        version: Option<String>,
    },
    /// Direct URL to a zip archive
    Url(String),
    /// Local directory path
    Path(PathBuf),
}

impl TemplateSource {
    /// Parse a source string into a TemplateSource
    ///
    /// Supports:
    /// - "owner/repo" or "github.com/owner/repo" or "https://github.com/owner/repo"
    /// - Local directory path (must contain aikit.toml)
    /// - Direct URL to zip archive (https://...)
    pub fn parse(s: &str) -> Result<Self, InstallError> {
        let path = Path::new(s);

        // Check if it's an existing local directory
        if path.exists() && path.is_dir() {
            let aikit_toml = path.join("aikit.toml");
            if !aikit_toml.exists() {
                return Err(InstallError::InvalidSource(format!(
                    "Directory '{}' does not contain aikit.toml",
                    s
                )));
            }
            return Ok(TemplateSource::Path(path.to_path_buf()));
        }

        // Exclude relative and absolute paths that don't exist
        if s.starts_with("./") || s.starts_with("../") || path.is_absolute() {
            return Err(InstallError::InvalidSource(format!(
                "Path '{}' does not exist",
                s
            )));
        }

        let source_lower = s.to_lowercase();

        // Check for GitHub URL or owner/repo format
        if source_lower.contains("github.com") {
            return parse_github_url(s);
        }

        // Check for owner/repo format (2 segments, optionally with @version)
        // Split by '/' to check format, but ignore the version part for segment count
        let base_part = s.split('@').next().unwrap_or(s);
        if base_part.split('/').count() == 2 && !path.exists() {
            return parse_github_url(s);
        }

        // Check for HTTPS URL (treat as direct zip URL)
        if s.starts_with("https://") || s.starts_with("http://") {
            return Ok(TemplateSource::Url(s.to_string()));
        }

        Err(InstallError::InvalidSource(format!(
            "Invalid source '{}'. Expected:\n  - Local directory path (must exist and contain aikit.toml)\n  - GitHub URL: github.com/owner/repo or https://github.com/owner/repo\n  - Short format: owner/repo\n  - Direct zip URL: https://...",
            s
        )))
    }
}

/// Parse GitHub URL to extract owner, repo, and optional version
fn parse_github_url(s: &str) -> Result<TemplateSource, InstallError> {
    let url = s
        .trim_start_matches("https://")
        .trim_start_matches("http://");

    let path = if url.starts_with("github.com/") {
        url.strip_prefix("github.com/").unwrap()
    } else {
        url
    };

    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 2 {
        return Err(InstallError::InvalidSource(
            "Invalid GitHub URL format. Expected: github.com/owner/repo or owner/repo".to_string(),
        ));
    }

    let owner = parts[0].to_string();
    let repo = parts[1].to_string();

    // Check if repo contains version (e.g., "repo@v1.0.0")
    let (repo_clean, version) = if let Some(idx) = repo.find('@') {
        let (r, v) = repo.split_at(idx);
        (r.to_string(), Some(v[1..].to_string()))
    } else {
        (repo.clone(), None)
    };

    Ok(TemplateSource::GitHub {
        owner,
        repo: repo_clean,
        version,
    })
}

/// Fetch a package from source to destination directory
///
/// Returns (manifest, package_root_path)
/// - manifest: Parsed TemplateManifest from aikit.toml
/// - package_root_path: Path to the directory containing aikit.toml
pub fn fetch_package_to_dir(
    source: &TemplateSource,
    dest_dir: &Path,
) -> Result<(TemplateManifest, PathBuf), InstallError> {
    match source {
        TemplateSource::Path(path) => fetch_from_path(path, dest_dir),
        TemplateSource::GitHub {
            owner,
            repo,
            version,
        } => fetch_from_github(owner, repo, version.as_deref(), dest_dir),
        TemplateSource::Url(url) => fetch_from_url(url, dest_dir),
    }
}

/// Fetch from local directory
fn fetch_from_path(
    source_path: &Path,
    _dest_dir: &Path,
) -> Result<(TemplateManifest, PathBuf), InstallError> {
    // Validate source path exists and is a directory
    if !source_path.exists() {
        return Err(InstallError::InvalidSource(format!(
            "Source path does not exist: {}",
            source_path.display()
        )));
    }

    if !source_path.is_dir() {
        return Err(InstallError::InvalidSource(format!(
            "Source path is not a directory: {}",
            source_path.display()
        )));
    }

    // Check for aikit.toml
    let aikit_toml = source_path.join("aikit.toml");
    if !aikit_toml.exists() {
        return Err(InstallError::InvalidSource(format!(
            "aikit.toml not found in: {}",
            source_path.display()
        )));
    }

    // Read and parse manifest
    let manifest_content = fs::read_to_string(&aikit_toml).map_err(|e| {
        InstallError::FetchFailed(format!(
            "Failed to read aikit.toml from '{}': {}",
            source_path.display(),
            e
        ))
    })?;

    let manifest = TemplateManifest::from_toml_str(&manifest_content)?;

    // Return source path as package root
    let package_root = source_path.canonicalize().map_err(InstallError::Io)?;

    Ok((manifest, package_root))
}

/// Fetch from GitHub repository
fn fetch_from_github(
    owner: &str,
    repo: &str,
    version: Option<&str>,
    dest_dir: &Path,
) -> Result<(TemplateManifest, PathBuf), InstallError> {
    let ref_ = version.unwrap_or("main");

    // Step 1: Fetch manifest from GitHub
    let manifest_url = format!(
        "https://raw.githubusercontent.com/{}/{}/{}/aikit.toml",
        owner, repo, ref_
    );

    let client = reqwest::blocking::Client::new();
    let mut request = client.get(&manifest_url);

    // Add GitHub token if available
    if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
        request = request.header("Authorization", format!("token {}", token));
    }

    let response = request.send().map_err(|e| {
        InstallError::FetchFailed(format!(
            "Failed to fetch manifest from '{}': {}",
            manifest_url, e
        ))
    })?;

    if !response.status().is_success() {
        return Err(InstallError::FetchFailed(format!(
            "Failed to fetch manifest from '{}': HTTP {}",
            manifest_url,
            response.status()
        )));
    }

    let manifest_content = response.text().map_err(|e| {
        InstallError::FetchFailed(format!("Failed to read manifest response: {}", e))
    })?;

    let manifest = TemplateManifest::from_toml_str(&manifest_content)?;

    // Step 2: Download zipball
    let zipball_url = format!(
        "https://api.github.com/repos/{}/{}/zipball/{}",
        owner, repo, ref_
    );

    let mut zip_request = client.get(&zipball_url);
    if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
        zip_request = zip_request.header("Authorization", format!("token {}", token));
    }
    zip_request = zip_request.header("User-Agent", "aikit-sdk/1.0");

    let mut zip_response = zip_request.send().map_err(|e| {
        InstallError::FetchFailed(format!(
            "Failed to download zipball from '{}': {}",
            zipball_url, e
        ))
    })?;

    if !zip_response.status().is_success() {
        return Err(InstallError::FetchFailed(format!(
            "Failed to download zipball from '{}': HTTP {}",
            zipball_url,
            zip_response.status()
        )));
    }

    let mut zip_bytes = Vec::new();
    zip_response
        .read_to_end(&mut zip_bytes)
        .map_err(|e| InstallError::FetchFailed(format!("Failed to read zipball: {}", e)))?;

    // Step 3: Extract zip to dest_dir
    extract_zip(&zip_bytes, dest_dir)?;

    // Step 4: Find package root (directory containing aikit.toml)
    let package_root = find_package_root(dest_dir).ok_or_else(|| {
        InstallError::InvalidSource(
            "Could not find package root (directory containing aikit.toml) after extraction"
                .to_string(),
        )
    })?;

    Ok((manifest, package_root))
}

/// Fetch from direct URL to zip archive
fn fetch_from_url(url: &str, dest_dir: &Path) -> Result<(TemplateManifest, PathBuf), InstallError> {
    let client = reqwest::blocking::Client::new();
    let request = client.get(url);

    let mut response = request
        .send()
        .map_err(|e| InstallError::FetchFailed(format!("Failed to fetch URL '{}': {}", url, e)))?;

    if !response.status().is_success() {
        return Err(InstallError::FetchFailed(format!(
            "Failed to fetch URL '{}': HTTP {}",
            url,
            response.status()
        )));
    }

    let mut bytes = Vec::new();
    response
        .read_to_end(&mut bytes)
        .map_err(|e| InstallError::FetchFailed(format!("Failed to read response: {}", e)))?;

    // Extract zip to dest_dir
    extract_zip(&bytes, dest_dir)?;

    // Find package root and read manifest
    let package_root = find_package_root(dest_dir).ok_or_else(|| {
        InstallError::InvalidSource(
            "Could not find package root (directory containing aikit.toml) after extraction"
                .to_string(),
        )
    })?;

    let aikit_toml = package_root.join("aikit.toml");
    let manifest_content = fs::read_to_string(&aikit_toml).map_err(|e| {
        InstallError::FetchFailed(format!(
            "Failed to read aikit.toml from '{}': {}",
            package_root.display(),
            e
        ))
    })?;

    let manifest = TemplateManifest::from_toml_str(&manifest_content)?;

    Ok((manifest, package_root))
}

/// Extract zip bytes to destination directory
fn extract_zip(zip_bytes: &[u8], dest_dir: &Path) -> Result<(), InstallError> {
    use std::io::Cursor;

    // Create dest_dir if it doesn't exist
    fs::create_dir_all(dest_dir)?;

    let cursor = Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| InstallError::FetchFailed(format!("Invalid zip archive: {}", e)))?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| {
            InstallError::FetchFailed(format!("Failed to access zip entry {}: {}", i, e))
        })?;
        let outpath = dest_dir.join(file.mangled_name());

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            let mut outfile = fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }

    Ok(())
}

/// Find package root: directory containing aikit.toml
/// Handles GitHub zipball case (single top-level directory)
fn find_package_root(dir: &Path) -> Option<PathBuf> {
    // Check if aikit.toml is directly in dir
    let direct = dir.join("aikit.toml");
    if direct.exists() {
        return Some(dir.to_path_buf());
    }

    // Look for single child directory containing aikit.toml (GitHub zipball case)
    let children: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();

    if children.len() == 1 {
        let child = &children[0];
        if child.join("aikit.toml").exists() {
            return Some(child.clone());
        }
    }

    // Look for any directory containing aikit.toml
    for entry in fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.is_dir() && path.join("aikit.toml").exists() {
            return Some(path);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_template_source_parse_local_directory() {
        let temp_dir = TempDir::new().unwrap();
        let aikit_toml = temp_dir.path().join("aikit.toml");
        fs::write(
            &aikit_toml,
            "[package]\nname = \"test\"\nversion = \"1.0.0\"",
        )
        .unwrap();

        let source = TemplateSource::parse(temp_dir.path().to_str().unwrap()).unwrap();
        match source {
            TemplateSource::Path(p) => {
                assert_eq!(p, temp_dir.path());
            }
            _ => panic!("Expected Path source"),
        }
    }

    #[test]
    fn test_template_source_parse_github_url() {
        let source = TemplateSource::parse("https://github.com/owner/repo").unwrap();
        match source {
            TemplateSource::GitHub {
                owner,
                repo,
                version,
            } => {
                assert_eq!(owner, "owner");
                assert_eq!(repo, "repo");
                assert!(version.is_none());
            }
            _ => panic!("Expected GitHub source"),
        }
    }

    #[test]
    fn test_template_source_parse_github_short() {
        let source = TemplateSource::parse("owner/repo").unwrap();
        match source {
            TemplateSource::GitHub {
                owner,
                repo,
                version,
            } => {
                assert_eq!(owner, "owner");
                assert_eq!(repo, "repo");
                assert!(version.is_none());
            }
            _ => panic!("Expected GitHub source"),
        }
    }

    #[test]
    fn test_template_source_parse_github_with_version() {
        let source = TemplateSource::parse("owner/repo@v1.0.0").unwrap();
        match source {
            TemplateSource::GitHub {
                owner,
                repo,
                version,
            } => {
                assert_eq!(owner, "owner");
                assert_eq!(repo, "repo");
                assert_eq!(version, Some("v1.0.0".to_string()));
            }
            _ => panic!("Expected GitHub source"),
        }
    }

    #[test]
    fn test_template_source_parse_invalid() {
        let result = TemplateSource::parse("not-a-valid-source");
        assert!(result.is_err());
    }

    #[test]
    fn test_fetch_from_path() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let aikit_toml = source_dir.join("aikit.toml");
        fs::write(
            &aikit_toml,
            "[package]\nname = \"test-pkg\"\nversion = \"1.0.0\"\n\n[artifacts]\n\"test/**\" = \".test\"",
        )
        .unwrap();

        // Create test file
        fs::create_dir_all(source_dir.join("test")).unwrap();
        fs::write(source_dir.join("test/file.txt"), "content").unwrap();

        let (manifest, package_root) = fetch_from_path(&source_dir, temp_dir.path()).unwrap();
        assert_eq!(manifest.package.name, "test-pkg");
        assert_eq!(manifest.package.version, "1.0.0");
        assert_eq!(package_root, source_dir.canonicalize().unwrap());
    }

    #[test]
    fn test_fetch_from_path_no_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let result = fetch_from_path(&source_dir, temp_dir.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            InstallError::InvalidSource(_) => {}
            _ => panic!("Expected InvalidSource error"),
        }
    }

    #[test]
    fn test_fetch_from_path_not_directory() {
        let temp_dir = TempDir::new().unwrap();
        let source_file = temp_dir.path().join("file.txt");
        fs::write(&source_file, "content").unwrap();

        let result = fetch_from_path(&source_file, temp_dir.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            InstallError::InvalidSource(_) => {}
            _ => panic!("Expected InvalidSource error"),
        }
    }

    #[test]
    fn test_find_package_root_direct() {
        let temp_dir = TempDir::new().unwrap();
        let aikit_toml = temp_dir.path().join("aikit.toml");
        fs::write(&aikit_toml, "[package]\nname = \"test\"").unwrap();

        let result = find_package_root(temp_dir.path());
        assert!(result.is_some());
        assert_eq!(result.unwrap(), temp_dir.path());
    }

    #[test]
    fn test_find_package_root_single_child() {
        let temp_dir = TempDir::new().unwrap();
        let child_dir = temp_dir.path().join("child");
        fs::create_dir_all(&child_dir).unwrap();
        let aikit_toml = child_dir.join("aikit.toml");
        fs::write(&aikit_toml, "[package]\nname = \"test\"").unwrap();

        let result = find_package_root(temp_dir.path());
        assert!(result.is_some());
        assert_eq!(result.unwrap(), child_dir);
    }

    #[test]
    fn test_find_package_root_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let result = find_package_root(temp_dir.path());
        assert!(result.is_none());
    }
}
