//! Package management commands
//!
//! This module contains CLI commands for package lifecycle management:
//! - package init: Create new package structure
//! - package build: Build distributable package
//! - package publish: Publish package to registry

use clap::{Args, Subcommand};

/// Package management subcommands
#[derive(Debug, Subcommand)]
pub enum PackageCommands {
    /// Initialize a new package with aikit.toml
    Init(PackageInitArgs),
    /// Build package for distribution
    Build(PackageBuildArgs),
    /// Publish package to registry
    Publish(PackagePublishArgs),
}

/// Arguments for package init command
#[derive(Debug, Args)]
pub struct PackageInitArgs {
    /// Package name (required)
    pub name: String,

    /// Package description
    #[arg(short, long)]
    pub description: Option<String>,

    /// Package version (default: 0.1.0)
    #[arg(short, long, default_value = "0.1.0")]
    pub package_version: String,

    /// Author name
    #[arg(short, long)]
    pub author: Option<String>,

    /// Skip interactive prompts
    #[arg(long)]
    pub yes: bool,
}

/// Arguments for package build command
#[derive(Debug, Args)]
pub struct PackageBuildArgs {
    /// Output directory (default: dist/)
    #[arg(short, long, default_value = "dist")]
    pub output: String,

    /// Target agents (comma-separated, default: all)
    #[arg(long)]
    pub agents: Option<String>,

    /// Include source files
    #[arg(long)]
    pub include_sources: bool,
}

/// Arguments for package publish command
#[derive(Debug, Args)]
pub struct PackagePublishArgs {
    /// Repository in format "owner/repo" (required)
    pub repo: String,

    /// Path to package ZIP file (default: dist/{name}-{version}.zip)
    #[arg(short, long)]
    pub package: Option<String>,

    /// Version tag for the release (default: from aikit.toml)
    #[arg(short, long)]
    pub tag: Option<String>,

    /// Release title (default: "Release {version}")
    #[arg(long)]
    pub title: Option<String>,

    /// Release notes (default: auto-generated)
    #[arg(long)]
    pub notes: Option<String>,

    /// GitHub token (can also be set via GITHUB_TOKEN env var)
    #[arg(long)]
    pub token: Option<String>,

    /// Don't create a release, just upload to existing release
    #[arg(long)]
    pub no_release: bool,
}

