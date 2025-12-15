//! AIKIT - Rust Spec Kit CLI Complete Reimplementation
//!
//! This is a complete Rust reimplementation of the GitHub Spec Kit CLI tool,
//! providing behaviorally identical functionality to the Python-based `specify` command.

#![allow(dead_code)]

mod cli;
mod config;
mod core;
mod fs;
mod git;
mod github;
mod models;
mod tui;

/// Main entry point for the AIKIT CLI
fn main() {
    // Load environment variables from .env file if it exists
    let _ = dotenv::dotenv();

    // Initialize error handling
    if let Err(e) = cli::run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
