# aikit-sdk

`aikit-sdk` is the Rust gateway used by the `aikit` CLI and available for direct integration.
It provides deterministic APIs for:

- agent catalog and capability lookup
- path and instruction-file resolution
- file deployment (commands, skills, subagents)
- package/template install helpers
- runnable-agent detection
- buffered and streaming agent execution
- MCP server registration (merge into agent JSON) for supported assistants

## Install

```toml
[dependencies]
aikit-sdk = "0.2.1"
```

Or from this workspace:

```toml
[dependencies]
aikit-sdk = { path = "../aikit-sdk" }
```

## Quick start

```rust
use aikit_sdk::{all_agents, validate_agent_key, commands_dir};
use std::path::Path;

let _agents = all_agents();
validate_agent_key("claude")?;
let cmd_dir = commands_dir(Path::new("."), "claude")?;
println!("{}", cmd_dir.display());
# Ok::<(), aikit_sdk::DeployError>(())
```

## Deploy content

```rust
use aikit_sdk::{deploy_command, deploy_skill, deploy_subagent};
use std::path::Path;

let root = Path::new(".");
deploy_command("claude", root, "lint", "# command body")?;
deploy_skill("cursor-agent", root, "my-skill", "# SKILL.md", None)?;
deploy_subagent("claude", root, "reviewer", "# subagent")?;
# Ok::<(), aikit_sdk::DeployError>(())
```

## MCP config merge

Supported keys: `cursor-agent`, `claude`, `gemini`, `copilot`, `opencode`, `codex` (see `mcp_supported_agents()` and `MCP_SUPPORTED_AGENT_KEYS`). Each uses the on-disk format expected by that product (`mcpServers`, VS Code `servers`, OpenCode `mcp`, or Codex TOML tables). On **Windows**, VS Code user MCP and OpenCode global paths fall back to `<home>\\AppData\\Roaming\\...` when `%APPDATA%` / `config_dir` are missing.

```rust
use aikit_sdk::{
    add_mcp_server, mcp_config_path, AddMcpServerOptions, McpScope, McpServerTransport,
};
use std::path::Path;

let path = add_mcp_server(AddMcpServerOptions {
    agent_key: "gemini".into(),
    scope: McpScope::Project,
    project_root: Path::new(".").to_path_buf(),
    server_name: "my-cli".into(),
    transport: McpServerTransport::Http {
        url: "http://127.0.0.1:8730/mcp".into(),
        headers: None,
    },
    overwrite: false,
})?;
println!("{}", path.display());
# Ok::<(), aikit_sdk::McpDeployError>(())
```

## Instruction files

For agent guidance files (`AGENTS.md`, `CLAUDE.md`, `GEMINI.md`):

- `instruction_file(...)`
- `resolve_instruction_file(...)`
- `instruction_file_with_override(...)`
- `instruction_file_agents()`

These helpers provide deterministic paths and fallback behavior per agent.

## Run agents

Runnable keys: `codex`, `claude`, `gemini`, `opencode`, `agent`.

```rust
use aikit_sdk::{run_agent, RunOptions};

let result = run_agent(
    "claude",
    "Summarize the architecture",
    RunOptions::default().with_stream(false),
)?;

println!("exit={:?}", result.exit_code());
# Ok::<(), aikit_sdk::RunError>(())
```

For incremental output, use `run_agent_events(...)`. Event payloads include normalized stream messages and raw transport lines where applicable.

## Agent availability

- `is_agent_available(key)`
- `get_installed_agents()`
- `get_agent_status()`
- `is_runnable(key)` and `runnable_agents()`

## Test

From workspace root:

```bash
cargo test -p aikit-sdk
```

Some tests are marked ignored (for manual Windows/real-agent scenarios):

```bash
cargo test -p aikit-sdk -- --ignored
```

## Related docs

- Workspace overview: `../README.md`
- Python bindings: `../aikit-py/README.md`
- Contributor guide: `CONTRIBUTING.md`

## License

Apache-2.0
