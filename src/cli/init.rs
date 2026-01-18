//! `aikit init` command implementation
//!
//! This module implements the project initialization command.

use crate::core::agent::{get_agent_config, ScriptVariant};
use crate::core::template::ProjectPath;
use crate::fs::permissions;
use crate::git;
use crate::github::api::GitHubClient;
use anyhow::{Context, Result};
use clap::Args;

/// Initialize a new Spec-Driven Development project
#[derive(Args, Debug)]
pub struct InitArgs {
    /// Project name (directory to create). Use '.' for current directory.
    #[arg(value_name = "PROJECT_NAME")]
    pub project_name: Option<String>,

    /// AI assistant to use (e.g., claude, gemini, copilot)
    #[arg(long, value_name = "AGENT")]
    pub ai: Option<String>,

    /// Script type (sh or ps)
    #[arg(long, value_name = "TYPE")]
    pub script: Option<String>,

    /// Initialize in current directory
    #[arg(long)]
    pub here: bool,

    /// Skip confirmation when merging into non-empty directory
    #[arg(long)]
    pub force: bool,

    /// Skip Git repository initialization
    #[arg(long)]
    pub no_git: bool,

    /// GitHub personal access token for API requests
    #[arg(long, value_name = "TOKEN")]
    pub github_token: Option<String>,

    /// Skip TLS certificate verification (not recommended)
    #[arg(long)]
    pub skip_tls: bool,

    /// Enable verbose diagnostic output
    #[arg(long)]
    pub debug: bool,

    /// Skip CLI tool validation for selected agent
    #[arg(long)]
    pub ignore_agent_tools: bool,
}

