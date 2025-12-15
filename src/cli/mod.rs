//! CLI command module
//!
//! This module contains all CLI command implementations.

mod check;
mod init;
mod release;
mod template_package; // Old template zip archive builder (used by release command)

// Package management commands (init, build, publish)
mod commands {
    pub mod install;
    pub mod package;
    pub mod search;
}

use anyhow::Result;
use clap::{Parser, Subcommand};

/// AIKIT - Rust Spec Kit CLI Complete Reimplementation
#[derive(Parser)]
#[command(name = "aikit", version = env!("CARGO_PKG_VERSION"), disable_version_flag = true)]
#[command(about = "AIKIT - Rust Spec Kit CLI", long_about = None)]
pub struct Cli {
    /// Enable debug output (verbose diagnostic information)
    #[arg(long, global = true)]
    pub debug: bool,


    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new Spec-Driven Development project
    Init(init::InitArgs),
    /// Check installed tools and AI agent CLIs
    Check(check::CheckArgs),
    /// Install package from GitHub URL
    Install(commands::install::InstallArgs),
    /// Update installed package
    Update(commands::install::UpdateArgs),
    /// Remove installed package
    Remove(commands::install::RemoveArgs),
    /// List installed packages
    List(commands::install::ListArgs),
    /// Search for packages in registries
    Search(commands::search::SearchArgs),
    /// Package management commands (init, build, publish)
    #[command(subcommand)]
    Package(commands::package::PackageCommands),
    /// Create GitHub release with package files
    Release(release::ReleaseArgs),
}

/// Run the CLI application
pub fn run() -> Result<()> {
    let cli = Cli::parse();

    // Set debug mode if enabled
    if cli.debug {
        std::env::set_var("AIKIT_DEBUG", "1");
        eprintln!("[DEBUG] Debug mode enabled");
    }

    // Runtime for async operations
    let rt = tokio::runtime::Runtime::new()?;

    match cli.command {
        Commands::Init(args) => rt.block_on(init::execute(args))?,
        Commands::Check(args) => check::execute(args)?,
        Commands::Install(args) => rt
            .block_on(commands::install::execute_install(args))
            .map_err(|e| anyhow::anyhow!("{}", e))?,
        Commands::Update(args) => rt
            .block_on(commands::install::execute_update(args))
            .map_err(|e| anyhow::anyhow!("{}", e))?,
        Commands::Remove(args) => rt
            .block_on(commands::install::execute_remove(args))
            .map_err(|e| anyhow::anyhow!("{}", e))?,
        Commands::List(args) => rt
            .block_on(commands::install::execute_list(args))
            .map_err(|e| anyhow::anyhow!("{}", e))?,
        Commands::Search(args) => rt
            .block_on(commands::search::execute_search(args))
            .map_err(|e| anyhow::anyhow!("{}", e))?,
        Commands::Package(cmd) => match cmd {
            commands::package::PackageCommands::Init(args) => rt
                .block_on(commands::package::execute_init(args))
                .map_err(|e| anyhow::anyhow!("{}", e))?,
            commands::package::PackageCommands::Build(args) => rt
                .block_on(commands::package::execute_build(args))
                .map_err(|e| anyhow::anyhow!("{}", e))?,
            commands::package::PackageCommands::Publish(args) => rt
                .block_on(commands::package::execute_publish(args))
                .map_err(|e| anyhow::anyhow!("{}", e))?,
        },
        Commands::Release(args) => rt.block_on(release::execute(args))?,
    }

    Ok(())
}

/// Check if debug mode is enabled
#[allow(dead_code)]
pub fn is_debug() -> bool {
    std::env::var("AIKIT_DEBUG").is_ok()
}

/// Print debug message if debug mode is enabled
#[allow(dead_code)]
pub fn debug_print(msg: &str) {
    if is_debug() {
        eprintln!("[DEBUG] {}", msg);
    }
}
