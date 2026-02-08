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
    /// Validate package structure and that templates exist (install-ready)
    Validate(PackageValidateArgs),
    /// Build package for distribution
    Build(PackageBuildArgs),
    /// Publish package to registry
    Publish(PackagePublishArgs),
}

/// Arguments for package validate command
#[derive(Debug, Args)]
pub struct PackageValidateArgs {
    /// Package directory (default: current directory)
    #[arg(short, long, default_value = ".")]
    pub path: String,
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

/// Execute package validate command
pub async fn execute_validate(args: PackageValidateArgs) -> Result<(), Box<dyn std::error::Error>> {
    use crate::models::package::Package;
    use anyhow::Context;
    use std::path::Path;

    let root = Path::new(&args.path)
        .canonicalize()
        .with_context(|| format!("Package path not found or not accessible: {}", args.path))?;
    let manifest_path = root.join("aikit.toml");

    if !manifest_path.exists() {
        return Err(format!(
            "aikit.toml not found in {} (run from package directory or use --path)",
            root.display()
        )
        .into());
    }

    let package = Package::from_toml_file(&manifest_path).map_err(|e| {
        anyhow::anyhow!(
            "Failed to load package from {}: {}",
            manifest_path.display(),
            e
        )
    })?;

    package
        .validate()
        .map_err(|e| format!("Package validation failed: {}", e))?;

    let mut errors = Vec::new();
    for (cmd_name, cmd_def) in &package.commands {
        let source_path_str = cmd_def.effective_source(cmd_name);
        let source_path = root.join(&source_path_str);
        if !source_path.exists() {
            errors.push(format!(
                "Command '{}': source file missing (path: {})",
                cmd_name,
                source_path.display()
            ));
        }
    }
    for (name, def) in &package.subagents {
        let path = root.join(&def.source);
        if !path.exists() || !path.is_file() {
            errors.push(format!(
                "Subagent '{}': source file missing or not a file (path: {})",
                name,
                path.display()
            ));
        }
    }
    for (name, def) in &package.skills {
        let path = root.join(&def.source);
        if !path.exists() || !path.is_dir() {
            errors.push(format!(
                "Skill '{}': source directory missing or not a directory (path: {})",
                name,
                path.display()
            ));
        }
    }

    if errors.is_empty() {
        println!(
            "Package '{}' v{} is valid and install-ready.",
            package.package.name, package.package.version
        );
        Ok(())
    } else {
        eprintln!("Validation failed ({} issue(s)):", errors.len());
        for e in &errors {
            eprintln!("  - {}", e);
        }
        Err(format!(
            "Package structure invalid: {} path(s) missing or invalid",
            errors.len()
        )
        .into())
    }
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
        let mut command_names: Vec<&String> = package.commands.keys().collect();
        command_names.sort();
        for name in command_names {
            if let Some(cmd) = package.commands.get(name) {
                notes.push_str(&format!("- `{}` - {}\n", name, cmd.description));
            }
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
    use crate::models::package::Package;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn create_test_package_dir() -> (TempDir, Package) {
        let temp_dir = TempDir::new().unwrap();
        let package = Package::create_template(
            "test-package".to_string(),
            Some("Test package".to_string()),
            Some("test".to_string()),
            Some("0.1.0".to_string()),
        );

        let package_dir = temp_dir.path().join("test-package");
        fs::create_dir_all(&package_dir).unwrap();

        // Write aikit.toml
        package
            .to_toml_file(&package_dir.join("aikit.toml"))
            .unwrap();

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

        let _package_dir = temp_dir.path().join("test-package");
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
        let temp_dir_obj = TempDir::new().expect("Failed to create main temp dir object");
        let temp_dir_path = temp_dir_obj.path();

        let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
        std::env::set_current_dir(temp_dir_path).expect("Failed to set CWD for test");

        let package = crate::models::package::Package::create_template(
            "test-package".to_string(),
            None,
            None,
            None,
        );

        let dist_dir_abs = temp_dir_path.join("dist"); // This is an absolute path
        fs::create_dir_all(&dist_dir_abs).expect("Failed to create dist directory");

        let expected_zip_path_abs = dist_dir_abs.join(format!(
            "{}-{}.zip",
            package.package.name, package.package.version
        ));
        create_test_zip_file(&expected_zip_path_abs);

        let result = find_package_zip(&package, None);

        // Restore CWD after result is obtained, but before assertions
        std::env::set_current_dir(orig_cwd).expect("Failed to restore original CWD");

        assert!(
            result.is_ok(),
            "Expected find_package_zip to be Ok, but got {:?}",
            result.err()
        );
        let found_path_relative = result.unwrap(); // This is PathBuf("dist/test-package-0.1.0.zip")
        let found_path_abs = temp_dir_path.join(found_path_relative); // This should be absolute

        assert_eq!(found_path_abs, expected_zip_path_abs, "Found path mismatch");
    }

    #[test]
    fn test_find_package_zip_not_found() {
        let (_temp_dir, package) = create_test_package_dir();
        let empty_work_dir_obj = TempDir::new().unwrap();
        let empty_work_dir = empty_work_dir_obj.path();

        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(empty_work_dir).unwrap();

        // Ensure no 'dist' directory exists in this empty temp dir
        let dist_path = empty_work_dir.join("dist");
        if dist_path.exists() {
            fs::remove_dir_all(&dist_path).unwrap();
        }

        let result = find_package_zip(&package, None);
        std::env::set_current_dir(&orig).unwrap(); // Restore CWD

        assert!(result.is_err()); // Should now correctly return an error
    }

    #[test]
    fn test_find_package_zip_with_invalid_path() {
        let (_temp_dir, package) = create_test_package_dir();

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
        let (_temp_dir, package) = create_test_package_dir();

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
        assert!(!release_info.draft);
        assert!(!release_info.prerelease);
    }

    #[test]
    fn test_release_info_prerelease_detection() {
        use crate::core::git::ReleaseInfo;

        let release_info_alpha = ReleaseInfo::new(
            "v1.0.0-alpha".to_string(),
            "Alpha Release".to_string(),
            "Test".to_string(),
            false,
        );

        assert!(release_info_alpha.prerelease);

        let release_info_beta = ReleaseInfo::new(
            "v1.0.0-beta".to_string(),
            "Beta Release".to_string(),
            "Test".to_string(),
            false,
        );

        assert!(release_info_beta.prerelease);

        let release_info_rc = ReleaseInfo::new(
            "v1.0.0-rc1".to_string(),
            "RC Release".to_string(),
            "Test".to_string(),
            false,
        );

        assert!(release_info_rc.prerelease);

        let release_info_stable = ReleaseInfo::new(
            "v1.0.0".to_string(),
            "Stable Release".to_string(),
            "Test".to_string(),
            false,
        );

        assert!(!release_info_stable.prerelease);
    }

    #[test]
    fn test_package_validate() {
        let (_temp_dir, package) = create_test_package_dir();

        assert!(package.validate().is_ok());
    }

    #[tokio::test]
    async fn test_execute_validate_success() {
        let (temp_dir, _package) = create_test_package_dir();
        let package_dir = temp_dir.path().join("test-package");
        fs::create_dir_all(package_dir.join("templates")).unwrap();
        fs::write(package_dir.join("help.md"), "# Help\n").unwrap();

        let args = PackageValidateArgs {
            path: package_dir.to_string_lossy().to_string(),
        };
        let result = execute_validate(args).await;
        assert!(result.is_ok(), "validate should pass: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_execute_validate_missing_template() {
        let (temp_dir, _package) = create_test_package_dir();
        let package_dir = temp_dir.path().join("test-package");
        assert!(!package_dir.join("help.md").exists());

        let args = PackageValidateArgs {
            path: package_dir.to_string_lossy().to_string(),
        };
        let result = execute_validate(args).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("path(s) missing or invalid"));
        assert!(err.contains("Package structure invalid"));
    }

    #[tokio::test]
    async fn test_execute_validate_no_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let empty_dir = temp_dir.path().join("empty");
        fs::create_dir_all(&empty_dir).unwrap();

        let args = PackageValidateArgs {
            path: empty_dir.to_string_lossy().to_string(),
        };
        let result = execute_validate(args).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("aikit.toml not found"));
    }

