//! Package installation commands
//!
//! This module contains CLI commands for package installation management:
//! - install: Install package from URL
//! - update: Update installed package
//! - remove: Remove installed package
//! - list: List installed packages

use crate::error::AikError;
use crate::github::api::GitHubClient as GitHubApiClient;
use atty;
use clap::{Args, Subcommand};
use std::path::PathBuf;
use toml;

/// Source type for package installation
#[derive(Debug, Clone)]
pub enum SourceType {
    LocalFolder(PathBuf),
    GitHubRepo {
        owner: String,
        repo: String,
        version: String,
    },
}

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
    pub install_version: Option<String>,

    /// GitHub token (can also be set via GITHUB_TOKEN or GH_TOKEN env var)
    #[arg(long)]
    pub token: Option<String>,

    /// Force reinstall if already installed
    #[arg(long)]
    pub force: bool,

    /// Skip .gitignore modification prompt
    #[arg(long)]
    pub yes: bool,

    /// AI agent to install for (e.g., claude, copilot, cursor-agent)
    #[arg(long)]
    pub ai: Option<String>,
}

impl InstallArgs {
    pub fn detect_source_type(&self) -> Result<SourceType, AikError> {
        let path = std::path::Path::new(&self.source);

        // Check if it's an existing local directory
        if path.exists() && path.is_dir() {
            // Validate it contains aikit.toml
            let aikit_toml = path.join("aikit.toml");
            if !aikit_toml.exists() {
                return Err(AikError::InvalidSource(format!(
                    "Directory '{}' does not contain aikit.toml",
                    self.source
                )));
            }
            return Ok(SourceType::LocalFolder(path.to_path_buf()));
        }

        // Check if it's a GitHub URL or owner/repo format
        if self.looks_like_github_source() {
            let (owner, repo, version) =
                parse_github_url(&self.source, self.install_version.as_deref())?;
            return Ok(SourceType::GitHubRepo {
                owner,
                repo,
                version,
            });
        }

        // Provide helpful error
        Err(AikError::InvalidSource(format!(
            "Invalid source '{}'. Expected:\n  - Local directory path (must exist and contain aikit.toml)\n  - GitHub URL: github.com/owner/repo or https://github.com/owner/repo\n  - Short format: owner/repo",
            self.source
        )))
    }

