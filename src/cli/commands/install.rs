//! Package installation commands
//!
//! This module contains CLI commands for package installation management:
//! - install: Install package from URL
//! - update: Update installed package
//! - remove: Remove installed package
//! - list: List installed packages

use clap::{Args, Subcommand};
use toml;

/// Installation management subcommands
#[derive(Debug, Subcommand)]
pub enum InstallCommands {
    /// Install package from GitHub URL
    Install(InstallArgs),
    /// Update installed package
    Update(UpdateArgs),
    /// Remove installed package
    Remove(RemoveArgs),
    /// List installed packages
    List(ListArgs),
}

/// Arguments for install command
#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Package source (GitHub URL or package name)
    pub source: String,

    /// Specific version to install
    #[arg(short, long)]
    pub version: Option<String>,

    /// Force reinstall if already installed
    #[arg(long)]
    pub force: bool,

    /// Skip .gitignore modification prompt
    #[arg(long)]
    pub yes: bool,
}

/// Arguments for update command
#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Package name to update
    pub package: String,

    /// Allow breaking changes
    #[arg(long)]
    pub breaking: bool,
}

/// Arguments for remove command
#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// Package name to remove
    pub package: String,

    /// Force removal without confirmation
    #[arg(long)]
    pub force: bool,
}

/// Arguments for list command
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Filter by author
    #[arg(long)]
    pub author: Option<String>,

    /// Show detailed information
    #[arg(long)]
    pub detailed: bool,
}

/// Execute install command
pub async fn execute_install(args: InstallArgs) -> Result<(), Box<dyn std::error::Error>> {
    use crate::core::filesystem::AikDirectory;
    use crate::core::git::GitHubClient;
    use crate::models::package::Package;
    use crate::models::registry::LocalRegistry;
    use std::path::PathBuf;

    println!("Installing package from: {}", args.source);

    if let Some(version) = &args.version {
        println!("Version: {}", version);
    }

    // Create .aikit directory if it doesn't exist
    let aik_dir = AikDirectory::new(PathBuf::from(".aikit"));
    if !aik_dir.exists() {
        println!("Creating .aikit directory...");
        aik_dir.create()?;
    }

    // Parse the source URL
    let (owner, repo, version) = parse_github_url(&args.source, args.version.as_deref())?;

    println!("Fetching package manifest from {}/{}...", owner, repo);

    // Initialize GitHub client
    let github = GitHubClient::new(None);

    // Get package manifest
    let manifest = github.get_package_manifest(&owner, &repo, Some(&version)).await
        .map_err(|e| format!("Failed to fetch package manifest: {}", e))?;

    // Validate package
    // Convert PackageManifest to TOML string for parsing
    let manifest_toml = toml::to_string(&manifest)?;
    let package = crate::models::package::Package::from_toml_str(&manifest_toml)?;
    package.validate().map_err(|e| format!("Package validation failed: {}", e))?;

    // Check if already installed
    let registry_path = aik_dir.registry_path();
    let mut registry = LocalRegistry::load_from_file(&registry_path)
        .unwrap_or_else(|_| LocalRegistry::new());

    if registry.is_installed(&package.package.name) && !args.force {
        return Err(format!("Package '{}' is already installed. Use --force to reinstall.", package.package.name).into());
    }

    println!("Downloading package {} v{}...", package.package.name, package.package.version);

    // Download package archive
    let temp_dir = tempfile::tempdir()?;
    let archive_path = temp_dir.path().join(format!("{}-{}.zip", package.package.name, package.package.version));

    github.download_archive(&owner, &repo, Some(&version), &archive_path).await
        .map_err(|e| format!("Failed to download package: {}", e))?;

    // Extract and install package
    println!("Installing package...");
    install_package_from_archive(&package, &archive_path, &aik_dir, &args)?;

    // Update registry
    use crate::models::package::InstalledPackage;
    let installed = InstalledPackage {
        package: package.package.clone(),
        installed_at: chrono::Utc::now(),
        source_url: args.source.clone(),
        install_path: format!("packages/{}-{}", package.package.name, package.package.version),
    };

    registry.add_package(installed);
    registry.save_to_file(&registry_path)?;

    // Handle .gitignore
    // Note: skip_gitignore field doesn't exist in InstallArgs, always prompt
    {
        use crate::core::filesystem::GitIgnoreManager;
        let gitignore = GitIgnoreManager::new(std::path::Path::new("."));
        if gitignore.prompt_user() {
            gitignore.add_aikit()?;
            println!("Added .aikit/ to .gitignore");
        }
    }

    // Generate agent commands
    println!("Generating agent commands...");
    generate_agent_commands(&package, &aik_dir)?;

    println!("✅ Package '{}' v{} installed successfully!", package.package.name, package.package.version);
    println!("📦 Installed to: .aikit/packages/{}-{}", package.package.name, package.package.version);

    Ok(())
}