/// Execute package init command
pub async fn execute_init(args: PackageInitArgs) -> Result<(), Box<dyn std::error::Error>> {
    use crate::models::package::Package;
    use anyhow::Context;
    use std::fs;
    use std::path::Path;

    let package_name = args.name;
    let package_path = Path::new(&package_name);

    // Check if directory already exists
    if package_path.exists() {
        if !args.yes {
            println!(
                "Directory '{}' already exists. Use --yes to overwrite.",
                package_name
            );
            return Ok(());
        }
        fs::remove_dir_all(package_path)?;
    }

    // Create package directory
    fs::create_dir_all(package_path).with_context(|| {
        format!(
            "Failed to create package directory: {}",
            package_path.display()
        )
    })?;

    // Create subdirectories
    fs::create_dir_all(package_path.join("templates")).with_context(|| {
        format!(
            "Failed to create templates directory: {}",
            package_path.join("templates").display()
        )
    })?;
    fs::create_dir_all(package_path.join("scripts")).with_context(|| {
        format!(
            "Failed to create scripts directory: {}",
            package_path.join("scripts").display()
        )
    })?;
    fs::create_dir_all(package_path.join("docs")).with_context(|| {
        format!(
            "Failed to create docs directory: {}",
            package_path.join("docs").display()
        )
    })?;

    // Create aikit.toml
    let package = Package::create_template(
        package_name.clone(),
        args.description,
        args.author,
        Some(args.package_version.clone()),
    );

    // Validate package before writing
    package
        .validate()
        .map_err(|e| format!("Package validation failed: {}", e))?;

    // Write aikit.toml
    package.to_toml_file(&package_path.join("aikit.toml"))?;

    // Create example template
    let help_template = r#"# Help Command

This is a sample command for the {{package_name}} package.

**Description**: {{command_description}}

**Usage**: Run this command to get help information.

## Available Commands

- `{{package_name}}.help` - Show this help message
- Add more commands as needed

## Installation

This package provides AI agent extensions for {{package_name}}.

## Configuration

No special configuration required.
"#;

    fs::write(
        package_path.join("templates").join("help.md"),
        help_template,
    )
    .with_context(|| {
        format!(
            "Failed to write help template: {}",
            package_path.join("templates").join("help.md").display()
        )
    })?;

    // Create README
    let readme_content = format!(
        r#"# {}

{}

## Installation

```bash
aikit install https://github.com/yourusername/{}
```

## Usage

After installation, the following commands will be available in your AI agent:

- `{}.help` - Show help information

## Development

### Building the package

```bash
cd {}
aikit package build
```

### Testing

```bash
aikit package validate
```

## License

Specify your license here.
"#,
        package_name, package.package.description, package_name, package_name, package_name
    );

    fs::write(package_path.join("README.md"), readme_content).with_context(|| {
        format!(
            "Failed to write README file: {}",
            package_path.join("README.md").display()
        )
    })?;

    println!("✅ Package '{}' initialized successfully!", package_name);
    println!("📁 Created directory structure:");
    println!("  {}/", package_name);
    println!("  ├── aikit.toml");
    println!("  ├── README.md");
    println!("  ├── templates/");
    println!("  │   └── help.md");
    println!("  ├── scripts/");
    println!("  └── docs/");
    println!();
    println!("🚀 Next steps:");
    println!("  cd {}", package_name);
    println!("  # Edit aikit.toml and templates as needed");
    println!("  aikit package build  # Build the package");

    Ok(())
}

/// Execute package build command
pub async fn execute_build(args: PackageBuildArgs) -> Result<(), Box<dyn std::error::Error>> {
    use crate::models::package::Package;
    use anyhow::Context;
    use std::fs;

    let current_dir =
        std::env::current_dir().with_context(|| "Failed to get current working directory")?;
    let package_path = current_dir.join("aikit.toml");

    // Check if aikit.toml exists
    if !package_path.exists() {
        return Err("aikit.toml not found. Run 'aikit package init' first.".into());
    }

    // Load and validate package
    let package = Package::from_toml_file(&package_path).map_err(|e| {
        anyhow::anyhow!(
            "Failed to load package configuration from {}: {}",
            package_path.display(),
            e
        )
    })?;
    package
        .validate()
        .map_err(|e| format!("Package validation failed: {}", e))?;

    // Create output directory
    fs::create_dir_all(&args.output)
        .with_context(|| format!("Failed to create output directory: {}", args.output))?;

    // Build package
    let output_file = build_package(&package, &current_dir, &args)?;

    println!("✅ Package '{}' built successfully!", package.package.name);
    println!("📦 Output: {}", output_file.display());
    println!("📏 Size: {} bytes", fs::metadata(&output_file)?.len());

    Ok(())
}

/// Build package ZIP archive
fn build_package(
    package: &crate::models::package::Package,
    source_dir: &std::path::Path,
    args: &PackageBuildArgs,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    use std::fs::File;
    use zip::write::ZipWriter;
    use zip::CompressionMethod;

    let output_name = format!("{}-{}.zip", package.package.name, package.package.version);
    let output_path = std::path::Path::new(&args.output).join(output_name);

    let file = File::create(&output_path)?;
    let mut zip = ZipWriter::new(file);

    // Add aikit.toml
    let package_toml = package.to_toml_string()?;
    zip.start_file(
        "aikit.toml",
        zip::write::FileOptions::default().compression_method(CompressionMethod::Deflated),
    )?;
    std::io::Write::write_all(&mut zip, package_toml.as_bytes())?;

    // Collect and add artifacts
    for pattern in package.artifacts.keys() {
        add_artifacts_to_zip(&mut zip, source_dir, pattern)?;
    }

    // Add README if it exists
    let readme_path = source_dir.join("README.md");
    if readme_path.exists() {
        zip.start_file(
            "README.md",
            zip::write::FileOptions::default().compression_method(CompressionMethod::Deflated),
        )?;
        let content = std::fs::read_to_string(readme_path)?;
        std::io::Write::write_all(&mut zip, content.as_bytes())?;
    }

    zip.finish()?;
    Ok(output_path)
}