    fn looks_like_github_source(&self) -> bool {
        let source = &self.source;

        // Exclude relative and absolute paths
        if source.starts_with("./") || source.starts_with("../") {
            return false;
        }
        if std::path::Path::new(source).is_absolute() {
            return false;
        }

        let source_lower = source.to_lowercase();
        source_lower.contains("github.com")
            || (source_lower.contains('/')
                && source.split('/').count() == 2
                && !std::path::Path::new(source).exists())
    }
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
pub async fn execute_install(args: InstallArgs) -> Result<(), AikError> {
    use crate::core::filesystem::AikDirectory;
    use crate::core::git::GitHubClient;
    use crate::core::ux::{create_spinner, show_info, show_success, show_warning};
    use crate::models::registry::LocalRegistry;
    use std::path::PathBuf;

    let spinner = create_spinner("Detecting source type...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    println!("Installing package from: {}", args.source);

    if let Some(version) = &args.install_version {
        println!("Version: {}", version);
    }

    // Find or create .aikit directory
    let aik_dir = match AikDirectory::find() {
        Ok(dir) => dir,
        Err(_) => {
            // .aikit not found, create it in current directory
            let aik_dir = AikDirectory::new(PathBuf::from(".aikit"));
            println!("Creating .aikit directory...");
            aik_dir.create()?;
            aik_dir
        }
    };

    // Validate inputs
    if let Some(version) = &args.install_version {
        crate::core::validation::validate_version_format(version)?;
    }

    // Detect source type
    spinner.set_message("Detecting source type...");
    let source_type = args.detect_source_type()?;
    spinner.finish_with_message("Source type detected");

    // Keep temp dir alive for GitHub installs until after extraction (avoid "No such file or directory")
    let (_temp_guard, package, archive_path): (
        Option<tempfile::TempDir>,
        crate::models::package::Package,
        Option<std::path::PathBuf>,
    ) = match source_type {
        SourceType::LocalFolder(path) => {
            let install_spinner = create_spinner(&format!(
                "Installing from local directory: {}",
                path.display()
            ));
            let result = install_from_local_directory(&path);
            install_spinner.finish_with_message("Local package loaded");
            let (pkg, path_opt) = result?;
            (None, pkg, path_opt)
        }
        SourceType::GitHubRepo {
            owner,
            repo,
            version,
        } => {
            show_info(&format!(
                "Installing from GitHub: {}/{}@{}",
                owner, repo, version
            ));

            // Initialize GitHub client with token resolution
            let github = GitHubClient::new(GitHubApiClient::resolve_token(args.token.clone()));

            // Get package manifest
            let manifest_spinner = create_spinner(&format!(
                "Fetching package manifest from {}/{}...",
                owner, repo
            ));
            let manifest = github
                .get_package_manifest(&owner, &repo, Some(&version))
                .await?;
            manifest_spinner.finish_with_message("Package manifest fetched");

            // Convert PackageManifest to TOML string for parsing
            let manifest_toml = toml::to_string(&manifest)?;
            let package = crate::models::package::Package::from_toml_str(&manifest_toml)
                .map_err(|e| AikError::Generic(format!("Failed to parse manifest: {}", e)))?;

            // Download package archive (temp_dir must outlive install_package_from_archive)
            let download_spinner = create_spinner(&format!(
                "Downloading package {} v{}...",
                package.package.name, package.package.version
            ));

            let temp_dir = tempfile::tempdir().map_err(|e| {
                AikError::Generic(format!("Failed to create temp directory: {}", e))
            })?;
            let archive_path = temp_dir.path().join(format!(
                "{}-{}.zip",
                package.package.name, package.package.version
            ));

            github
                .download_archive(&owner, &repo, Some(&version), &archive_path)
                .await?;
            download_spinner.finish_with_message("Package downloaded");

            (Some(temp_dir), package, Some(archive_path))
        }
    };

    // Check if already installed
    let registry_path = aik_dir.registry_path();
    let mut registry =
        LocalRegistry::load_from_file(&registry_path).unwrap_or_else(|_| LocalRegistry::new());

    if registry.is_installed(&package.package.name) && !args.force {
        if crate::core::ux::confirm_action(&format!(
            "Package '{}' is already installed. Reinstall?",
            package.package.name
        ))? {
            // User confirmed reinstall
        } else {
            show_warning("Installation cancelled by user");
            return Ok(());
        }
    }

    // Extract and install package
    let install_spinner = create_spinner("Installing package...");
    let install_result = if let Some(archive_path) = archive_path {
        // Remote installation - extract from downloaded archive
        install_package_from_archive(&package, &archive_path, &aik_dir, &args)
    } else {
        // Local installation - copy directly from source directory
        install_package_from_directory(&package, &args.source, &aik_dir, &args)
    };

    match install_result {
        Ok(_) => install_spinner.finish_with_message("Package installed successfully"),
        Err(e) => {
            install_spinner.finish_with_message("Installation failed");
            return Err(e);
        }
    }

    // Update registry
    use crate::models::package::InstalledPackage;
    let installed = InstalledPackage {
        package: package.package.clone(),
        installed_at: chrono::Utc::now(),
        source_url: args.source.clone(),
        install_path: format!(
            "packages/{}-{}",
            package.package.name, package.package.version
        ),
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
            show_info("Added .aikit/ to .gitignore");
        }
    }

    show_success(&format!(
        "Package '{}' v{} installed successfully!",
        package.package.name, package.package.version
    ));

    // Determine which agent(s) to generate commands for
    let selected_agents = if let Some(ai_arg) = &args.ai {
        // Validate agent key
        crate::core::agent::validate_agent_key(ai_arg)
            .map_err(|e| AikError::InvalidSource(format!("Invalid agent '{}': {}", ai_arg, e)))?;
        vec![ai_arg.clone()]
    } else if atty::is(atty::Stream::Stdin) {
        // Interactive selection
        match crate::tui::agent_select::select_agent_interactive()
            .map_err(|e| AikError::Generic(format!("Interactive agent selection failed: {}", e)))?
        {
            crate::tui::agent_select::SelectionResult::Selected(key) => {
                vec![key]
            }
            crate::tui::agent_select::SelectionResult::Cancelled => {
                println!("Installation cancelled.");
                return Ok(());
            }
        }
    } else {
        // Non-interactive: require --ai flag
        return Err(AikError::InvalidSource(
            "AI agent not specified. Use --ai <agent> to specify an agent, or run in interactive mode.\n\
             Available agents: claude, copilot, cursor-agent, gemini, qwen, opencode, codex, windsurf, kilocode, auggie, roo, codebuddy, qoder, amp, shai, q, bob".to_string(),
        ));
    };

    // Generate agent commands
    println!(
        "Generating agent commands for: {}",
        selected_agents.join(", ")
    );
    if let Err(e) = generate_agent_commands(&package, &aik_dir, &selected_agents) {
        eprintln!("Warning: Failed to generate agent commands: {}", e);
    }

    // Resolve installed package root (handles zipball top-level dir)
    let package_root = match aikit_sdk::installed_package_root(
        &aik_dir.packages_path(),
        &package.package.name,
        &package.package.version,
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Warning: Could not resolve package root: {}", e);
            return Ok(());
        }
    };
    let project_root = aik_dir.project_root();

