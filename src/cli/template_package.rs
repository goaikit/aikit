//! `aikit package` command implementation
//!
//! This module implements the package generation command.

use crate::core::package::PackageConfig;
use anyhow::{Context, Result};
use clap::Args;

/// Build template zip archives for GitHub releases
#[derive(Args, Debug)]
pub struct PackageArgs {
    /// Version string with 'v' prefix (e.g., v1.0.0)
    #[arg(value_name = "VERSION")]
    pub version: String,

    /// Output directory for zip files
    #[arg(long, value_name = "DIR", default_value = ".genreleases")]
    pub output_dir: String,
}

/// Execute the package command
pub async fn execute(args: PackageArgs) -> Result<()> {
    // Parse filters from environment variables
    let agents = PackageConfig::parse_agents_env();
    let scripts = PackageConfig::parse_scripts_env();

    // Validate version format
    let config = PackageConfig {
        version: args.version.clone(),
        agents,
        scripts,
        output_dir: std::path::PathBuf::from(args.output_dir),
    };

    config.validate().map_err(|e| anyhow::anyhow!("{}", e))?;

    // Create output directory
    std::fs::create_dir_all(&config.output_dir).context(format!(
        "Failed to create output directory: {}",
        config.output_dir.display()
    ))?;

    // Determine source root (current directory)
    let source_root = std::env::current_dir()?;

    // Load command templates
    let templates = crate::core::package::load_command_templates(source_root.join("templates"))
        .context("Failed to load command templates")?;

    if templates.is_empty() {
        return Err(anyhow::anyhow!(
            "No command templates found in templates/commands/. Make sure you're running from the repository root."
        ));
    }

    // Get agents to process
    let agents: Vec<_> = if let Some(ref filter) = config.agents {
        crate::core::agent::get_agent_configs()
            .into_iter()
            .filter(|a| filter.contains(&a.key))
            .collect()
    } else {
        crate::core::agent::get_agent_configs()
    };

    // Get script variants to process
    let script_variants: Vec<crate::core::agent::ScriptVariant> =
        if let Some(ref filter) = config.scripts {
            filter.clone()
        } else {
            vec![
                crate::core::agent::ScriptVariant::Sh,
                crate::core::agent::ScriptVariant::Ps,
            ]
        };

    // Generate packages for each agent/script combination
    let mut generated = 0;
    for agent in &agents {
        for &script_variant in &script_variants {
            match crate::core::package::generate_package(
                &config,
                agent,
                script_variant,
                &templates,
                &source_root,
            ) {
                Ok(zip_path) => {
                    println!("Generated: {}", zip_path.display());
                    generated += 1;
                }
                Err(e) => {
                    eprintln!(
                        "Error generating package for {}/{:?}: {}",
                        agent.key, script_variant, e
                    );
                }
            }
        }
    }

    println!(
        "\nGenerated {} package(s) in {}",
        generated,
        config.output_dir.display()
    );

    Ok(())
}
