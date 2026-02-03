//! CLI command module
//!
//! This module contains all CLI command implementations.

mod check;
mod init;
mod release;
mod template_package; // Old template zip archive builder (used by release command)
mod version;

// Package management commands (init, build, publish)
pub mod commands {
    pub mod install;
    pub mod package;
}

use anyhow::Result;
use clap::{Parser, Subcommand};

/// AIKIT - Universal template package manager for AI agents
#[derive(Parser)]
#[command(
    name = "aikit",
    about = "AIKit - Universal template package manager for AI agents",
    long_about = None,
    version = env!("CARGO_PKG_VERSION"),
    arg_required_else_help = true
)]
pub struct Cli {
    /// Enable debug output (verbose diagnostic information)
    #[arg(long, global = true)]
    pub debug: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
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
    /// Package management commands (init, build, publish)
    #[command(subcommand)]
    Package(commands::package::PackageCommands),
    /// Create GitHub release with package files
    Release(release::ReleaseArgs),
}

/// Run the CLI application
pub fn run() -> Result<()> {
    let cli = Cli::parse();

    // Initialization banner (binary name and version)
    eprintln!("aikit {}", env!("CARGO_PKG_VERSION"));

    // Set debug mode if enabled
    if cli.debug {
        std::env::set_var("AIKIT_DEBUG", "1");
        eprintln!("[DEBUG] Debug mode enabled");
    }

    // Runtime for async operations
    let rt = tokio::runtime::Runtime::new()?;

    match cli.command {
        Some(Commands::Init(args)) => rt.block_on(init::execute(args))?,
        Some(Commands::Check(args)) => check::execute(args)?,
        Some(Commands::Install(args)) => rt
            .block_on(commands::install::execute_install(args))
            .map_err(|e| anyhow::anyhow!("{}", e))?,
        Some(Commands::Update(args)) => rt
            .block_on(commands::install::execute_update(args))
            .map_err(|e| anyhow::anyhow!("{}", e))?,
        Some(Commands::Remove(args)) => rt
            .block_on(commands::install::execute_remove(args))
            .map_err(|e| anyhow::anyhow!("{}", e))?,
        Some(Commands::List(args)) => rt
            .block_on(commands::install::execute_list(args))
            .map_err(|e| anyhow::anyhow!("{}", e))?,
        Some(Commands::Package(cmd)) => match cmd {
            commands::package::PackageCommands::Init(args) => rt
                .block_on(commands::package::execute_init(args))
                .map_err(|e| anyhow::anyhow!("{}", e))?,
            commands::package::PackageCommands::Validate(args) => rt
                .block_on(commands::package::execute_validate(args))
                .map_err(|e| anyhow::anyhow!("{}", e))?,
            commands::package::PackageCommands::Build(args) => rt
                .block_on(commands::package::execute_build(args))
                .map_err(|e| anyhow::anyhow!("{}", e))?,
            commands::package::PackageCommands::Publish(args) => rt
                .block_on(commands::package::execute_publish(args))
                .map_err(|e| anyhow::anyhow!("{}", e))?,
        },
        Some(Commands::Release(args)) => rt.block_on(release::execute(args))?,
        // This should never be reached due to arg_required_else_help = true
        None => unreachable!("arg_required_else_help should prevent None commands"),
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
