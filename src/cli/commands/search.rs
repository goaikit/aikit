//! Package search command
//!
//! This module contains the search command for discovering packages
//! from remote registries.

use clap::Args;
use crate::error::AikError;

/// Arguments for search command
#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Search query
    pub query: String,

    /// Maximum number of results (default: 20)
    #[arg(short, long, default_value = "20")]
    pub limit: usize,

    /// Show detailed information
    #[arg(long)]
    pub detailed: bool,

    /// Registry URL (default: GitHub)
    #[arg(long)]
    pub registry: Option<String>,
}

/// Execute search command
pub async fn execute_search(args: SearchArgs) -> Result<(), AikError> {
    use crate::core::git::GitHubClient;

    // Validate query
    if args.query.trim().is_empty() {
        return Err(AikError::InvalidSource("Search query cannot be empty".to_string()));
    }

    if args.query.len() > 100 {
        return Err(AikError::InvalidSource("Search query too long (max 100 characters)".to_string()));
    }

    println!("🔍 Searching for packages: '{}'", args.query);

    // Initialize GitHub client
    let github = GitHubClient::new(None);

    // Search GitHub repositories for packages
    let results = github
        .search_repositories(&args.query, args.limit)
        .await?;

    if results.is_empty() {
        println!("No packages found matching: {}", args.query);
        println!("💡 Try different keywords or check repository visibility");
        return Ok(());
    }

    // Filter results to only include repositories that might have packages
    // (This is a basic filter - in a full implementation, we'd check for aikit.toml)
    let package_repos: Vec<_> = results
        .into_iter()
        .filter(|repo| {
            // Basic heuristics for package repositories
            repo.description
                .as_ref()
                .map(|desc| {
                    desc.to_lowercase().contains("package")
                        || desc.to_lowercase().contains("aikit")
                        || desc.to_lowercase().contains("ai agent")
                })
                .unwrap_or(false)
                || repo.name.to_lowercase().contains("package")
                || repo.name.to_lowercase().contains("aikit")
        })
        .take(args.limit)
        .collect();

    if package_repos.is_empty() {
        println!("No AIKIT packages found matching: {}", args.query);
        println!("📦 Packages typically contain 'aikit.toml' and mention AI agents");
        return Ok(());
    }

    // Display results
    if args.detailed {
        display_detailed_results(&package_repos);
    } else {
        display_compact_results(&package_repos);
    }

    println!("\n💡 Install any package with: aikit install <owner>/<repo>");
    Ok(())
}

/// Display search results in compact format
fn display_compact_results(repos: &[crate::core::git::RepositoryInfo]) {
    println!("\n📦 Found {} packages:", repos.len());
    println!("{}", "─".repeat(80));

    for repo in repos {
        let stars = if repo.stargazers_count > 0 {
            format!("⭐ {}", repo.stargazers_count)
        } else {
            "".to_string()
        };

        let description = repo
            .description
            .as_ref()
            .map(|d| {
                if d.len() > 60 {
                    format!("{}...", &d[..57])
                } else {
                    d.clone()
                }
            })
            .unwrap_or_else(|| "No description".to_string());

        println!(
            "{:<30} {:<15} {}",
            format!("{}/{}", repo.owner.login, repo.name),
            stars,
            description
        );
    }
}

/// Display search results in detailed format
fn display_detailed_results(repos: &[crate::core::git::RepositoryInfo]) {
    println!("\n📦 Package Search Results:");
    println!("{}", "═".repeat(80));

    for (i, repo) in repos.iter().enumerate() {
        println!("{}. {} ({})", i + 1, repo.full_name, repo.html_url);
        println!(
            "   Description: {}",
            repo.description
                .as_ref()
                .unwrap_or(&"No description".to_string())
        );

        if repo.stargazers_count > 0 {
            println!("   ⭐ Stars: {}", repo.stargazers_count);
        }

        println!("   📅 Updated: {}", format_date(&repo.updated_at));
        println!("   🏗️  Install: aikit install {}", repo.full_name);
        println!();
    }
}

/// Format date string for display
fn format_date(date_str: &str) -> String {
    // Basic date formatting - in a real implementation, you'd parse and format properly
    date_str.split('T').next().unwrap_or(date_str).to_string()
}
