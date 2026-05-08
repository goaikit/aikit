//! `aikit mcp` — merge MCP server entries into agent-specific JSON files.

use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};

use aikit_sdk::{
    add_mcp_server, mcp_supported_agents, normalize_mcp_agent_key, parse_env_pairs,
    parse_header_pairs, AddMcpServerOptions, McpScope, McpServerTransport,
};

#[derive(Parser, Debug)]
pub struct McpArgs {
    #[command(subcommand)]
    pub cmd: McpCommands,
}

#[derive(Subcommand, Debug)]
pub enum McpCommands {
    /// Agents that support `mcp add` and their config paths
    List,
    /// Add or replace one MCP server entry (JSON `mcpServers` / `servers` / `mcp`, or Codex TOML)
    Add(McpAddArgs),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum ScopeArg {
    Project,
    Global,
}

#[derive(Parser, Debug)]
pub struct McpAddArgs {
    /// Agent: `cursor-agent`|`cursor`, `claude`, `gemini`, `copilot`|`vscode`, `opencode`, `codex`
    #[arg(long)]
    pub agent: String,
    #[arg(long, value_enum)]
    pub scope: ScopeArg,
    /// Project root when `--scope project` (default: current directory)
    #[arg(long, default_value = ".")]
    pub project: std::path::PathBuf,
    /// MCP server id inside `mcpServers`
    #[arg(long)]
    pub name: String,
    /// Streamable HTTP / remote MCP URL (mutually exclusive with `--command`)
    #[arg(long)]
    pub url: Option<String>,
    /// Executable for stdio MCP (mutually exclusive with `--url`)
    #[arg(long)]
    pub command: Option<String>,
    /// One argv token for stdio MCP; repeat for each argument
    #[arg(long = "arg", action = clap::ArgAction::Append)]
    pub cmd_args: Vec<String>,
    /// `KEY=value` for stdio `env`; repeat per variable
    #[arg(long, action = clap::ArgAction::Append)]
    pub env: Vec<String>,
    /// `KEY=value` for HTTP `headers`; repeat per header
    #[arg(long, action = clap::ArgAction::Append)]
    pub header: Vec<String>,
    /// Replace an existing `mcpServers.<name>` entry
    #[arg(long)]
    pub overwrite: bool,
}

pub fn execute(args: McpArgs) -> Result<()> {
    match args.cmd {
        McpCommands::List => execute_list(),
        McpCommands::Add(a) => execute_add(a),
    }
}

fn execute_list() -> Result<()> {
    println!(
        "{:<14} {:<16} {:<28} GLOBAL_FILE",
        "AGENT_KEY", "DISPLAY", "PROJECT_FILE",
    );
    for row in mcp_supported_agents() {
        println!(
            "{:<14} {:<16} {:<28} {}",
            row.agent_key, row.display_name, row.project_config_path, row.global_config_path
        );
    }
    println!();
    println!(
        "Aliases: `--agent cursor` → `{}`; `--agent vscode` → `{}`.",
        normalize_mcp_agent_key("cursor"),
        normalize_mcp_agent_key("vscode")
    );
    println!(
        "The aikit catalog has {} agents; {} keys above are supported by `mcp add`.",
        aikit_sdk::all_agents().len(),
        aikit_sdk::MCP_SUPPORTED_AGENT_KEYS.len()
    );
    Ok(())
}

fn execute_add(args: McpAddArgs) -> Result<()> {
    let scope = match args.scope {
        ScopeArg::Project => McpScope::Project,
        ScopeArg::Global => McpScope::Global,
    };

    let project_root = if args.project.as_os_str().is_empty() {
        bail!("--project must not be empty");
    } else {
        std::fs::canonicalize(&args.project).unwrap_or_else(|_| args.project.clone())
    };

    let transport = match (&args.url, &args.command) {
        (Some(u), None) => {
            let headers = if args.header.is_empty() {
                None
            } else {
                Some(parse_header_pairs(&args.header).map_err(|e| anyhow::anyhow!("{}", e))?)
            };
            McpServerTransport::Http {
                url: u.clone(),
                headers,
            }
        }
        (None, Some(cmd)) => {
            let env = if args.env.is_empty() {
                None
            } else {
                Some(parse_env_pairs(&args.env).map_err(|e| anyhow::anyhow!("{}", e))?)
            };
            McpServerTransport::Stdio {
                command: cmd.clone(),
                args: args.cmd_args.clone(),
                env,
            }
        }
        (None, None) => bail!("specify either --url or --command"),
        (Some(_), Some(_)) => bail!("specify only one of --url or --command"),
    };

    let path = add_mcp_server(AddMcpServerOptions {
        agent_key: args.agent,
        scope,
        project_root,
        server_name: args.name,
        transport,
        overwrite: args.overwrite,
    })
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    println!("{}", path.display());
    Ok(())
}