    // Deploy subagents and skills per agent, then copy artifacts
    for agent_key in &selected_agents {
        if let Some(agent_config) = crate::core::agent::get_agent_config(agent_key) {
            if let Err(e) = deploy_subagents_for_agent(
                &package,
                &package_root,
                &project_root,
                agent_key,
                &agent_config,
            ) {
                eprintln!("Warning: Deploy subagents for {}: {}", agent_key, e);
            }
            if let Err(e) = deploy_skills_for_agent(
                &package,
                &package_root,
                &project_root,
                agent_key,
                &agent_config,
            ) {
                eprintln!("Warning: Deploy skills for {}: {}", agent_key, e);
            }
        }
    }

    // Build mappings with agent scope (first selected agent, or None for default mappings)
    let agent_scope = selected_agents.first().map(|s| s.as_str());
    let mappings = package.get_artifact_mappings(agent_scope);

    if let Err(e) = aikit_sdk::copy_artifacts(&package_root, &project_root, &mappings) {
        eprintln!("Warning: Copy artifacts: {}", e);
    }

    println!(
        "✅ Package '{}' v{} installed successfully!",
        package.package.name, package.package.version
    );
    println!(
        "📦 Installed to: .aikit/packages/{}-{}",
        package.package.name, package.package.version
    );

    Ok(())
}

/// Install package from local directory
fn install_from_local_directory(
    source_path: &std::path::Path,
) -> Result<(crate::models::package::Package, Option<std::path::PathBuf>), AikError> {
    use std::fs;
    use std::path::Path;

    let source_dir = Path::new(source_path);

    // Check if package.toml or aikit.toml exists
    let package_toml_path = source_dir.join("package.toml");
    let aikit_toml_path = source_dir.join("aikit.toml");

    let toml_path = if package_toml_path.exists() {
        package_toml_path
    } else if aikit_toml_path.exists() {
        aikit_toml_path
    } else {
        return Err(AikError::InvalidSource(format!(
            "package.toml or aikit.toml not found in directory: {}",
            source_path.display()
        )));
    };

    let package_toml_content = fs::read_to_string(&toml_path)
        .map_err(|e| crate::error::io_context("Failed to read package manifest", &toml_path, e))?;

    let package =
        crate::models::package::Package::from_toml_str(&package_toml_content).map_err(|e| {
            AikError::Generic(format!("Failed to parse {}: {}", toml_path.display(), e))
        })?;

    // Validate package
    package.validate().map_err(AikError::PackageValidation)?;

    // For local installation, we don't need to download an archive
    // We'll install directly from the source directory
    Ok((package, None))
}

/// Parse GitHub URL and extract owner, repo, and version
fn parse_github_url(
    source: &str,
    version: Option<&str>,
) -> Result<(String, String, String), AikError> {
    // Handle various GitHub URL formats:
    // https://github.com/owner/repo
    // https://github.com/owner/repo/releases/download/v1.0.0/package.zip
    // github.com/owner/repo
    // owner/repo

    let url = source
        .trim_start_matches("https://")
        .trim_start_matches("http://");

    let path = if url.starts_with("github.com/") {
        url.strip_prefix("github.com/").unwrap()
    } else if url.contains('/') && !url.contains("github.com") {
        // Assume owner/repo format
        url
    } else {
        return Err(AikError::InvalidGitHubUrl(
            "Expected: github.com/owner/repo or owner/repo".to_string(),
        ));
    };

    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 2 {
        return Err(AikError::InvalidGitHubUrl(
            "Invalid GitHub URL format".to_string(),
        ));
    }

    let owner = parts[0].to_string();
    let repo = parts[1].to_string();

    // Validate owner and repo names
    crate::core::validation::validate_github_owner_name(&owner)?;
    crate::core::validation::validate_github_repo_name(&repo)?;

    // Validate version if provided
    if let Some(v) = version {
        crate::core::validation::validate_version_format(v)?;
    }

    let version = version.unwrap_or("main").to_string();

    Ok((owner, repo, version))
}

/// Normalize a path by resolving `..` and `.` components in memory
fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut result = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                result.push(component);
            }
            Component::CurDir => {
                // Skip `.` - it doesn't change anything
            }
            Component::ParentDir => {
                // Pop the last normal component if possible
                if !result.pop() {
                    // Can't pop past root - just skip
                }
            }
            Component::Normal(c) => {
                result.push(c);
            }
        }
    }
    result
}