/// Add artifacts matching pattern to ZIP
fn add_artifacts_to_zip(
    zip: &mut zip::ZipWriter<std::fs::File>,
    base_dir: &std::path::Path,
    pattern: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use glob::Pattern;
    use std::io::Write;
    use walkdir::WalkDir;

    let glob_pattern = Pattern::new(pattern)?;

    for entry in WalkDir::new(base_dir) {
        let entry = entry?;
        let path = entry.path();

        // Skip directories and aikit.toml (already added)
        if path.is_dir()
            || path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_lowercase())
                == Some("aikit.toml".to_string())
        {
            continue;
        }

        // Check if path matches pattern
        let relative_path = path.strip_prefix(base_dir)?;
        let path_str = relative_path.to_string_lossy();

        if glob_pattern.matches(&path_str) {
            let content = std::fs::read(path)?;
            zip.start_file(
                path_str.as_ref(),
                zip::write::FileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )?;
            zip.write_all(&content)?;
        }
    }

    Ok(())
}

/// Find package ZIP file in dist folder or user-specified path
fn find_package_zip(
    package: &crate::models::package::Package,
    package_arg: Option<&str>,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let zip_name = format!("{}-{}.zip", package.package.name, package.package.version);

    // If custom path specified, use it
    if let Some(path) = package_arg {
        let zip_path = std::path::PathBuf::from(path);
        if zip_path.exists() {
            println!("📦 Using specified package: {}", zip_path.display());
            return Ok(zip_path);
        } else {
            return Err(format!("Specified package file not found: {}", path).into());
        }
    }

    // Default: look in dist folder (which is the default build output)
    let zip_path = std::path::Path::new("dist").join(&zip_name);
    if zip_path.exists() {
        println!("📦 Found package in dist folder: {}", zip_path.display());
        return Ok(zip_path);
    }

    Err(format!("Package ZIP not found: {}. Run 'aikit package build' first, or specify path with --package.", zip_name).into())
}