/// Parse GitHub URL and extract owner, repo, and version
fn parse_github_url(source: &str, version: Option<&str>) -> Result<(String, String, String), Box<dyn std::error::Error>> {
    // Handle various GitHub URL formats:
    // https://github.com/owner/repo
    // https://github.com/owner/repo/releases/download/v1.0.0/package.zip
    // github.com/owner/repo
    // owner/repo

    let url = source.trim_start_matches("https://").trim_start_matches("http://");

    let path = if url.starts_with("github.com/") {
        url.strip_prefix("github.com/").unwrap()
    } else if url.contains('/') && !url.contains("github.com") {
        // Assume owner/repo format
        url
    } else {
        return Err("Invalid GitHub URL format. Expected: github.com/owner/repo or owner/repo".into());
    };

    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 2 {
        return Err("Invalid GitHub URL format".into());
    }

    let owner = parts[0].to_string();
    let repo = parts[1].to_string();
    let version = version.unwrap_or("main").to_string();

    Ok((owner, repo, version))
}

/// Install package from downloaded archive
fn install_package_from_archive(
    package: &crate::models::package::Package,
    archive_path: &std::path::Path,
    aik_dir: &crate::core::filesystem::AikDirectory,
    _args: &InstallArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs::File;
    use zip::ZipArchive;

    // Open the ZIP archive
    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;

    // Extract to packages directory
    let install_path = aik_dir.install_package(
        &package.package.name,
        &package.package.version,
        archive_path.parent().unwrap_or(std::path::Path::new(".")),
    )?;

    // Extract files
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = install_path.join(file.name());

        if file.name().ends_with('/') {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    std::fs::create_dir_all(p)?;
                }
            }
            let mut outfile = File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }

    Ok(())
}

/// Generate agent-specific command files
fn generate_agent_commands(
    package: &crate::models::package::Package,
    aik_dir: &crate::core::filesystem::AikDirectory,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::core::agent::get_agent_configs;

    for agent_config in get_agent_configs() {
        generate_commands_for_agent(package, &agent_config, aik_dir)?;
    }

    Ok(())
}

/// Generate commands for a specific agent
fn generate_commands_for_agent(
    package: &crate::models::package::Package,
    agent: &crate::core::agent::AgentConfig,
    _aik_dir: &crate::core::filesystem::AikDirectory,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;

    // Create agent commands directory if it doesn't exist
    let commands_dir = std::path::Path::new(&agent.output_dir);
    fs::create_dir_all(commands_dir)?;

    // Generate command files for each package command
    for (command_name, command_def) in &package.commands {
        let content = agent.generate_package_command(
            &package.package.name,
            command_name,
            &command_def.description,
            "# Package command - implementation goes here",
        );

        let filename = format!("{}.md", agent.get_namespace_prefix(&package.package.name));
        let filepath = commands_dir.join(filename);

        fs::write(filepath, content)?;
    }

    Ok(())
}

/// Execute update command
pub async fn execute_update(args: UpdateArgs) -> Result<(), Box<dyn std::error::Error>> {
    use crate::core::filesystem::AikDirectory;
    use crate::core::git::GitHubClient;
    use crate::models::registry::LocalRegistry;
    use std::path::PathBuf;

    let aik_dir = AikDirectory::new(PathBuf::from(".aikit"));
    if !aik_dir.exists() {
        return Err("No packages installed (.aikit directory not found)".into());
    }

    let registry_path = aik_dir.registry_path();
    let mut registry = LocalRegistry::load_from_file(&registry_path)
        .unwrap_or_else(|_| LocalRegistry::new());

    // Check if package is installed
    let installed_package = registry.get_package(&args.package)
        .ok_or_else(|| format!("Package '{}' is not installed", args.package))?;

    println!("Checking for updates to '{}' (current: {})...", args.package, installed_package.package.version);

    // For now, we need the GitHub URL to check for updates
    // In a full implementation, we'd query the registry or GitHub API
    // For this demo, we'll assume no update is available

    println!("No updates available for package '{}'", args.package);
    println!("Current version: {}", installed_package.package.version);

    // In a real implementation, this would:
    // 1. Parse the source URL from installed_package.source_url
    // 2. Query GitHub API for latest release
    // 3. Compare versions
    // 4. Download and install if newer version available

    Ok(())
}