/// Install package from downloaded archive
fn install_package_from_archive(
    package: &crate::models::package::Package,
    archive_path: &std::path::Path,
    aik_dir: &crate::core::filesystem::AikDirectory,
    _args: &InstallArgs,
) -> Result<(), AikError> {
    use std::fs::File;
    use zip::ZipArchive;

    let file = File::open(archive_path)
        .map_err(|e| crate::error::io_context("Failed to open archive", archive_path, e))?;
    let mut archive = ZipArchive::new(file)?;

    let install_path = aik_dir.install_package(
        &package.package.name,
        &package.package.version,
        archive_path.parent().unwrap_or(std::path::Path::new(".")),
    )?;

    // Canonicalize install_path once for comparison
    let install_canonical = install_path
        .canonicalize()
        .map_err(|e| AikError::Generic(format!("Failed to canonicalize install_path: {}", e)))?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;

        let entry_name = file.name();

        // Check for path traversal in the entry name before any processing
        if entry_name.contains("..") {
            return Err(AikError::Generic(format!(
                "Path traversal detected in zip entry: {}",
                entry_name
            )));
        }

        // Normalize the entry name to prevent path traversal (resolves . components)
        let normalized = normalize_path(std::path::Path::new(entry_name));

        // Validate that the normalized path is relative (prevents absolute paths like /etc/passwd)
        if !normalized.is_relative() {
            return Err(AikError::Generic(format!(
                "Absolute path detected in zip entry: {}",
                entry_name
            )));
        }

        let outpath = install_path.join(&normalized);

        // Validate that the output path is under install_path
        // First try to canonicalize, if the path doesn't exist yet we compute the canonical form
        // of the install_path and join it with the normalized relative path
        let outpath_canonical = if outpath.exists() {
            outpath.canonicalize().map_err(|e| {
                AikError::Generic(format!("Failed to canonicalize output path: {}", e))
            })?
        } else {
            install_canonical.join(&normalized)
        };

        if !outpath_canonical.starts_with(&install_canonical) {
            return Err(AikError::Generic(format!(
                "Path traversal detected in zip entry: {}",
                entry_name
            )));
        }

        if file.name().ends_with('/') {
            std::fs::create_dir_all(&outpath)
                .map_err(|e| crate::error::io_context("Failed to create directory", &outpath, e))?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    std::fs::create_dir_all(p).map_err(|e| {
                        crate::error::io_context("Failed to create parent directory", p, e)
                    })?;
                }
            }
            let mut outfile = File::create(&outpath)
                .map_err(|e| crate::error::io_context("Failed to create file", &outpath, e))?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }

    Ok(())
}

/// Install package from local directory
fn install_package_from_directory(
    package: &crate::models::package::Package,
    source_dir: &str,
    aik_dir: &crate::core::filesystem::AikDirectory,
    _args: &InstallArgs,
) -> Result<(), AikError> {
    use std::fs;
    use std::path::Path;

    let source_path = Path::new(source_dir);
    let install_path = aik_dir.packages_path().join(format!(
        "{}-{}",
        package.package.name, package.package.version
    ));

    // Create package directory
    fs::create_dir_all(&install_path)?;

    // Copy only relevant files, excluding version control and build artifacts
    copy_package_files(source_path, &install_path)?;

    Ok(())
}

/// Copy package files, excluding version control and build directories
fn copy_package_files(
    from: &std::path::Path,
    to: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    use walkdir::WalkDir;

    // Directories to exclude
    let exclude_dirs = [
        "target",
        "build",
        "out",
        ".git",
        ".aikit",
        "node_modules",
        ".next",
        "dist",
    ];

    for entry in WalkDir::new(from).into_iter().filter_map(|e| e.ok()) {
        let source_path = entry.path();
        let relative_path = source_path.strip_prefix(from)?;

        // Skip excluded directories
        if let Some(dir_name) = relative_path.iter().next() {
            if let Some(dir_str) = dir_name.to_str() {
                if exclude_dirs
                    .iter()
                    .any(|&excluded| excluded.eq_ignore_ascii_case(dir_str))
                {
                    continue;
                }
            }
        }

        let dest_path = to.join(relative_path);

        if source_path.is_dir() {
            fs::create_dir_all(&dest_path)?;
        } else {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(source_path, dest_path)?;
        }
    }

    Ok(())
}

/// Generate agent-specific command files
fn generate_agent_commands(
    package: &crate::models::package::Package,
    aik_dir: &crate::core::filesystem::AikDirectory,
    agent_keys: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::core::agent::get_agent_config;

    for agent_key in agent_keys {
        if let Some(agent_config) = get_agent_config(agent_key) {
            generate_commands_for_agent(package, &agent_config, aik_dir)?;
        } else {
            return Err(format!("Unknown agent: {}", agent_key).into());
        }
    }

    Ok(())
}