/// Execute the init command
pub async fn execute(args: InitArgs) -> Result<()> {
    // Resolve project path
    let is_here = args.here || args.project_name.as_deref() == Some(".");
    let project_path = if is_here {
        ProjectPath::new(std::env::current_dir()?)
    } else {
        let project_name = args
            .project_name
            .ok_or_else(|| anyhow::anyhow!("PROJECT_NAME is required unless --here is used"))?;
        ProjectPath::new(project_name.into())
    };

    // Check for non-empty directory and prompt if needed
    let path_is_empty = !project_path.path.exists()
        || std::fs::read_dir(&project_path.path)
            .map(|mut dir| dir.next().is_none())
            .unwrap_or(true);
    if is_here && !path_is_empty && !args.force {
        // Prompt for confirmation
        eprint!("Directory is not empty. Files will be merged. Continue? [y/N]: ");
        use std::io::{self, Write};
        io::stdout().flush()?;
        let mut response = String::new();
        io::stdin().read_line(&mut response)?;
        if !response.trim().eq_ignore_ascii_case("y")
            && !response.trim().eq_ignore_ascii_case("yes")
        {
            println!("Cancelled.");
            return Ok(());
        }
    }

    // Validate project path
    if !args.force && project_path.path.exists() && project_path.path.is_file() {
        return Err(anyhow::anyhow!(
            "Path '{}' exists and is a file, not a directory",
            project_path.path.display()
        ));
    }

    // Resolve agent selection
    let agent_key = if let Some(ai) = args.ai {
        crate::core::agent::validate_agent_key(&ai).map_err(|e| anyhow::anyhow!("{}", e))?;
        ai
    } else {
        // Check if stdin is a TTY for interactive selection
        if atty::is(atty::Stream::Stdin) {
            match crate::tui::agent_select::select_agent_interactive() {
                Ok(crate::tui::agent_select::SelectionResult::Selected(key)) => key,
                Ok(crate::tui::agent_select::SelectionResult::Cancelled) => {
                    println!("Selection cancelled.");
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("Error during interactive selection: {}", e);
                    eprintln!("Falling back to default agent: copilot");
                    "copilot".to_string()
                }
            }
        } else {
            // Non-interactive: default to copilot
            "copilot".to_string()
        }
    };

    let agent_config = get_agent_config(&agent_key)
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found", agent_key))?;

    // Resolve script variant
    let script_variant = if let Some(script) = args.script {
        match script.as_str() {
            "sh" => ScriptVariant::Sh,
            "ps" => ScriptVariant::Ps,
            _ => {
                return Err(anyhow::anyhow!(
                    "Invalid script type '{}'. Must be 'sh' or 'ps'",
                    script
                ))
            }
        }
    } else {
        ScriptVariant::default_for_platform()
    };

    // Check agent tools if required
    if !args.ignore_agent_tools && agent_config.requires_cli {
        if let Err(_e) = crate::core::tools::check_agent_tool(&agent_config) {
            return Err(anyhow::anyhow!(
                "Agent '{}' requires CLI tool '{}' but it was not found.\n\
                Install it or use --ignore-agent-tools to skip this check.\n\
                Install URL: {}",
                agent_config.name,
                agent_config.key,
                agent_config
                    .install_url
                    .as_deref()
                    .unwrap_or("See agent documentation")
            ));
        }
    }

    // Create project directory if needed
    if !is_here && !project_path.path.exists() {
        std::fs::create_dir_all(&project_path.path).with_context(|| {
            format!("Failed to create directory {}", project_path.path.display())
        })?;
    }

    // Initialize GitHub client
    let token = GitHubClient::resolve_token(args.github_token);
    let github_client = GitHubClient::with_skip_tls(token, args.skip_tls)?;

    // Download template
    let release = github_client
        .get_latest_release("aroff", "spec-kit")
        .await
        .context("Failed to fetch latest release from GitHub")?;

    let assets = release["assets"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Release missing 'assets' array"))?;

    // Convert assets to Vec<String> (URLs)
    let asset_urls: Vec<String> = assets
        .iter()
        .filter_map(|asset| {
            asset["browser_download_url"]
                .as_str()
                .map(|s| s.to_string())
        })
        .collect();

    // Convert ScriptVariant to string
    let script_variant_str = match script_variant {
        ScriptVariant::Sh => "sh",
        ScriptVariant::Ps => "ps",
    };

    let template_url =
        crate::core::template::select_template_asset(&asset_urls, &agent_key, script_variant_str)
            .ok_or_else(|| anyhow::anyhow!("Failed to find matching template asset"))?;

    // Download the template zip
    let zip_data = github_client
        .download_file(&template_url)
        .await
        .context("Failed to download template")?;

    // Extract and flatten ZIP to target directory
    // If --here and directory not empty, merge files instead of overwriting
    let path_is_empty = !project_path.path.exists()
        || std::fs::read_dir(&project_path.path)
            .map(|mut dir| dir.next().is_none())
            .unwrap_or(true);
    if is_here && !path_is_empty {
        // Extract to temp first, then merge
        let temp_dir = tempfile::tempdir()?;
        crate::core::template::extract_and_flatten_zip(&zip_data, temp_dir.path())
            .map_err(|e| anyhow::anyhow!("Failed to extract template to temp: {}", e))?;

        // Merge files from temp to target
        merge_directory_contents(temp_dir.path(), &project_path.path)
            .context("Failed to merge template files")?;
    } else {
        // Direct extraction for new directories
        crate::core::template::extract_and_flatten_zip(&zip_data, &project_path.path)
            .map_err(|e| anyhow::anyhow!("Failed to extract template: {}", e))?;
    }

    // Create agent-specific command file directory
    let agent_dir = project_path.path.join(&agent_config.output_dir);
    crate::fs::ensure_directory(&agent_dir).context("Failed to create agent directory")?;

    // Set script permissions on .sh files
    if let Err(e) = set_script_permissions_recursive(&project_path.path) {
        eprintln!("Warning: Failed to set some script permissions: {}", e);
        // Non-fatal, continue
    }

    // Initialize Git if requested
    if !args.no_git && !git::is_git_repo(&project_path.path) {
        if let Err(e) = git::init_git_repo(&project_path.path) {
            eprintln!("Warning: Failed to initialize git repository: {}", e);
            // Non-fatal, continue
        } else {
            // Create initial commit
            if let Err(e) = git::create_initial_commit(&project_path.path) {
                eprintln!("Warning: Failed to create initial commit: {}", e);
                // Non-fatal, continue
            }
        }
    }

    // Display success message
    println!(
        "✓ Project initialized successfully at {}",
        project_path.path.display()
    );
    println!("  Agent: {}", agent_config.name);
    println!("  Script type: {:?}", script_variant);

    // Display Codex setup instructions if needed
    if agent_key == "codex" {
        println!("\nNote: Codex requires CODEX_HOME environment variable to be set.");
        println!("  export CODEX_HOME=/path/to/codex");
    }

    // Display security notice
    println!("\n⚠️  Security Notice:");
    println!(
        "  Consider adding '{}' to .gitignore if it contains sensitive information.",
        agent_config.folder
    );

    Ok(())
}

/// Set script permissions recursively
fn set_script_permissions_recursive<P: AsRef<std::path::Path>>(path: P) -> Result<()> {
    use walkdir::WalkDir;

    for entry in WalkDir::new(path) {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase())
            == Some("sh".to_string())
        {
            permissions::set_script_permissions(entry.path())?;
        }
    }

    Ok(())
}

/// Merge directory contents, handling file conflicts
fn merge_directory_contents<P: AsRef<std::path::Path>, Q: AsRef<std::path::Path>>(
    from: P,
    to: Q,
) -> Result<()> {
    use crate::fs::merge;
    use walkdir::WalkDir;

    let from = from.as_ref();
    let to = to.as_ref();

    for entry in WalkDir::new(from) {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(from)?;
        let dest = to.join(relative);

        if path.is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }

            // Handle special files that need merging
            if dest.exists() {
                // Check if it's a JSON file that should be merged
                if dest
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_lowercase())
                    == Some("json".to_string())
                {
                    let new_content: serde_json::Value =
                        serde_json::from_str(&std::fs::read_to_string(path)?)?;
                    merge::merge_json_file(&dest, &new_content)?;
                } else {
                    // For other files, skip (don't overwrite existing)
                    // This matches Python behavior for --here
                }
            } else {
                // File doesn't exist, copy it
                std::fs::copy(path, &dest)?;
            }
        }
    }

    Ok(())
}
