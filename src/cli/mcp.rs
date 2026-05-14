//! `aikit mcp` — merge MCP server entries into agent-specific JSON files.

use anyhow::{bail, Result};

use aikit_sdk::{
    add_mcp_server, mcp_supported_agents, normalize_mcp_agent_key, parse_env_pairs,
    parse_header_pairs, AddMcpServerOptions, McpScope, McpServerTransport,
};

#[derive(Debug, Default)]
pub struct McpAddArgs {
    pub agent: String,
    pub scope: String,
    pub project: std::path::PathBuf,
    pub name: String,
    pub url: Option<String>,
    pub command: Option<String>,
    pub cmd_args: Vec<String>,
    pub env: Vec<String>,
    pub header: Vec<String>,
    pub overwrite: bool,
}

pub fn execute_list() -> Result<()> {
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

pub fn execute_add(args: McpAddArgs) -> Result<()> {
    let scope = match args.scope.as_str() {
        "global" => McpScope::Global,
        _ => McpScope::Project,
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