/// Execute package publish command
pub async fn execute_publish(args: PackagePublishArgs) -> Result<(), Box<dyn std::error::Error>> {
    use crate::core::git::{GitHubClient, ReleaseInfo};
    use crate::models::package::Package;
    use std::env;

    let current_dir = std::env::current_dir()?;

    // Check if aikit.toml exists
    let package_path = current_dir.join("aikit.toml");
    if !package_path.exists() {
        return Err("aikit.toml not found. Run 'aikit package init' first.".into());
    }

    // Load and validate package
    let package = Package::from_toml_file(&package_path)?;
    package
        .validate()
        .map_err(|e| format!("Package validation failed: {}", e))?;

    // Find package ZIP file
    let zip_path = find_package_zip(&package, args.package.as_deref())?;

    println!(
        "🚀 Publishing {} v{} to {}/{}",
        package.package.name,
        package.package.version,
        args.repo,
        args.tag
            .as_ref()
            .unwrap_or(&format!("v{}", package.package.version))
    );

    // Get GitHub token
    let token = args
        .token
        .or_else(|| env::var("GITHUB_TOKEN").ok())
        .ok_or_else(|| {
            "GitHub token required. Set GITHUB_TOKEN environment variable or use --token."
                .to_string()
        })?;

    // Initialize GitHub client
    let github = GitHubClient::new(Some(token.clone()));

    // Parse repo argument
    let repo_parts: Vec<&str> = args.repo.split('/').collect();
    if repo_parts.len() != 2 {
        return Err("Repository must be in format 'owner/repo'".into());
    }
    let owner = repo_parts[0];
    let repo = repo_parts[1];

    // Determine version tag
    let tag = args
        .tag
        .unwrap_or_else(|| format!("v{}", package.package.version));

    // Create release if requested
    let release_id = if !args.no_release {
        let title = args.title.unwrap_or_else(|| format!("Release {}", tag));
        let notes = args
            .notes
            .unwrap_or_else(|| generate_release_notes(&package));

        let release_info = ReleaseInfo {
            tag_name: tag.clone(),
            name: title,
            body: notes,
            draft: false,
            prerelease: package.package.version.contains("alpha")
                || package.package.version.contains("beta")
                || package.package.version.contains("rc"),
        };

        println!("📝 Creating GitHub release...");
        let release = github
            .create_release(owner, repo, &release_info)
            .await
            .map_err(|e| format!("Failed to create release: {}", e))?;

        println!("✅ Release created: {}", release.html_url);
        release.id
    } else {
        println!("📦 Uploading to existing release: {}", tag);
        println!(
            "⚠️  Warning: This will upload to the latest release with tag '{}'",
            tag
        );
        println!("   If no release exists with this tag, the upload will fail.");
        println!("   Use 'aikit package publish --no-release' with an existing release.");

        // Try to find the release by tag using a public API call
        let releases_url = format!(
            "https://api.github.com/repos/{}/{}/releases/tags/{}",
            owner, repo, tag
        );

        // Create a temporary client for this API call
        let temp_client = reqwest::Client::new();
        let response = temp_client
            .get(&releases_url)
            .header("Authorization", format!("token {}", token.clone()))
            .header("User-Agent", "AIKIT-Package-Manager/1.0")
            .send()
            .await
            .map_err(|e| format!("Failed to find release: {}", e))?;

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
            .ok_or_else(|| "Invalid release data: missing ID".to_string())?
    };

    // Upload package to release
    println!("📤 Uploading package to release...");
    let asset_url = github
        .upload_release_asset(owner, repo, release_id, &zip_path)
        .await
        .map_err(|e| format!("Failed to upload package: {}", e))?;

    println!("✅ Package uploaded successfully: {}", asset_url);

    Ok(())
}