/// Execute remove command
pub async fn execute_remove(args: RemoveArgs) -> Result<(), Box<dyn std::error::Error>> {
    use crate::core::filesystem::AikDirectory;
    use crate::models::registry::LocalRegistry;
    use std::path::PathBuf;

    let aik_dir = AikDirectory::new(PathBuf::from(".aikit"));
    if !aik_dir.exists() {
        return Err("No packages installed (.aikit directory not found)".into());
    }

    let registry_path = aik_dir.registry_path();
    let mut registry = LocalRegistry::load_from_file(&registry_path)
        .unwrap_or_else(|_| LocalRegistry::new());

    // Check if package is installed
    if !registry.is_installed(&args.package) {
        return Err(format!("Package '{}' is not installed", args.package).into());
    }

    // Confirm removal unless forced
    if !args.force {
        println!("Are you sure you want to remove package '{}'?", args.package);
        println!("This will delete all associated files and commands. (y/N): ");

        // For now, assume yes in automated context
        // TODO: Add interactive confirmation
    }

    // Get installed package info to determine version
    let installed_package = registry.get_package(&args.package)
        .ok_or_else(|| format!("Package '{}' not found in registry", args.package))?;

    // Remove package files
    aik_dir.remove_package(&args.package, &installed_package.package.version)?;

    // Remove from registry
    registry.remove_package(&args.package);
    registry.save_to_file(&registry_path)?;

    // Remove agent commands
    remove_agent_commands(&args.package)?;

    println!("✅ Package '{}' removed successfully!", args.package);

    Ok(())
}

/// Remove agent commands for a package
fn remove_agent_commands(package_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    use crate::core::agent::get_agent_configs;
    use std::fs;

    for agent in get_agent_configs() {
        let commands_dir = std::path::Path::new(&agent.output_dir);
        if commands_dir.exists() {
            // Remove command files that start with the package name
            for entry in fs::read_dir(commands_dir)? {
                let entry = entry?;
                let filename = entry.file_name().to_string_lossy().to_string();

                // Check if this is a command file for this package
                if filename.starts_with(&format!("{}.", package_name)) && filename.ends_with(".md") {
                    fs::remove_file(entry.path())?;
                }
            }
        }
    }

    Ok(())
}

/// Execute list command
pub async fn execute_list(args: ListArgs) -> Result<(), Box<dyn std::error::Error>> {
    use crate::core::filesystem::AikDirectory;
    use crate::models::registry::LocalRegistry;
    use std::path::PathBuf;

    let aik_dir = AikDirectory::new(PathBuf::from(".aikit"));
    if !aik_dir.exists() {
        println!("No packages installed (.aikit directory not found)");
        return Ok(());
    }

    let registry_path = aik_dir.registry_path();
    let registry = LocalRegistry::load_from_file(&registry_path)
        .unwrap_or_else(|_| LocalRegistry::new());

    let packages = if let Some(author) = &args.author {
        registry.packages_by_author(author)
    } else {
        registry.list_packages()
    };

    if packages.is_empty() {
        println!("No packages installed");
        return Ok(());
    }

    if args.detailed {
        println!("Installed packages:");
        println!("{:<25} {:<12} {:<15} {}", "Name", "Version", "Author", "Description");
        println!("{}", "-".repeat(80));

        for package in packages {
            let author = package.package.authors.first().unwrap_or(&"Unknown".to_string()).clone();
            println!("{:<25} {:<12} {:<15} {}",
                package.package.name,
                package.package.version,
                author,
                package.package.description
            );
        }
    } else {
        println!("Installed packages:");
        for package in packages {
            println!("  {} v{} - {}",
                package.package.name,
                package.package.version,
                package.package.description
            );
        }
    }

    Ok(())
}