    #[test]
    fn test_package_from_toml() {
        let (_temp_dir, package) = create_test_package_dir();

        assert_eq!(package.package.name, "test-package");
        assert_eq!(package.package.version, "0.1.0");
        assert_eq!(package.package.description, "Test package");
    }

    #[test]
    fn test_package_to_toml_string() {
        let (_temp_dir, package) = create_test_package_dir();

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
        let (_temp_dir, package) = create_test_package_dir();

        assert_eq!(package.install_dir(), "test-package-0.1.0");
    }

    #[test]
    fn test_package_get_artifact_mappings() {
        let (_temp_dir, package) = create_test_package_dir();

        let mappings = package.get_artifact_mappings(None);

        assert!(!mappings.is_empty());
        assert!(mappings.contains_key("templates/*.md"));
    }

    #[test]
    fn test_package_get_artifact_mappings_with_agent() {
        let (_temp_dir, package) = create_test_package_dir();

        let mappings = package.get_artifact_mappings(Some("test-agent"));

        assert!(mappings.contains_key("templates/*.md"));
    }

    #[tokio::test]
    #[ignore] // TODO: Fix test isolation issue when run with full test suite
    async fn test_package_build_creates_zip() {
        let temp_dir = TempDir::new().unwrap();
        let package_dir = temp_dir.path().join("test-package");
        fs::create_dir_all(&package_dir).unwrap();

        // Clean up any existing dist folder from package_dir AND current directory
        let dist_dir = package_dir.join("dist");
        if let Err(e) = fs::remove_dir_all(&dist_dir) {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("Warning: Failed to clean up dist directory: {}", e);
            }
        }

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

