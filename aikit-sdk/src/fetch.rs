use crate::install::InstallError;
use crate::manifest::TemplateManifest;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum TemplateSource {
    GitHub {
        owner: String,
        repo: String,
        version: Option<String>,
    },
    Url(String),
    Path(PathBuf),
}

impl TemplateSource {
    pub fn parse(s: &str) -> Result<Self, InstallError> {
        let path = Path::new(s);

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

        if s.starts_with("./") || s.starts_with("../") {
            return Err(InstallError::InvalidSource(format!(
                "Invalid source '{}': directory does not exist",
                s
            )));
        }

        if path.is_absolute() {
            return Err(InstallError::InvalidSource(format!(
                "Invalid source '{}': directory does not exist",
                s
            )));
        }

        if s.starts_with("http://") || s.starts_with("https://") {
            let source_lower = s.to_lowercase();
            if source_lower.contains("github.com") {
                return parse_github_url(s, None);
            }
            return Ok(TemplateSource::Url(s.to_string()));
        }

        let source_lower = s.to_lowercase();
        if source_lower.contains("github.com") {
            return parse_github_url(s, None);
        }

        if source_lower.contains('/') && !source_lower.contains("github.com") && !path.exists() {
            return parse_github_url(s, None);
        }

        Err(InstallError::InvalidSource(format!(
            "Invalid source '{}'. Expected:\n  - Local directory path (must exist and contain aikit.toml)\n  - GitHub URL: github.com/owner/repo or https://github.com/owner/repo\n  - Short format: owner/repo",
            s
        )))
    }
}

fn parse_github_url(source: &str, version: Option<&str>) -> Result<TemplateSource, InstallError> {
    let url = source
        .trim_start_matches("https://")
        .trim_start_matches("http://");

    let path = if url.starts_with("github.com/") {
        url.strip_prefix("github.com/").unwrap()
    } else if url.contains('/') && !url.contains("github.com") {
        url
    } else {
        return Err(InstallError::InvalidSource(
            "Expected: github.com/owner/repo or owner/repo".to_string(),
        ));
    };

    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 2 {
        return Err(InstallError::InvalidSource(
            "Invalid GitHub URL format".to_string(),
        ));
    }

    let owner = parts[0].to_string();
    let repo = parts[1].to_string();

    Ok(TemplateSource::GitHub {
        owner,
        repo,
        version: version.map(|v| v.to_string()),
    })
}

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

fn fetch_from_path(
    source_path: &Path,
    _dest_dir: &Path,
) -> Result<(TemplateManifest, PathBuf), InstallError> {
    if !source_path.exists() || !source_path.is_dir() {
        return Err(InstallError::InvalidSource(format!(
            "Source path '{}' does not exist or is not a directory",
            source_path.display()
        )));
    }

    let aikit_toml_path = source_path.join("aikit.toml");
    if !aikit_toml_path.exists() {
        return Err(InstallError::InvalidSource(format!(
            "Source directory '{}' does not contain aikit.toml",
            source_path.display()
        )));
    }

    let toml_content = fs::read_to_string(&aikit_toml_path)?;
    let manifest = TemplateManifest::from_toml_str(&toml_content)?;

    let package_root = source_path.canonicalize()?;

    Ok((manifest, package_root))
}

fn fetch_from_github(
    owner: &str,
    repo: &str,
    version: Option<&str>,
    dest_dir: &Path,
) -> Result<(TemplateManifest, PathBuf), InstallError> {
    let ref_param = version.unwrap_or("main");

    let manifest_url = format!(
        "https://raw.githubusercontent.com/{}/{}/{}/aikit.toml",
        owner, repo, ref_param
    );

    let manifest_content = fetch_http(&manifest_url)?;
    let manifest = TemplateManifest::from_toml_str(&manifest_content)?;

    let zipball_url = format!(
        "https://api.github.com/repos/{}/{}/zipball/{}",
        owner, repo, ref_param
    );

    let temp_zip_path = dest_dir.join(format!("{}-{}-download.zip", owner, repo));
    fetch_http_to_file(&zipball_url, &temp_zip_path)?;

    unzip_file(&temp_zip_path, dest_dir)?;

    let package_root = find_package_root(dest_dir)?;

    fs::remove_file(&temp_zip_path).ok();

    Ok((manifest, package_root))
}

fn fetch_from_url(url: &str, dest_dir: &Path) -> Result<(TemplateManifest, PathBuf), InstallError> {
    let temp_zip_path = dest_dir.join("download.zip");
    fetch_http_to_file(url, &temp_zip_path)?;

    unzip_file(&temp_zip_path, dest_dir)?;

    let package_root = find_package_root(dest_dir)?;

    let toml_path = package_root.join("aikit.toml");
    if !toml_path.exists() {
        fs::remove_file(&temp_zip_path).ok();
        return Err(InstallError::InvalidSource(format!(
            "Downloaded package does not contain aikit.toml at {}",
            package_root.display()
        )));
    }

    let toml_content = fs::read_to_string(&toml_path)?;
    let manifest = TemplateManifest::from_toml_str(&toml_content)?;

    fs::remove_file(&temp_zip_path).ok();

    Ok((manifest, package_root))
}

