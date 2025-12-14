//! CLI configuration and setup
//!
//! This module handles CLI-specific configuration and initialization.

use clap::{ArgMatches, Command};

/// Build the main CLI command structure
pub fn build_cli() -> Command {
    Command::new("aikit")
        .version(env!("CARGO_PKG_VERSION"))
        .author("AIKIT Team")
        .about("Universal package manager for AI agent extensions")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("package")
                .about("Package management commands")
                .subcommand_required(true)
                .subcommand(
                    Command::new("init")
                        .about("Initialize a new package")
                        .arg(
                            clap::Arg::new("name")
                                .help("Package name")
                                .required(true)
                        )
                        .arg(
                            clap::Arg::new("description")
                                .long("description")
                                .short('d')
                                .help("Package description")
                        )
                        .arg(
                            clap::Arg::new("version")
                                .long("version")
                                .short('v')
                                .default_value("0.1.0")
                                .help("Package version")
                        )
                        .arg(
                            clap::Arg::new("author")
                                .long("author")
                                .short('a')
                                .help("Author name")
                        )
                        .arg(
                            clap::Arg::new("yes")
                                .long("yes")
                                .short('y')
                                .help("Skip interactive prompts")
                                .action(clap::ArgAction::SetTrue)
                        )
                )
                .subcommand(
                    Command::new("build")
                        .about("Build package for distribution")
                        .arg(
                            clap::Arg::new("output")
                                .long("output")
                                .short('o')
                                .default_value("dist")
                                .help("Output directory")
                        )
                        .arg(
                            clap::Arg::new("agents")
                                .long("agents")
                                .help("Target agents (comma-separated)")
                        )
                        .arg(
                            clap::Arg::new("include-sources")
                                .long("include-sources")
                                .help("Include source files")
                                .action(clap::ArgAction::SetTrue)
                        )
                )
                .subcommand(
                    Command::new("publish")
                        .about("Publish package to registry")
                        .arg(
                            clap::Arg::new("registry")
                                .long("registry")
                                .help("Registry URL")
                        )
                        .arg(
                            clap::Arg::new("repo")
                                .long("repo")
                                .short('r')
                                .help("Repository URL")
                        )
                        .arg(
                            clap::Arg::new("release")
                                .long("release")
                                .help("Create GitHub release")
                                .action(clap::ArgAction::SetTrue)
                        )
                )
        )
        .subcommand(
            Command::new("install")
                .about("Install package from URL")
                .arg(
                    clap::Arg::new("source")
                        .help("Package source (GitHub URL or package name)")
                        .required(true)
                )
                .arg(
                    clap::Arg::new("version")
                        .long("version")
                        .short('v')
                        .help("Specific version to install")
                )
                .arg(
                    clap::Arg::new("force")
                        .long("force")
                        .short('f')
                        .help("Force reinstall if already installed")
                        .action(clap::ArgAction::SetTrue)
                )
                .arg(
                    clap::Arg::new("yes")
                        .long("yes")
                        .short('y')
                        .help("Skip .gitignore modification prompt")
                        .action(clap::ArgAction::SetTrue)
                )
        )
        .subcommand(
            Command::new("update")
                .about("Update installed package")
                .arg(
                    clap::Arg::new("package")
                        .help("Package name to update")
                        .required(true)
                )
                .arg(
                    clap::Arg::new("breaking")
                        .long("breaking")
                        .help("Allow breaking changes")
                        .action(clap::ArgAction::SetTrue)
                )
        )
        .subcommand(
            Command::new("remove")
                .about("Remove installed package")
                .arg(
                    clap::Arg::new("package")
                        .help("Package name to remove")
                        .required(true)
                )
                .arg(
                    clap::Arg::new("force")
                        .long("force")
                        .short('f')
                        .help("Force removal without confirmation")
                        .action(clap::ArgAction::SetTrue)
                )
        )
        .subcommand(
            Command::new("list")
                .about("List installed packages")
                .arg(
                    clap::Arg::new("author")
                        .long("author")
                        .short('a')
                        .help("Filter by author")
                )
                .arg(
                    clap::Arg::new("detailed")
                        .long("detailed")
                        .short('d')
                        .help("Show detailed information")
                        .action(clap::ArgAction::SetTrue)
                )
        )
        .subcommand(
            Command::new("search")
                .about("Search for packages")
                .arg(
                    clap::Arg::new("query")
                        .help("Search query")
                        .required(true)
                )
                .arg(
                    clap::Arg::new("limit")
                        .long("limit")
                        .short('l')
                        .default_value("20")
                        .help("Maximum number of results")
                )
                .arg(
                    clap::Arg::new("detailed")
                        .long("detailed")
                        .short('d')
                        .help("Show detailed information")
                        .action(clap::ArgAction::SetTrue)
                )
                .arg(
                    clap::Arg::new("registry")
                        .long("registry")
                        .help("Registry URL")
                )
        )
        .subcommand(
            Command::new("init")
                .about("Initialize AIKIT project for an AI agent")
                .arg(
                    clap::Arg::new("ai")
                        .long("ai")
                        .short('a')
                        .help("AI agent to initialize for")
                        .value_parser(["claude", "cursor", "copilot", "gemini", "continue"])
                )
                .arg(
                    clap::Arg::new("force")
                        .long("force")
                        .short('f')
                        .help("Overwrite existing configuration")
                        .action(clap::ArgAction::SetTrue)
                )
        )
        .subcommand(
            Command::new("check")
                .about("Check AIKIT installation and configuration")
        )
}

/// Parse CLI arguments and return matches
pub fn parse_args() -> ArgMatches {
    build_cli().get_matches()
}

/// Get help text for a specific command
pub fn get_command_help(command: &str) -> String {
    match build_cli().find_subcommand(command) {
        Some(cmd) => {
            let mut help = Vec::new();
            cmd.write_help(&mut help).unwrap();
            String::from_utf8(help).unwrap()
        }
        None => format!("Unknown command: {}", command),
    }
}
