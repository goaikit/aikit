//! CLI command module
//!
//! This module contains all CLI command implementations.

mod check;
mod init;
mod package;
mod release;
mod version;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// AIKIT - Rust Spec Kit CLI Complete Reimplementation
#[derive(Parser)]
#[command(name = "aikit")]
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
    /// Display version information
    Version(version::VersionArgs),
    /// Build template zip archives for GitHub releases
    Package(package::PackageArgs),
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
        Commands::Version(args) => rt.block_on(version::execute(args))?,
        Commands::Package(args) => rt.block_on(package::execute(args))?,
        Commands::Release(args) => rt.block_on(release::execute(args))?,
    }

    Ok(())
}

/// Check if debug mode is enabled
pub fn is_debug() -> bool {
    std::env::var("AIKIT_DEBUG").is_ok()
}

/// Print debug message if debug mode is enabled
pub fn debug_print(msg: &str) {
    if is_debug() {
        eprintln!("[DEBUG] {}", msg);
    }
}
