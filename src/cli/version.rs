//! `aikit version` command implementation
//!
//! This module implements the version information command.

use crate::github::api::GitHubClient;
use crate::tui::output::{format_panel, format_table};
use anyhow::Result;
use clap::Args;
use std::env;

/// Display version information
#[derive(Args, Debug)]
pub struct VersionArgs {
    /// GitHub token for API requests (optional)
    #[arg(long, value_name = "TOKEN")]
    pub github_token: Option<String>,
}

/// Execute the version command
pub async fn execute(args: VersionArgs) -> Result<()> {
    // Get CLI version from Cargo.toml
    let cli_version = env!("CARGO_PKG_VERSION");

    // Get system information
    let os = env::consts::OS;
    let arch = env::consts::ARCH;
    let rust_version = get_rust_version();

    // Try to get template version from GitHub
    let template_version = match get_template_version(args.github_token).await {
        Ok(version) => version,
        Err(_) => "unknown".to_string(),
    };

    // Build version table
    let headers = vec!["Component", "Version"];
    let rows = vec![
        vec!["CLI".to_string(), cli_version.to_string()],
        vec!["Template".to_string(), template_version],
        vec!["OS".to_string(), os.to_string()],
        vec!["Architecture".to_string(), arch.to_string()],
        vec!["Rust".to_string(), rust_version.to_string()],
    ];

    let table = format_table(&headers, &rows);
    let panel = format_panel("AIKIT Version Information", &table);
    println!("{}", panel);

    Ok(())
}

async fn get_template_version(github_token: Option<String>) -> Result<String> {
    let token = GitHubClient::resolve_token(github_token);
    let client = GitHubClient::new(token)?;

    // TODO: Get from actual spec-kit repository
    // For now, return a placeholder
    let release = client.get_latest_release("aroff", "spec-kit").await?;
    let tag_name = release["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No tag_name in release"))?;

    Ok(tag_name.to_string())
}

fn get_rust_version() -> String {
    // Try to get from rustc, fallback to "unknown"
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string())
}