fn fetch_http(url: &str) -> Result<String, InstallError> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("aikit-sdk/0.1.0")
        .build()
        .map_err(|e| InstallError::FetchFailed(format!("Failed to create HTTP client: {}", e)))?;

    let token = std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .ok();

    let mut request = client.get(url);
    if let Some(token) = token {
        request = request.header("Authorization", format!("token {}", token));
    }

    let response = request
        .send()
        .map_err(|e| InstallError::FetchFailed(format!("HTTP request failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(InstallError::FetchFailed(format!(
            "HTTP request failed with status: {}",
            response.status()
        )));
    }

    response
        .text()
        .map_err(|e| InstallError::FetchFailed(format!("Failed to read response: {}", e)))
}

fn fetch_http_to_file(url: &str, dest_path: &Path) -> Result<(), InstallError> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("aikit-sdk/0.1.0")
        .build()
        .map_err(|e| InstallError::FetchFailed(format!("Failed to create HTTP client: {}", e)))?;

    let token = std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .ok();

    let mut request = client.get(url);
    if let Some(token) = token {
        request = request.header("Authorization", format!("token {}", token));
    }

    let response = request
        .send()
        .map_err(|e| InstallError::FetchFailed(format!("HTTP request failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(InstallError::FetchFailed(format!(
            "HTTP request failed with status: {}",
            response.status()
        )));
    }

    let bytes = response
        .bytes()
        .map_err(|e| InstallError::FetchFailed(format!("Failed to read response: {}", e)))?;

    fs::write(dest_path, bytes)?;

    Ok(())
}

fn unzip_file(zip_path: &Path, dest_dir: &Path) -> Result<(), InstallError> {
    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
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

fn find_package_root(dest_dir: &Path) -> Result<PathBuf, InstallError> {
    let manifest = dest_dir.join("aikit.toml");
    if manifest.exists() {
        return Ok(dest_dir.to_path_buf());
    }

    let children: Vec<PathBuf> = fs::read_dir(dest_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();

    if children.len() == 1 {
        let child = &children[0];
        if child.join("aikit.toml").exists() {
            return Ok(child.clone());
        }
    }

    Err(InstallError::InvalidSource(
        "Could not find package root (directory containing aikit.toml)".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_local_path() {
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
            _ => panic!("Expected Path variant"),
        }
    }

    #[test]
    fn test_parse_local_path_missing_manifest() {
        let temp_dir = TempDir::new().unwrap();

        let result = TemplateSource::parse(temp_dir.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InstallError::InvalidSource(_)
        ));
    }

    #[test]
    fn test_parse_github_url() {
        let source = TemplateSource::parse("github.com/owner/repo").unwrap();
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
            _ => panic!("Expected GitHub variant"),
        }
    }

    #[test]
    fn test_parse_github_url_with_version() {
        let source = TemplateSource::parse("github.com/owner/repo").unwrap();
        if let TemplateSource::GitHub { .. } = source {
        } else {
            panic!("Expected GitHub variant");
        }
    }

    #[test]
    fn test_parse_github_full_url() {
        let source = TemplateSource::parse("https://github.com/owner/repo").unwrap();
        match source {
            TemplateSource::GitHub { owner, repo, .. } => {
                assert_eq!(owner, "owner");
                assert_eq!(repo, "repo");
            }
            _ => panic!("Expected GitHub variant"),
        }
    }

    #[test]
    fn test_parse_owner_repo_format() {
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
            _ => panic!("Expected GitHub variant"),
        }
    }

    #[test]
    fn test_parse_url() {
        let source = TemplateSource::parse("https://example.com/package.zip").unwrap();
        match source {
            TemplateSource::Url(url) => {
                assert_eq!(url, "https://example.com/package.zip");
            }
            _ => panic!("Expected Url variant"),
        }
    }

    #[test]
    fn test_parse_invalid_source() {
        let result = TemplateSource::parse("not-a-valid-source");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InstallError::InvalidSource(_)
        ));
    }

    #[test]
    fn test_fetch_from_path() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let aikit_toml = source_dir.join("aikit.toml");
        fs::write(
            &aikit_toml,
            "[package]\nname = \"test\"\nversion = \"1.0.0\"\n\n[artifacts]\n\"newton/**\" = \".newton\"",
        )
        .unwrap();

        let (manifest, package_root) = fetch_from_path(&source_dir, temp_dir.path()).unwrap();

        assert_eq!(manifest.package.name, "test");
        assert_eq!(manifest.package.version, "1.0.0");
        assert_eq!(manifest.artifacts.len(), 1);
        assert_eq!(package_root, source_dir.canonicalize().unwrap());
    }

    #[test]
    fn test_fetch_from_path_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let source_path = temp_dir.path().join("nonexistent");

        let result = fetch_from_path(&source_path, temp_dir.path());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InstallError::InvalidSource(_)
        ));
    }

    #[test]
    fn test_fetch_from_path_no_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let result = fetch_from_path(&source_dir, temp_dir.path());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            InstallError::InvalidSource(_)
        ));
    }
}