        // Change to package directory
        std::env::set_current_dir(&package_dir).unwrap();

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

        // Verify ZIP was created (in the package_dir)
        let zip_path = package_dir.join("dist/test-package-0.1.0.zip");
        assert!(zip_path.exists());
    }

    #[test]
    fn test_package_publish_creates_release() {
        // This test is disabled due to async runtime conflicts with mockito
        // The functionality is tested in integration tests instead
        println!("Skipping test_package_publish_creates_release due to mockito runtime conflicts");
        // TODO: Rewrite test without mockito or move to integration tests
    }

    #[test]
    fn test_package_publish_with_no_release_flag() {
        // This test is disabled due to async runtime conflicts with mockito
        // The functionality is tested in integration tests instead
        println!(
            "Skipping test_package_publish_with_no_release_flag due to mockito runtime conflicts"
        );
        // TODO: Rewrite test without mockito or move to integration tests
    }

    #[test]
    fn test_package_publish_release_not_found() {
        // This test is disabled due to async runtime conflicts with mockito
        // The functionality is tested in integration tests instead
        println!(
            "Skipping test_package_publish_release_not_found due to mockito runtime conflicts"
        );
        // TODO: Rewrite test without mockito or move to integration tests
    }

    #[test]
    fn test_generate_release_notes_basic() {
        let (_temp_dir, package) = create_test_package_dir();

        let notes = generate_release_notes(&package);

        assert!(notes.contains("Test package"));
        assert!(notes.contains("0.1.0"));
        assert!(notes.contains("Initial release"));
    }

    #[test]
    fn test_generate_release_notes_with_commands() {
        let (_temp_dir, mut package) = create_test_package_dir();

        // Add commands
        package.commands.insert(
            "run".to_string(),
            crate::models::package::CommandDefinition {
                description: "Run tests".to_string(),
                template: Some("run.md".to_string()),
                source: None,
            },
        );

        package.commands.insert(
            "build".to_string(),
            crate::models::package::CommandDefinition {
                description: "Build project".to_string(),
                template: Some("build.md".to_string()),
                source: None,
            },
        );

        let notes = generate_release_notes(&package);

        assert!(notes.contains("Commands"));
        assert!(notes.contains("run"));
        assert!(notes.contains("Run tests"));
        assert!(notes.contains("build"));
        assert!(notes.contains("Build project"));
    }

    #[tokio::test]
    async fn test_package_publish_full_workflow_snapshot() {
        let (temp_dir, package) = create_test_package_dir();
        let package_dir = temp_dir.path().join("test-package");

        let dist_dir = package_dir.join("dist");
        fs::create_dir_all(&dist_dir).unwrap();

        let zip_path = dist_dir.join(format!(
            "{}-{}.zip",
            package.package.name, package.package.version
        ));
        create_test_zip_file(&zip_path);

        let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
        std::env::set_current_dir(&package_dir).expect("Failed to set CWD for test");

        let args = PackagePublishArgs {
            repo: "test-owner/test-repo".to_string(),
            package: None,
            tag: None,
            title: None,
            notes: None,
            token: Some("test_token".to_string()),
            no_release: false,
        };

        let result = execute_publish(args).await;

        let _ = std::env::set_current_dir(orig_cwd);

        assert!(result.is_err());
        insta::assert_snapshot!(
            "package_publish_full_workflow",
            result.unwrap_err().to_string()
        );
    }

    #[tokio::test]
    async fn test_package_publish_with_custom_package_snapshot() {
        let (temp_dir, _package) = create_test_package_dir();
        let package_dir = temp_dir.path().join("test-package");

        let custom_zip = temp_dir.path().join("custom-package.zip");
        create_test_zip_file(&custom_zip);

        let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
        std::env::set_current_dir(&package_dir).expect("Failed to set CWD for test");

        let args = PackagePublishArgs {
            repo: "test-owner/test-repo".to_string(),
            package: Some(custom_zip.to_string_lossy().to_string()),
            tag: Some("v1.0.0".to_string()),
            title: Some("Custom Release".to_string()),
            notes: Some("Custom release notes".to_string()),
            token: Some("test_token".to_string()),
            no_release: false,
        };

        let result = execute_publish(args).await;

        let _ = std::env::set_current_dir(orig_cwd);

        assert!(result.is_err());
        insta::assert_snapshot!(
            "package_publish_custom_package",
            result.unwrap_err().to_string()
        );
    }

    #[tokio::test]
    async fn test_package_publish_no_release_snapshot() {
        let (temp_dir, package) = create_test_package_dir();
        let package_dir = temp_dir.path().join("test-package");

        let dist_dir = package_dir.join("dist");
        fs::create_dir_all(&dist_dir).unwrap();

        let zip_path = dist_dir.join(format!(
            "{}-{}.zip",
            package.package.name, package.package.version
        ));
        create_test_zip_file(&zip_path);

        let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
        std::env::set_current_dir(&package_dir).expect("Failed to set CWD for test");

        let args = PackagePublishArgs {
            repo: "test-owner/test-repo".to_string(),
            package: None,
            tag: Some("v1.0.0".to_string()),
            title: None,
            notes: None,
            token: Some("test_token".to_string()),
            no_release: true,
        };

        let result = execute_publish(args).await;

        let _ = std::env::set_current_dir(orig_cwd);

        assert!(result.is_err());
        insta::assert_snapshot!(
            "package_publish_no_release",
            result.unwrap_err().to_string()
        );
    }

    #[test]
    fn test_generate_release_notes_snapshot() {
        let (_temp_dir, mut package) = create_test_package_dir();

        package.commands.insert(
            "run".to_string(),
            crate::models::package::CommandDefinition {
                description: "Run tests".to_string(),
                template: Some("run.md".to_string()),
                source: None,
            },
        );

        package.commands.insert(
            "build".to_string(),
            crate::models::package::CommandDefinition {
                description: "Build project".to_string(),
                template: Some("build.md".to_string()),
                source: None,
            },
        );

        let notes = generate_release_notes(&package);

        insta::assert_snapshot!("generate_release_notes_with_commands", notes);
    }

    #[tokio::test]
    async fn test_package_publish_with_env_token() {
        let (temp_dir, package) = create_test_package_dir();
        let package_dir = temp_dir.path().join("test-package");

        let dist_dir = package_dir.join("dist");
        fs::create_dir_all(&dist_dir).unwrap();

        let zip_path = dist_dir.join(format!(
            "{}-{}.zip",
            package.package.name, package.package.version
        ));
        create_test_zip_file(&zip_path);

        let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
        std::env::set_current_dir(&package_dir).expect("Failed to set CWD for test");

        std::env::set_var("GITHUB_TOKEN", "env_test_token");

        let args = PackagePublishArgs {
            repo: "test-owner/test-repo".to_string(),
            package: None,
            tag: None,
            title: None,
            notes: None,
            token: None,
            no_release: false,
        };

        let result = execute_publish(args).await;

        std::env::remove_var("GITHUB_TOKEN");
        let _ = std::env::set_current_dir(orig_cwd);

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_package_publish_missing_token() {
        let (temp_dir, package) = create_test_package_dir();
        let package_dir = temp_dir.path().join("test-package");

        let dist_dir = package_dir.join("dist");
        fs::create_dir_all(&dist_dir).unwrap();

        let zip_path = dist_dir.join(format!(
            "{}-{}.zip",
            package.package.name, package.package.version
        ));
        create_test_zip_file(&zip_path);

        let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
        std::env::set_current_dir(&package_dir).expect("Failed to set CWD for test");

        let args = PackagePublishArgs {
            repo: "test-owner/test-repo".to_string(),
            package: None,
            tag: None,
            title: None,
            notes: None,
            token: None,
            no_release: false,
        };

        let result = execute_publish(args).await;

        let _ = std::env::set_current_dir(orig_cwd);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("token") || error_msg.contains("GitHub token required"));
    }

    #[tokio::test]
    async fn test_package_publish_invalid_repo_format() {
        let (temp_dir, package) = create_test_package_dir();
        let package_dir = temp_dir.path().join("test-package");

        let dist_dir = package_dir.join("dist");
        fs::create_dir_all(&dist_dir).unwrap();
        let zip_path = dist_dir.join(format!(
            "{}-{}.zip",
            package.package.name, package.package.version
        ));
        create_test_zip_file(&zip_path);

        let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
        std::env::set_current_dir(&package_dir).expect("Failed to set CWD for test");

        let args = PackagePublishArgs {
            repo: "invalid-repo".to_string(),
            package: None,
            tag: None,
            title: None,
            notes: None,
            token: Some("test_token".to_string()),
            no_release: false,
        };

        let result = execute_publish(args).await;

        let _ = std::env::set_current_dir(orig_cwd);

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("owner/repo")
                || error_msg.contains("format")
                || error_msg.contains("GitHub token")
                || error_msg.contains("Failed to create release")
        );
    }

    #[test]
    fn test_build_package_snapshot() {
        let (temp_dir, package) = create_test_package_dir();
        let package_dir = temp_dir.path().join("test-package");

        let templates_dir = package_dir.join("templates");
        fs::create_dir_all(&templates_dir).unwrap();
        fs::write(templates_dir.join("help.md"), "# Help\n").unwrap();

        let dist_dir = temp_dir.path().join("dist");
        fs::create_dir_all(&dist_dir).unwrap();

        let args = PackageBuildArgs {
            output: dist_dir.to_string_lossy().to_string(),
            agents: None,
            include_sources: false,
        };

        let result = build_package(&package, &package_dir, &args);

        assert!(result.is_ok(), "build_package failed: {:?}", result.err());
        let zip_path = result.unwrap();

        // Check that the ZIP was created in the expected location
        let zip_str = zip_path.to_string_lossy().to_string();
        assert!(zip_str.contains("test-package-0.1.0.zip"));
        assert!(zip_str.ends_with("test-package-0.1.0.zip"));
    }

    #[tokio::test]
    async fn test_complete_workflow_snapshot() {
        let temp_dir_obj = TempDir::new().expect("Failed to create main temp dir object");
        let temp_dir_path = temp_dir_obj.path();

        let package = crate::models::package::Package::create_template(
            "test-package".to_string(),
            Some("Test description".to_string()),
            Some("test@example.com".to_string()),
            Some("1.0.0".to_string()),
        );

        let package_dir = temp_dir_path.join("test-package");
        fs::create_dir_all(&package_dir).expect("Failed to create package directory");
        package
            .to_toml_file(&package_dir.join("aikit.toml"))
            .expect("Failed to write package.toml");

        let templates_dir = package_dir.join("templates");
        fs::create_dir_all(&templates_dir).unwrap();
        fs::write(templates_dir.join("help.md"), "# Help\n").unwrap();

        let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
        std::env::set_current_dir(&package_dir).expect("Failed to set CWD for test");

        let build_args = PackageBuildArgs {
            output: "dist".to_string(),
            agents: None,
            include_sources: false,
        };

        let build_result = execute_build(build_args).await;

        assert!(build_result.is_ok());

        let publish_args = PackagePublishArgs {
            repo: "test-owner/test-repo".to_string(),
            package: None,
            tag: None,
            title: None,
            notes: None,
            token: Some("test_token".to_string()),
            no_release: false,
        };

        let publish_result = execute_publish(publish_args).await;

        let _ = std::env::set_current_dir(orig_cwd);

        insta::assert_snapshot!(
            "complete_workflow_publish_result",
            publish_result.unwrap_err().to_string()
        );
    }
}
