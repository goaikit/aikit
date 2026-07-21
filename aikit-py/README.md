# aikit-py

`aikit-py` is the Python binding for `aikit-sdk`.
It exposes the same core capabilities from Python:

- agent catalog lookup
- deterministic path resolution
- command/skill/subagent deployment
- MCP server entries merged into agent config files (JSON/TOML)
- runnable-agent availability checks
- agent execution (`run_agent`)
- streaming callbacks (`run_agent_events_py`)

## Requirements

- Python `>=3.9`
- Rust toolchain (needed for local build from source)

## Install

```bash
pip install aikit-py
```

With **uv**:

```bash
uv add aikit-py
```

For local development from this workspace (recommended — uses the checked-in `uv.lock`):

```bash
cd aikit-py
uv sync
uv run maturin develop
```

Or with plain pip:

```bash
cd aikit-py
python -m venv .venv
source .venv/bin/activate
pip install -U pip maturin
maturin develop
```

## Quick start

```python
import aikit_py
from tempfile import TemporaryDirectory

# Catalog
print([a.key for a in aikit_py.all_agents()])
aikit_py.validate_agent_key("claude")

with TemporaryDirectory() as root:
    # Deploy a command for Claude
    path = aikit_py.deploy_command(
        "claude",
        root,
        "hello",
        "# hello command\nprint('hello')\n",
    )
    print(path)
```

## MCP server config (merge)

Supported agent keys match the Rust SDK: `cursor-agent`, `claude`, `gemini`, `copilot`, `opencode`, `codex`. Aliases `cursor` and `vscode` are normalized like the CLI.

- `mcp_config_path(agent_key, scope, project_root)` returns the config file path. `scope` is `"project"` or `"global"`.
- `add_mcp_server(...)` merges one server (stdio or HTTP) into that file. Raises `McpDeployError` on I/O, parse, duplicate name (unless `overwrite=True`), or invalid input.
- `mcp_supported_agents()` returns rows with `agent_key`, `display_name`, `project_config_path`, `global_config_path`.
- `mcp_supported_agent_keys()` returns the key list.
- `normalize_mcp_agent_key(key)` returns the catalog key string.
- `mcp_parse_env_pairs` / `mcp_parse_header_pairs` parse `KEY=value` lines into a string map (same rules as `aikit mcp add`).

```python
import json
import aikit_py
from tempfile import TemporaryDirectory

with TemporaryDirectory() as root:
    written = aikit_py.add_mcp_server(
        "claude",
        root,
        "my-tools",
        scope="project",
        command="npx",
        args=["-y", "@modelcontextprotocol/server-filesystem", root],
        env=None,
        url=None,
        headers=None,
        overwrite=False,
    )
    with open(written) as f:
        cfg = json.load(f)
    assert "my-tools" in cfg["mcpServers"]
```

Global scope uses the real user home (same as the CLI). Use project scope in tests and automation so paths stay inside a temp directory.

HTTP example:

```python
written = aikit_py.add_mcp_server(
    "gemini",
    root,
    "remote",
    scope="project",
    url="http://127.0.0.1:8730/mcp",
    headers={"Authorization": "Bearer token"},
    overwrite=False,
)
```

Errors: path/catalog failures raise `McpDeployError`; deploy helpers raise `DeployError`.

Runnable script: `examples/mcp_deploy_stdio.py`. MCP narrative for the doc site: `../webdocs/mcp.mdx`. Rust API tables: [aikit-sdk README](../aikit-sdk/README.md#mcp-config-merge).

## Run an agent

Runnable keys: `codex`, `claude`, `gemini`, `opencode`, `agent`.

```python
import aikit_py

if aikit_py.is_runnable_py("claude"):
    result = aikit_py.run_agent(
        "claude",
        "Summarize this repository",
        model=None,
        yolo=False,
        stream=False,
    )
    print(result["stdout"].decode("utf-8", errors="replace"))
```

## Streaming events

Use `run_agent_events_py` for incremental callback delivery while the child process is running.

```python
import aikit_py

def on_event(event: dict) -> None:
    seq = event["seq"]
    stream = event["stream"]
    payload = event["payload"]
    print(seq, stream, payload.keys())

result = aikit_py.run_agent_events_py(
    "claude",
    "List important modules",
    on_event,
    model=None,
    yolo=False,
    stream=True,
)
print(result["status_code"])
```

Event payload contains exactly one of:

- `json_line`
- `raw_line`
- `raw_bytes`
- `token_usage_line`

## Main API surface

- `all_agents()`, `agent(key)`, `validate_agent_key(key)`
- `commands_dir(...)`, `skill_dir(...)`, `subagent_path(...)`
- `deploy_command(...)`, `deploy_skill(...)`, `deploy_subagent(...)`
- `mcp_config_path`, `add_mcp_server`, `mcp_supported_agents`, `mcp_supported_agent_keys`, `normalize_mcp_agent_key`, `mcp_parse_env_pairs`, `mcp_parse_header_pairs`
- `run_agent(...)`, `run_agent_events_py(...)`
- `runnable_agents_list()`, `is_runnable_py(key)`
- `is_agent_available(key)`, `get_installed_agents()`, `get_agent_status()`

## Test

From workspace root:

```bash
cargo test -p aikit-py
cd aikit-py && uv run maturin develop && uv run pytest tests/
```

Or with plain pip:

```bash
cd aikit-py && maturin develop && pytest tests/
```

Python tests: `aikit-py/tests/test_aikit_py.py`, `aikit-py/tests/test_mcp_deploy.py`.

## Related docs

- Workspace overview: `../README.md`
- Rust gateway: `../aikit-sdk/README.md`
- MCP (site source): `../webdocs/mcp.mdx`
- Contributor guide: `CONTRIBUTING.md`

## License

Apache-2.0