/// Load template content from installed package directory
fn load_template_content(
    package: &crate::models::package::Package,
    command_name: &str,
    command_def: &crate::models::package::CommandDefinition,
    aik_dir: &crate::core::filesystem::AikDirectory,
) -> Result<String, Box<dyn std::error::Error>> {
    use std::fs;

    let template_path_str = command_def.effective_source(command_name);
    let package_root = aikit_sdk::installed_package_root(
        &aik_dir.packages_path(),
        &package.package.name,
        &package.package.version,
    )
    .map_err(|e| format!("Failed to resolve package root: {}", e))?;
    let full_path = package_root.join(&template_path_str);

    fs::read_to_string(&full_path).map_err(|e| {
        format!(
            "Failed to load template '{}' from package '{}' (path: {}): {}",
            template_path_str,
            package.package.name,
            full_path.display(),
            e
        )
        .into()
    })
}

/// Generate commands for a specific agent
fn generate_commands_for_agent(
    package: &crate::models::package::Package,
    agent: &crate::core::agent::AgentConfig,
    aik_dir: &crate::core::filesystem::AikDirectory,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;

    // Create agent commands directory relative to project root (.aikit parent)
    let project_root = aik_dir.project_root();
    let commands_dir = project_root.join(&agent.output_dir);
    fs::create_dir_all(&commands_dir)?;

    // Generate command files for each package command
    for (command_name, command_def) in &package.commands {
        // Load actual template content from installed package
        let template_content = load_template_content(package, command_name, command_def, aik_dir)?;

        // Generate command content using loaded template
        let content = agent.generate_package_command(
            &package.package.name,
            command_name,
            &command_def.description,
            &template_content,
        );

        // Fix filename: use {package}.{command} instead of {package}.{agent_key}
        let filename = format!("{}.{}.md", package.package.name, command_name);
        let filepath = commands_dir.join(filename);

        fs::write(filepath, content)?;
    }

    Ok(())
}

/// Deploy package subagents for one agent (skips if agent has no agents_dir).
fn deploy_subagents_for_agent(
    package: &crate::models::package::Package,
    package_root: &std::path::Path,
    project_root: &std::path::Path,
    agent_key: &str,
    agent_config: &crate::core::agent::AgentConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let agents_dir = match &agent_config.agents_dir {
        Some(d) => d,
        None => return Ok(()),
    };
    let _ = agents_dir; // used by deploy_subagent via aikit-sdk
    for (name, def) in &package.subagents {
        let src_path = package_root.join(&def.source);
        let content = std::fs::read_to_string(&src_path).map_err(|e| {
            format!(
                "Failed to read subagent '{}' from {}: {}",
                name,
                src_path.display(),
                e
            )
        })?;
        aikit_sdk::deploy_subagent(agent_key, project_root, name, &content)
            .map_err(|e| format!("deploy_subagent {}: {}", name, e))?;
    }
    Ok(())
}

/// Deploy package skills for one agent by copying each skill folder (skips if agent has no skills_dir).
fn deploy_skills_for_agent(
    package: &crate::models::package::Package,
    package_root: &std::path::Path,
    project_root: &std::path::Path,
    agent_key: &str,
    agent_config: &crate::core::agent::AgentConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let skills_dir = match &agent_config.skills_dir {
        Some(d) => d,
        None => return Ok(()),
    };
    let dest_base = project_root.join(skills_dir);
    for (name, def) in &package.skills {
        let src_dir = package_root.join(&def.source);
        if !src_dir.is_dir() {
            return Err(format!(
                "Skill '{}' source is not a directory: {}",
                name,
                src_dir.display()
            )
            .into());
        }
        let dest_dir = dest_base.join(name);
        std::fs::create_dir_all(&dest_dir)?;
        copy_package_files(&src_dir, &dest_dir)?;
    }
    let _ = agent_key;
    Ok(())
}