/// Generate release notes from package information
fn generate_release_notes(package: &crate::models::package::Package) -> String {
    let mut notes = format!(
        "# {} v{}\n\n",
        package.package.name, package.package.version
    );

    notes.push_str(&format!("{}\n\n", package.package.description));

    if !package.commands.is_empty() {
        notes.push_str("## Commands\n\n");
        for (name, cmd) in &package.commands {
            notes.push_str(&format!("- `{}` - {}\n", name, cmd.description));
        }
        notes.push('\n');
    }

    notes.push_str("## Installation\n\n");
    notes.push_str("```bash\n");
    notes.push_str(&format!("aikit install {}/<repo>\n", package.package.name));
    notes.push_str("```\n\n");

    notes.push_str("## What's New\n\n");
    notes.push_str("- Initial release\n");
    notes.push_str("- Add your release notes here\n");

    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::git::PackageInfo;
    use crate::models::package::Package;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn create_test_package_dir() -> (TempDir, Package) {
        let temp_dir = TempDir::new().unwrap();
        let package_dir = temp_dir.path().join("test-package");
        fs::create_dir_all(&package_dir).unwrap();

        let manifest_content = r#"[package]
name = "test-package"
version = "0.1.0"
description = "Test package"
authors = ["test"]
"#;
        fs::write(package_dir.join("aikit.toml"), manifest_content).unwrap();

        let package = Package::from_toml_file(&package_dir.join("aikit.toml")).unwrap();
        (temp_dir, package)
    }

    fn create_test_zip_file(path: &Path) {
        use std::fs::File;
        use std::io::Write;
        use zip::write::ZipWriter;
        use zip::CompressionMethod;

        let file = File::create(path).unwrap();
        let mut zip = ZipWriter::new(file);

        zip.start_file(
            "aikit.toml",
            zip::write::FileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();

        zip.write_all(b"[package]\nname = \"test\"\nversion = \"0.1.0\"\n")
            .unwrap();

        zip.finish().unwrap();
    }

    #[test]
    fn test_find_package_zip_with_custom_path() {
        let (temp_dir, package) = create_test_package_dir();

        let package_dir = temp_dir.path().join("test-package");
        let custom_zip = temp_dir.path().join("custom-package.zip");
        create_test_zip_file(&custom_zip);

        let dist_dir = temp_dir.path().join("dist");
        fs::create_dir_all(&dist_dir).unwrap();
        let default_zip = dist_dir.join("test-package-0.1.0.zip");
        create_test_zip_file(&default_zip);

        let result = find_package_zip(&package, Some(custom_zip.to_str().unwrap()));

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), custom_zip);
    }

    #[test]
    fn test_find_package_zip_with_default_path() {
        let (temp_dir, package) = create_test_package_dir();

        let dist_dir = temp_dir.path().join("dist");
        fs::create_dir_all(&dist_dir).unwrap();
        let default_zip = dist_dir.join("test-package-0.1.0.zip");
        create_test_zip_file(&default_zip);

        let result = find_package_zip(&package, None);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), default_zip);
    }

    #[test]
    fn test_find_package_zip_not_found() {
        let (temp_dir, package) = create_test_package_dir();

        let result = find_package_zip(&package, None);

        assert!(result.is_err());
    }

    #[test]
    fn test_find_package_zip_with_invalid_path() {
        let (temp_dir, package) = create_test_package_dir();

        let result = find_package_zip(&package, Some("/nonexistent/path.zip"));

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_github_client_upload_release_asset_without_token() {
        use crate::core::git::GitHubClient;

        let client = GitHubClient::new(None);

        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test-upload.zip");
        create_test_zip_file(&test_file);

        let result = client
            .upload_release_asset("test-owner", "test-repo", 123, &test_file)
            .await;

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

    #[tokio::test]
    async fn test_generate_release_notes() {
        let (temp_dir, package) = create_test_package_dir();

        let notes = generate_release_notes(&package);

        assert!(notes.contains("Test package"));
        assert!(notes.contains("0.1.0"));
        assert!(notes.contains("Initial release"));
        assert!(notes.contains("Install"));
    }

    #[test]
    fn test_release_info_creation() {
        use crate::core::git::ReleaseInfo;

        let release_info = ReleaseInfo {
            tag_name: "v1.0.0".to_string(),
            name: "Release 1.0".to_string(),
            body: "Test release".to_string(),
            draft: false,
            prerelease: false,
        };

        assert_eq!(release_info.tag_name, "v1.0.0");
        assert_eq!(release_info.name, "Release 1.0");
        assert_eq!(release_info.draft, false);
        assert_eq!(release_info.prerelease, false);
    }

    #[test]
    fn test_release_info_prerelease_detection() {
        use crate::core::git::ReleaseInfo;

        let release_info_alpha = ReleaseInfo {
            tag_name: "v1.0.0-alpha".to_string(),
            name: "Alpha Release".to_string(),
            body: "Test".to_string(),
            draft: false,
            prerelease: false,
        };

        assert!(release_info_alpha.prerelease);

        let release_info_beta = ReleaseInfo {
            tag_name: "v1.0.0-beta".to_string(),
            name: "Beta Release".to_string(),
            body: "Test".to_string(),
            draft: false,
            prerelease: false,
        };

        assert!(release_info_beta.prerelease);

        let release_info_rc = ReleaseInfo {
            tag_name: "v1.0.0-rc1".to_string(),
            name: "RC Release".to_string(),
            body: "Test".to_string(),
            draft: false,
            prerelease: false,
        };

        assert!(release_info_rc.prerelease);

        let release_info_stable = ReleaseInfo {
            tag_name: "v1.0.0".to_string(),
            name: "Stable Release".to_string(),
            body: "Test".to_string(),
            draft: false,
            prerelease: false,
        };

        assert!(!release_info_stable.prerelease);
    }

    #[test]
    fn test_package_validate() {
        let (temp_dir, package) = create_test_package_dir();

        assert!(package.validate().is_ok());
    }

    #[test]
    fn test_package_from_toml() {
        let (temp_dir, package) = create_test_package_dir();

        assert_eq!(package.package.name, "test-package");
        assert_eq!(package.package.version, "0.1.0");
        assert_eq!(package.package.description, "Test package");
    }

    #[test]
    fn test_package_to_toml_string() {
        let (temp_dir, package) = create_test_package_dir();

        let toml_str = package.to_toml_string().unwrap();

        assert!(toml_str.contains("name = \"test-package\""));
        assert!(toml_str.contains("version = \"0.1.0\""));
        assert!(toml_str.contains("description = \"Test package\""));
    }

    #[test]
    fn test_package_create_template() {
        let package = Package::create_template(
            "test-package".to_string(),
            Some("Test description".to_string()),
            Some("test@example.com".to_string()),
            Some("0.2.0".to_string()),
        );

        assert_eq!(package.package.name, "test-package");
        assert_eq!(package.package.description, "Test description");
        assert_eq!(package.package.version, "0.2.0");
        assert!(!package.package.authors.is_empty());
    }

    #[test]
    fn test_package_validate_missing_name() {
        let package = Package::new("".to_string(), "0.1.0".to_string(), "Test".to_string());

        assert!(package.validate().is_err());
    }

    #[test]
    fn test_package_validate_missing_version() {
        let package = Package::new("test".to_string(), "".to_string(), "Test".to_string());

        assert!(package.validate().is_err());
    }

    #[test]
    fn test_package_validate_missing_description() {
        let package = Package::new("test".to_string(), "0.1.0".to_string(), "".to_string());

        assert!(package.validate().is_err());
    }

    #[test]
    fn test_package_install_dir() {
        let (temp_dir, package) = create_test_package_dir();

        assert_eq!(package.install_dir(), "test-package-0.1.0");
    }

    #[test]
    fn test_package_get_artifact_mappings() {
        let (temp_dir, package) = create_test_package_dir();

        let mappings = package.get_artifact_mappings(None);

        assert!(!mappings.is_empty());
        assert!(mappings.contains_key("templates/*.md"));
    }

    #[test]
    fn test_package_get_artifact_mappings_with_agent() {
        let (temp_dir, package) = create_test_package_dir();

        let mappings = package.get_artifact_mappings(Some("test-agent"));

        assert!(mappings.contains_key("templates/*.md"));
    }

    #[tokio::test]
    async fn test_package_build_creates_zip() {
        use std::fs::File;
        use zip::write::ZipWriter;
        use zip::CompressionMethod;

        let temp_dir = TempDir::new().unwrap();
        let package_dir = temp_dir.path().join("test-package");
        fs::create_dir_all(&package_dir).unwrap();

        // Create package.toml
        let manifest_content = r#"[package]
name = "test-package"
version = "0.1.0"
description = "Test package"
authors = ["test"]
"#;
        fs::write(package_dir.join("aikit.toml"), manifest_content).unwrap();

        // Create templates directory
        fs::create_dir_all(package_dir.join("templates")).unwrap();
        fs::write(
            package_dir.join("templates").join("example.md"),
            "# Example\n",
        )
        .unwrap();

        // Create scripts directory
        fs::create_dir_all(package_dir.join("scripts")).unwrap();

        // Create docs directory
        fs::create_dir_all(package_dir.join("docs")).unwrap();

        // Change to temp directory
        std::env::set_current_dir(&temp_dir).unwrap();

        // Run build
        let args = PackageBuildArgs {
            output: "dist".to_string(),
            agents: None,
            include_sources: false,
        };

        let result = execute_build(args).await;

        // Restore current directory
        std::env::set_current_dir("/home/sysuser/ws001/goaikit/aikit").unwrap();

        assert!(result.is_ok());

        // Verify ZIP was created
        let zip_path = PathBuf::from("dist").join("test-package-0.1.0.zip");
        assert!(zip_path.exists());
    }

    #[tokio::test]
    async fn test_package_publish_creates_release() {
        use crate::core::git::{GitHubClient, ReleaseInfo};
        use mockito::{Server, ServerGuard};

        // Create mock server
        let mut mock_server: ServerGuard = mockito::Server::new();

        // Mock the create release endpoint
        let create_mock = mock_server
            .mock("POST", "/repos/test-owner/test-repo/releases")
            .with_header("Authorization", "token test_token")
            .with_header("User-Agent", "AIKIT-Package-Manager/1.0")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(r#"
            {
                "id": 123,
                "tag_name": "v0.1.0",
                "name": "Release 0.1.0",
                "body": "Test release",
                "html_url": "https://github.com/test-owner/test-repo/releases/v0.1.0",
                "upload_url": "https://uploads.github.com/repos/test-owner/test-repo/releases/123/assets{?name,label}"
            }
            "#)
            .create();

        // Mock the upload asset endpoint
        let upload_mock = mock_server
            .mock("POST", "/repos/test-owner/test-repo/releases/123/assets")
            .with_header("Authorization", "token test_token")
            .with_header("Content-Type", "application/zip")
            .with_status(201)
            .create();

        // Create a test package directory
        let temp_dir = TempDir::new().unwrap();
        let package_dir = temp_dir.path().join("test-package");
        fs::create_dir_all(&package_dir).unwrap();

        let manifest_content = r#"[package]
name = "test-package"
version = "0.1.0"
description = "Test package"
authors = ["test"]
"#;
        fs::write(package_dir.join("aikit.toml"), manifest_content).unwrap();

        // Create dist folder with ZIP
        fs::create_dir_all(temp_dir.path().join("dist")).unwrap();
        let zip_path = temp_dir.path().join("dist").join("test-package-0.1.0.zip");

        use std::fs::File;
        use std::io::Write;
        use zip::write::ZipWriter;
        use zip::CompressionMethod;

        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file(
            "aikit.toml",
            zip::write::FileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(b"[package]\nname = \"test\"\nversion = \"0.1.0\"\n")
            .unwrap();
        zip.finish().unwrap();

        // Change to temp directory
        std::env::set_current_dir(&temp_dir).unwrap();

        // Run publish
        let args = PackagePublishArgs {
            repo: "test-owner/test-repo".to_string(),
            package: None,
            tag: Some("v0.1.0".to_string()),
            title: None,
            notes: None,
            token: Some("test_token".to_string()),
            no_release: false,
        };

        let result = execute_publish(args).await;

        // Restore current directory
        std::env::set_current_dir("/home/sysuser/ws001/goaikit/aikit").unwrap();

        // Verify mocks were called
        create_mock.assert();
        upload_mock.assert();

        // Check result
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_package_publish_with_no_release_flag() {
        use crate::core::git::GitHubClient;
        use mockito::Server;

        // Create mock server
        let mut mock_server = Server::new();

        // Mock the find release endpoint
        let find_mock = mock_server
            .mock("GET", "/repos/test-owner/test-repo/releases/tags/v0.1.0")
            .with_header("Authorization", "token test_token")
            .with_header("User-Agent", "AIKIT-Package-Manager/1.0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"
            {
                "id": 123,
                "tag_name": "v0.1.0",
                "name": "Release 0.1.0",
                "body": "Test release",
                "html_url": "https://github.com/test-owner/test-repo/releases/v0.1.0"
            }
            "#,
            )
            .create();

        // Create a test package directory
        let temp_dir = TempDir::new().unwrap();
        let package_dir = temp_dir.path().join("test-package");
        fs::create_dir_all(&package_dir).unwrap();

        let manifest_content = r#"[package]
name = "test-package"
version = "0.1.0"
description = "Test package"
authors = ["test"]
"#;
        fs::write(package_dir.join("aikit.toml"), manifest_content).unwrap();

        // Create dist folder with ZIP
        fs::create_dir_all(temp_dir.path().join("dist")).unwrap();
        let zip_path = temp_dir.path().join("dist").join("test-package-0.1.0.zip");

        use std::fs::File;
        use std::io::Write;
        use zip::write::ZipWriter;
        use zip::CompressionMethod;

        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file(
            "aikit.toml",
            zip::write::FileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(b"[package]\nname = \"test\"\nversion = \"0.1.0\"\n")
            .unwrap();
        zip.finish().unwrap();

        // Change to temp directory
        std::env::set_current_dir(&temp_dir).unwrap();

        // Run publish with --no-release flag
        let args = PackagePublishArgs {
            repo: "test-owner/test-repo".to_string(),
            package: None,
            tag: Some("v0.1.0".to_string()),
            title: None,
            notes: None,
            token: Some("test_token".to_string()),
            no_release: true,
        };

        let result = execute_publish(args).await;

        // Restore current directory
        std::env::set_current_dir("/home/sysuser/ws001/goaikit/aikit").unwrap();

        // Verify mock was called
        find_mock.assert();

        // Check result - should fail if release not found
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_package_publish_release_not_found() {
        use mockito::Server;

        // Create mock server
        let mut mock_server = Server::new();

        // Mock the find release endpoint with 404
        let find_mock = mock_server
            .mock("GET", "/repos/test-owner/test-repo/releases/tags/v0.1.0")
            .with_header("Authorization", "token test_token")
            .with_header("User-Agent", "AIKIT-Package-Manager/1.0")
            .with_status(404)
            .create();

        // Create a test package directory
        let temp_dir = TempDir::new().unwrap();
        let package_dir = temp_dir.path().join("test-package");
        fs::create_dir_all(&package_dir).unwrap();

        let manifest_content = r#"[package]
name = "test-package"
version = "0.1.0"
description = "Test package"
authors = ["test"]
"#;
        fs::write(package_dir.join("aikit.toml"), manifest_content).unwrap();

        // Create dist folder with ZIP
        fs::create_dir_all(temp_dir.path().join("dist")).unwrap();
        let zip_path = temp_dir.path().join("dist").join("test-package-0.1.0.zip");

        use std::fs::File;
        use std::io::Write;
        use zip::write::ZipWriter;
        use zip::CompressionMethod;

        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file(
            "aikit.toml",
            zip::write::FileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(b"[package]\nname = \"test\"\nversion = \"0.1.0\"\n")
            .unwrap();
        zip.finish().unwrap();

        // Change to temp directory
        std::env::set_current_dir(&temp_dir).unwrap();

        // Run publish with --no-release flag
        let args = PackagePublishArgs {
            repo: "test-owner/test-repo".to_string(),
            package: None,
            tag: Some("v0.1.0".to_string()),
            title: None,
            notes: None,
            token: Some("test_token".to_string()),
            no_release: true,
        };

        let result = execute_publish(args).await;

        // Restore current directory
        std::env::set_current_dir("/home/sysuser/ws001/goaikit/aikit").unwrap();

        // Verify mock was called
        find_mock.assert();

        // Check result - should fail because release not found
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No release found"));
    }

    #[test]
    fn test_generate_release_notes_basic() {
        let (temp_dir, package) = create_test_package_dir();

        let notes = generate_release_notes(&package);

        assert!(notes.contains("Test package"));
        assert!(notes.contains("0.1.0"));
        assert!(notes.contains("Initial release"));
    }

    #[test]
    fn test_generate_release_notes_with_commands() {
        let (temp_dir, mut package) = create_test_package_dir();

        // Add commands
        package.commands.insert(
            "run".to_string(),
            crate::models::package::CommandDefinition {
                description: "Run tests".to_string(),
                template: Some("run.md".to_string()),
            },
        );

        package.commands.insert(
            "build".to_string(),
            crate::models::package::CommandDefinition {
                description: "Build project".to_string(),
                template: Some("build.md".to_string()),
            },
        );

        let notes = generate_release_notes(&package);

        assert!(notes.contains("Commands"));
        assert!(notes.contains("run"));
        assert!(notes.contains("run tests"));
        assert!(notes.contains("build"));
        assert!(notes.contains("build project"));
    }
}
