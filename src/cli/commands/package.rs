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
    fs::create_dir_all(package_path)?;

    // Create subdirectories
    fs::create_dir_all(package_path.join("templates"))?;
    fs::create_dir_all(package_path.join("scripts"))?;
    fs::create_dir_all(package_path.join("docs"))?;

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
    )?;

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

    fs::write(package_path.join("README.md"), readme_content)?;

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
    use std::fs;
    use std::path::Path;

    let current_dir = std::env::current_dir()?;
    let package_path = current_dir.join("aikit.toml");

    // Check if aikit.toml exists
    if !package_path.exists() {
        return Err("aikit.toml not found. Run 'aikit package init' first.".into());
    }

    // Load and validate package
    let package = Package::from_toml_file(&package_path)?;
    package
        .validate()
        .map_err(|e| format!("Package validation failed: {}", e))?;

    // Create output directory
    fs::create_dir_all(&args.output)?;

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
    use std::io::Write;
    use walkdir::WalkDir;
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
    for (pattern, _dest) in &package.artifacts {
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
        if path.is_dir() || path.file_name() == Some(std::ffi::OsStr::new("aikit.toml")) {
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
            std::io::Write::write_all(zip, &content)?;
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
    use std::fs;

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
    let github = GitHubClient::new(Some(token));

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
    if !args.no_release {
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
    }

    println!("📤 Upload functionality would be implemented here");
    println!(
        "💡 For now, manually upload {} to the GitHub release",
        zip_path.display()
    );

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