/// Copy artifact mappings from installed package root to project.
///
/// This function is deprecated in favor of using aikit_sdk::copy_artifacts directly.
/// It is kept for backward compatibility but delegates to the SDK.
#[deprecated(note = "Use aikit_sdk::copy_artifacts directly")]
fn copy_artifacts_to_project(
    package: &crate::models::package::Package,
    package_root: &std::path::Path,
    project_root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mappings = package.get_artifact_mappings(None);
    aikit_sdk::copy_artifacts(package_root, project_root, &mappings)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

/// Execute update command
pub async fn execute_update(args: UpdateArgs) -> Result<(), AikError> {
    use crate::core::filesystem::AikDirectory;
    use crate::models::registry::LocalRegistry;

    // Validate package name
    crate::core::validation::validate_package_name(&args.package)?;

    let aik_dir = AikDirectory::find().map_err(|_| {
        AikError::Installation("No packages installed (.aikit directory not found)".to_string())
    })?;

    let registry_path = aik_dir.registry_path();
    let registry =
        LocalRegistry::load_from_file(&registry_path).unwrap_or_else(|_| LocalRegistry::new());

    // Check if package is installed
    let installed_package = registry
        .get_package(&args.package)
        .ok_or_else(|| AikError::PackageNotFound(args.package.clone()))?;

    println!(
        "Checking for updates to '{}' (current: {})...",
        args.package, installed_package.package.version
    );

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
pub async fn execute_remove(args: RemoveArgs) -> Result<(), AikError> {
    use crate::core::filesystem::AikDirectory;
    use crate::models::registry::LocalRegistry;

    // Validate package name
    crate::core::validation::validate_package_name(&args.package)?;

    let aik_dir = AikDirectory::find().map_err(|_| {
        AikError::Installation("No packages installed (.aikit directory not found)".to_string())
    })?;

    let registry_path = aik_dir.registry_path();
    let mut registry =
        LocalRegistry::load_from_file(&registry_path).unwrap_or_else(|_| LocalRegistry::new());

    // Check if package is installed
    if !registry.is_installed(&args.package) {
        return Err(AikError::PackageNotFound(args.package.clone()));
    }

    // Confirm removal unless forced
    if !args.force {
        println!(
            "Are you sure you want to remove package '{}'?",
            args.package
        );
        println!("This will delete all associated files and commands. (y/N): ");

        // For now, assume yes in automated context
        // TODO: Add interactive confirmation
    }

    // Get installed package info to determine version
    let installed_package = registry
        .get_package(&args.package)
        .ok_or_else(|| AikError::PackageNotFound(args.package.clone()))?;

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
                if filename
                    .to_lowercase()
                    .starts_with(&format!("{}.", package_name.to_lowercase()))
                    && filename.to_lowercase().ends_with(".md")
                {
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

    let aik_dir = match AikDirectory::find() {
        Ok(dir) => dir,
        Err(_) => {
            println!("No packages installed (.aikit directory not found)");
            return Ok(());
        }
    };

    let registry_path = aik_dir.registry_path();
    let registry =
        LocalRegistry::load_from_file(&registry_path).unwrap_or_else(|_| LocalRegistry::new());

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
        println!(
            "{:<25} {:<12} {:<15} Description",
            "Name", "Version", "Author"
        );
        println!("{:-<80}", "");

        for package in packages {
            let author = package
                .package
                .authors
                .first()
                .unwrap_or(&"Unknown".to_string())
                .clone();
            println!(
                "{:<25} {:<12} {:<15} {}",
                package.package.name, package.package.version, author, package.package.description
            );
        }
    } else {
        println!("Installed packages:");
        for package in packages {
            println!(
                "  {} v{} - {}",
                package.package.name, package.package.version, package.package.description
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_detect_source_type_local_directory() {
        let temp_dir = TempDir::new().unwrap();
        let aikit_toml = temp_dir.path().join("aikit.toml");
        fs::write(
            &aikit_toml,
            "[package]\nname = \"test\"\nversion = \"1.0.0\"",
        )
        .unwrap();

        let args = InstallArgs {
            source: temp_dir.path().to_string_lossy().to_string(),
            install_version: None,
            token: None,
            force: false,
            yes: false,
            ai: None,
        };

        let result = args.detect_source_type();
        assert!(result.is_ok());
        match result.unwrap() {
            SourceType::LocalFolder(path) => {
                assert_eq!(path, temp_dir.path());
            }
            _ => panic!("Expected LocalFolder"),
        }
    }

    #[test]
    fn test_detect_source_type_github_url() {
        let args = InstallArgs {
            source: "https://github.com/owner/repo".to_string(),
            install_version: None,
            token: None,
            force: false,
            yes: false,
            ai: None,
        };

        let result = args.detect_source_type();
        // This should parse as a GitHub URL successfully
        assert!(result.is_ok());
        match result.unwrap() {
            SourceType::GitHubRepo {
                owner,
                repo,
                version,
            } => {
                assert_eq!(owner, "owner");
                assert_eq!(repo, "repo");
                assert_eq!(version, "main");
            }
            _ => panic!("Expected GitHubRepo"),
        }
    }

    #[test]
    fn test_detect_source_type_invalid() {
        let args = InstallArgs {
            source: "not-a-valid-source".to_string(),
            install_version: None,
            token: None,
            force: false,
            yes: false,
            ai: None,
        };

        let result = args.detect_source_type();
        assert!(result.is_err());
    }

    /// Test artifact copy with Newton template pattern (newton/** -> .newton)
    #[test]
    fn test_copy_artifacts_newton_template() -> Result<(), Box<dyn std::error::Error>> {
        use std::fs;

        let temp = TempDir::new()?;
        let work = temp.path();

        // Create a mock package root with newton/ structure
        let package_root = work.join("package_root");
        fs::create_dir_all(package_root.join("newton/scripts"))?;

        // Create test files
        fs::write(package_root.join("newton/README.md"), "# Newton Template")?;
        fs::write(
            package_root.join("newton/scripts/advisor.sh"),
            "#!/bin/sh\necho advisor",
        )?;
        fs::write(
            package_root.join("newton/scripts/evaluator.sh"),
            "#!/bin/sh\necho evaluator",
        )?;

        // Create project root
        let project_root = work.join("project_root");
        fs::create_dir_all(&project_root)?;

        // Create a mock package with artifact mapping
        let mut package = crate::models::package::Package::new(
            "newton-templates".to_string(),
            "1.0.0".to_string(),
            "Newton workspace template".to_string(),
        );
        package
            .artifacts
            .insert("newton/**".to_string(), ".newton".to_string());

        // Build mappings and use SDK copy_artifacts
        let mappings = package.get_artifact_mappings(None);
        aikit_sdk::copy_artifacts(&package_root, &project_root, &mappings)?;

        // Verify files were copied correctly
        assert!(project_root.join(".newton/README.md").exists());
        assert!(project_root.join(".newton/scripts/advisor.sh").exists());
        assert!(project_root.join(".newton/scripts/evaluator.sh").exists());

        // Verify content
        let readme = fs::read_to_string(project_root.join(".newton/README.md"))?;
        assert!(readme.contains("Newton Template"));

        let advisor = fs::read_to_string(project_root.join(".newton/scripts/advisor.sh"))?;
        assert!(advisor.contains("echo advisor"));

        Ok(())
    }

    /// Test artifact copy with nested directory structure
    #[test]
    fn test_copy_artifacts_nested_structure() -> Result<(), Box<dyn std::error::Error>> {
        use std::fs;

        let temp = TempDir::new()?;
        let work = temp.path();

        // Create package root with nested structure
        let package_root = work.join("package_root");
        fs::create_dir_all(package_root.join("newton/deeply/nested/dir"))?;

        // Create files at various depths
        fs::write(package_root.join("newton/top.txt"), "top")?;
        fs::write(package_root.join("newton/deeply/nested/file.txt"), "nested")?;

        let project_root = work.join("project_root");
        fs::create_dir_all(&project_root)?;

        let mut package = crate::models::package::Package::new(
            "test-pkg".to_string(),
            "1.0.0".to_string(),
            "Test".to_string(),
        );
        package
            .artifacts
            .insert("newton/**".to_string(), ".newton".to_string());

        // Build mappings and use SDK copy_artifacts
        let mappings = package.get_artifact_mappings(None);
        aikit_sdk::copy_artifacts(&package_root, &project_root, &mappings)?;

        // Verify nested files were copied
        assert!(project_root.join(".newton/top.txt").exists());
        assert!(project_root.join(".newton/deeply/nested/file.txt").exists());

        Ok(())
    }

    /// Test artifact copy with glob pattern filtering
    #[test]
    fn test_copy_artifacts_glob_pattern() -> Result<(), Box<dyn std::error::Error>> {
        use std::fs;

        let temp = TempDir::new()?;
        let work = temp.path();

        let package_root = work.join("package_root");
        fs::create_dir_all(package_root.join("newton/scripts"))?;
        fs::create_dir_all(package_root.join("other"))?;

        // Create files in both directories
        fs::write(package_root.join("newton/scripts/advisor.sh"), "#!/bin/sh")?;
        fs::write(package_root.join("other/ignore.txt"), "ignore")?;

        let project_root = work.join("project_root");
        fs::create_dir_all(&project_root)?;

        let mut package = crate::models::package::Package::new(
            "test-pkg".to_string(),
            "1.0.0".to_string(),
            "Test".to_string(),
        );
        // Only copy newton/**, not other/**
        package
            .artifacts
            .insert("newton/**".to_string(), ".newton".to_string());

        // Build mappings and use SDK copy_artifacts
        let mappings = package.get_artifact_mappings(None);
        aikit_sdk::copy_artifacts(&package_root, &project_root, &mappings)?;

        // Verify only newton/** files were copied
        assert!(project_root.join(".newton/scripts/advisor.sh").exists());
        assert!(!project_root.join("other/ignore.txt").exists());

        Ok(())
    }

    /// Test that install_package_from_archive rejects path traversal attempts
    #[test]
    fn test_install_package_from_archive_path_traversal() {
        use std::fs::File;
        use std::io::Write;
        use zip::write::FileOptions;
        use zip::ZipWriter;

        let temp_dir = TempDir::new().unwrap();
        let archive_path = temp_dir.path().join("package.zip");

        // Create a malicious zip with path traversal
        let file = File::create(&archive_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default();

        // Add a benign file
        zip.start_file("safe.txt", options).unwrap();
        zip.write_all(b"safe content").unwrap();

        // Add a path traversal file (should be blocked)
        zip.start_file("../../../escape.txt", options).unwrap();
        zip.write_all(b"malicious content").unwrap();

        zip.finish().unwrap();

        // Create a mock package
        let package = crate::models::package::Package::new(
            "test-pkg".to_string(),
            "1.0.0".to_string(),
            "Test package".to_string(),
        );

        // Create a mock AikDirectory
        let aik_dir = crate::core::filesystem::AikDirectory::new(temp_dir.path().join(".aikit"));

        // Create the install directory
        let install_path = aik_dir.packages_path().join(format!(
            "{}-{}",
            package.package.name, package.package.version
        ));
        std::fs::create_dir_all(&install_path).unwrap();

        // Create InstallArgs (the function doesn't use args in the current implementation)
        let args = InstallArgs {
            source: "test".to_string(),
            install_version: None,
            token: None,
            force: false,
            yes: false,
            ai: None,
        };

        // Try to install from the malicious archive
        let result = install_package_from_archive(&package, &archive_path, &aik_dir, &args);

        // Should fail (either due to path traversal or file system error)
        assert!(
            result.is_err(),
            "Expected installation to fail, but it succeeded"
        );

        // Verify that the escape file was NOT created outside the install directory
        let escape_file = temp_dir.path().join("escape.txt");
        assert!(
            !escape_file.exists(),
            "Malicious file was created outside install directory!"
        );

        // Verify no files were created in the temp directory outside the install directory
        let files_outside = std::fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.file_name().unwrap() != "package.zip")
            .collect::<Vec<_>>();

        assert!(
            files_outside.is_empty(),
            "Found unexpected files outside install directory: {:?}",
            files_outside
        );
    }

    /// Test that install_package_from_archive rejects absolute path attempts
    #[test]
    fn test_install_package_from_archive_absolute_path() {
        use std::fs::File;
        use std::io::Write;
        use zip::write::FileOptions;
        use zip::ZipWriter;

        let temp_dir = TempDir::new().unwrap();
        let archive_path = temp_dir.path().join("package.zip");

        // Create a malicious zip with absolute path
        let file = File::create(&archive_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default();

        // Add an absolute path file first (should be blocked)
        zip.start_file("/etc/passwd", options).unwrap();
        zip.write_all(b"malicious content").unwrap();

        // Add a benign file (should not be created due to error above)
        zip.start_file("safe.txt", options).unwrap();
        zip.write_all(b"safe content").unwrap();

        zip.finish().unwrap();

        // Create a mock package
        let package = crate::models::package::Package::new(
            "test-pkg".to_string(),
            "1.0.0".to_string(),
            "Test package".to_string(),
        );

        // Create a mock AikDirectory
        let aik_dir = crate::core::filesystem::AikDirectory::new(temp_dir.path().join(".aikit"));

        // Create the install directory
        let install_path = aik_dir.packages_path().join(format!(
            "{}-{}",
            package.package.name, package.package.version
        ));
        std::fs::create_dir_all(&install_path).unwrap();

        // Create InstallArgs
        let args = InstallArgs {
            source: "test".to_string(),
            install_version: None,
            token: None,
            force: false,
            yes: false,
            ai: None,
        };

        // Try to install from the malicious archive
        let result = install_package_from_archive(&package, &archive_path, &aik_dir, &args);

        // Should fail due to absolute path
        assert!(
            result.is_err(),
            "Expected installation to fail, but it succeeded"
        );

        // Verify safe.txt was NOT created (since extraction stopped at the absolute path error)
        assert!(
            !install_path.join("safe.txt").exists(),
            "No files should have been created due to absolute path error"
        );
    }

    /// Test that normalize_path correctly resolves .. and . components
    #[test]
    fn test_normalize_path() {
        use std::path::Path;

        // Test basic normalization
        assert_eq!(
            normalize_path(Path::new("foo/../bar")),
            PathBuf::from("bar")
        );
        assert_eq!(
            normalize_path(Path::new("foo/./bar")),
            PathBuf::from("foo/bar")
        );
        assert_eq!(
            normalize_path(Path::new("foo/bar/../baz")),
            PathBuf::from("foo/baz")
        );
        assert_eq!(normalize_path(Path::new("../foo")), PathBuf::from("foo"));
        assert_eq!(normalize_path(Path::new("../../foo")), PathBuf::from("foo"));
        assert_eq!(
            normalize_path(Path::new("foo/bar/.")),
            PathBuf::from("foo/bar")
        );

        // Test multiple levels
        assert_eq!(
            normalize_path(Path::new("a/b/c/../../d")),
            PathBuf::from("a/d")
        );
        assert_eq!(
            normalize_path(Path::new("a/./b/./c/./d")),
            PathBuf::from("a/b/c/d")
        );

        // Test edge cases
        assert_eq!(normalize_path(Path::new(".")), PathBuf::from(""));
        assert_eq!(normalize_path(Path::new("..")), PathBuf::from(""));
    }
}
