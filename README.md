# AIKIT

**Multi-agent template package manager and CLI.** Install and publish `aikit.toml` template packages from **GitHub or a local directory**, map them into many coding-assistant layouts, scaffold projects with **`aikit init`**, and run supported agent CLIs with **`aikit run`**. Use **`aikit check`** to see which tools and agents are available.

**Programmatic gateway:** [aikit-sdk](aikit-sdk/README.md) (Rust) and [aikit-py](aikit-py/README.md) (Python) expose the same agent catalog, path rules, deploy APIs, availability checks, and run/event APIs for your own tools and automation.

## Workspace crates

- `aikit` (root crate): CLI for package lifecycle, install/init flows, release, run/llm commands
- [`aikit-sdk`](aikit-sdk/README.md): Rust library for catalog/deploy and run/event APIs
- [`aikit-py`](aikit-py/README.md): Python package exposing the same gateway behaviors
- [`aikit-agent`](aikit-agent/README.md): in-process agent runtime used by `aikit run -a aikit`

## Installation

### Linux (GNU)
```bash
curl -L https://github.com/goaikit/aikit/releases/latest/download/aikit-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv aikit /usr/local/bin/
```

### Linux (MUSL/Universal)
```bash
curl -L https://github.com/goaikit/aikit/releases/latest/download/aikit-x86_64-unknown-linux-musl.tar.gz | tar xz
sudo mv aikit /usr/local/bin/
```

### Homebrew (Linux)
```bash
brew install goaikit/cli/aikit
```

### Scoop (Windows)
```powershell
scoop bucket add goaikit https://github.com/goaikit/scoop-bucket
scoop install aikit
```

### Windows
Download from [GitHub Releases](https://github.com/goaikit/aikit/releases/latest) and add to PATH.

## Quick Start

### Install Newton template (workspace with scripts)

```bash
# Install Newton template from GitHub or local path
aikit install gonewton/newton-templates --ai newton --yes

# This creates .newton/ with:
#   .newton/README.md - Template documentation
#   .newton/scripts/advisor.sh - Planning advisor
#   .newton/scripts/evaluator.sh - Progress evaluator
#   .newton/scripts/post-success.sh - Post-success hook
#   .newton/scripts/post-failure.sh - Post-failure hook
```

### Create a package and deploy to Cursor and Claude

```bash
# 1. Create a new package (defines aikit.toml and template layout)
aikit package init my-tools --description "My AI commands"

# 2. Enter the package and add your templates (rules, skills, prompts)
cd my-tools
# Edit aikit.toml and add files under templates/ as needed

# 3. Build the package (produces dist/ or agent-specific zips)
aikit package build

# 4. Publish to GitHub (creates release and uploads assets)
aikit package publish username/my-tools
# Or: push repo first, then aikit release v1.0.0

# 5. Install the package for Cursor (in a project that uses Cursor)
cd /path/to/your-project
aikit install username/my-tools --ai cursor

# 6. Install the same package for Claude (e.g. in another project or --ai claude)
aikit install username/my-tools --ai claude

# 7. Verify: list installed packages and check available agents
aikit list
aikit check
```

### Use an existing project with an AI assistant

```bash
# Create a new Spec-Driven Development project with Claude templates
aikit init my-project --ai claude

# Or set up in the current directory for Cursor
aikit init --here --ai cursor

# Install a community package
aikit install username/package-name
aikit list
```

## Commands

| Command | Description |
|---------|-------------|
| `aikit init [name]` | Initialize a Spec-Driven Development project with AI assistant templates |
| `aikit install <source>` | Install packages from GitHub (owner/repo) or local directory (use `--ai <agent>` to specify agent) |
| `aikit list` | Show installed packages (optional: `--author`, `--detailed`) |
| `aikit update <pkg>` | Update a package to latest version (optional: `--breaking`) |
| `aikit remove <pkg>` | Uninstall a package (optional: `--force`) |
| `aikit check` | Check git, VS Code, and AI agent CLIs availability |
| `aikit run` | Run a coding agent with a prompt |
| `aikit version` | Show version |
| `aikit package init <name>` | Create a new package with aikit.toml |
| `aikit package build` | Build distributable package (output: dist/ or .genreleases/) |
| `aikit package publish <owner/repo>` | Publish package to GitHub (release and assets) |
| `aikit llm` | Invoke an LLM via OpenAI-compatible API (supports streaming and JSON output) |
| `aikit release <version>` | Create GitHub release from .genreleases/ (e.g. v1.0.0) |

## Creating and publishing packages

```bash
# Create package
aikit package init my-tools --description "AI dev tools" --package-version 0.1.0

# Add templates and edit aikit.toml, then build
aikit package build

# Publish (creates GitHub release and uploads)
aikit package publish username/my-tools

# If you use a flow that produces zips in .genreleases/, create the release with:
# aikit release v1.0.0 --notes-file release_notes.md
```

## Configuration

- **GitHub auth:** Set `GITHUB_TOKEN` or `GH_TOKEN` in `.env` or use `--token` / `--github-token` on install, init, or release.
- **Package manifest:** Each package has an `aikit.toml` (name, version, description). Required for local installs and for publish.

Example `.env`:

```bash
GITHUB_TOKEN=your_github_token_here
```

## Running agents

The `aikit run` command allows you to execute AI coding agents directly. It can replace agent CLI wrappers like coder.sh in workflows like Newton.

```bash
# Run an agent with a prompt
aikit run --agent opencode -p "Help me refactor this code"

# Read prompt from stdin
echo "Add error handling" | aikit run --agent claude

# Emit structured NDJSON events to stdout (one JSON object per line)
aikit run --agent claude --events -p "Summarize the project"

# Combine --events with --stream for streaming-aware JSON output
aikit run --agent claude --events --stream -p "Refactor this module"
```

**Supported agents:** `codex`, `claude`, `gemini`, `opencode`, `agent`, `auto`

**Options:**

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--agent` | `-a` | Runnable agent key (`codex`, `claude`, `gemini`, `opencode`, `agent`, `auto`) | **Required** |
| `--model` | `-m` | Model passed to the agent | Omitted unless `-m` is set; no CLI default (agent binary decides) |
| `--prompt` | `-p` | Prompt to run | Reads from stdin if omitted |
| `--yolo` | | Auto-confirm, skip checks | `false` |
| `--stream` | | Enable agent-native streaming output flags | `false` |
| `--events` | | Emit NDJSON event stream to stdout | `false` |
| `--progress` | | Display live human-readable progress on stderr (conflicts with `--events`) | `false` |

`aikit run` also accepts the root **`--debug`** flag (global on the `aikit` CLI; it appears in `aikit run --help`).

Agent selection is **CLI-only**: `-a` / `--agent` is required. `aikit run` does not read `CODING_AGENT` or `CODING_AGENT_MODEL`. When `-m` is omitted, no model flag is passed to the agent; the spawned agent applies its own defaults or errors if it requires an explicit model.

**`--stream` vs `--events`:**
- `--stream`: Tunes agent-native partial output flags (e.g. `--stream-partial-output`, `stream-json`). Passed through to the agent argv builder.
- `--events`: Switches the CLI to emit one JSON object per line (NDJSON) to stdout. Process exit code matches the child agent's exit code.
- Both flags can be combined: `--events --stream` uses events-mode JSON output AND adds stream-partial flags for supported agents.

**NDJSON event format** (when using `--events`): each line is one JSON object with `agent_key`, `seq`, `stream` (`stdout` | `stderr`), and `payload`. The payload is one of:

- **`json_line`**: parsed JSON object from the agent
- **`raw_line`**: UTF-8 text line that is not JSON
- **`raw_bytes`**: non-UTF-8 data as a byte array in JSON
- **`token_usage_line`**: normalized token counts extracted from a preceding `json_line` (fields: `usage`, `source`, `raw_agent_line_seq`)

Example lines:

```json
{"agent_key":"claude","seq":1,"stream":"stdout","payload":{"json_line":{"type":"progress","message":"Starting..."}}}
{"agent_key":"claude","seq":2,"stream":"stdout","payload":{"token_usage_line":{"usage":{"input_tokens":100,"output_tokens":50,"total_tokens":150,"cache_read_tokens":null,"cache_creation_tokens":null,"reasoning_tokens":null},"source":"Claude","raw_agent_line_seq":1}}}
```

On **Windows**, the Cursor agent is often `agent.cmd`. If spawn fails, set **`AIKIT_CURSOR_AGENT`** to the full path (see [aikit-sdk README: Windows Configuration](aikit-sdk/README.md#windows-configuration)).

Run `aikit run --help` for the authoritative option reference.

**Programmatic use (gateway libraries):**

- **Rust ([aikit-sdk](aikit-sdk/README.md)):** Add `aikit-sdk` to `Cargo.toml`. Use catalog/path/deploy APIs, `run_agent` for buffered stdout/stderr, or `run_agent_events` for the same event stream shape as `aikit run --events` (including optional `token_usage_line` events). Returns `Result<RunResult, RunError>`.
- **Python ([aikit-py](aikit-py/README.md)):** `pip install aikit-py`. Same surface area from Python: catalog, deploy, `run_agent`, and `run_agent_events_py(..., on_event, ...)` for per-event callbacks (schema matches CLI NDJSON).

## Supported AI assistants

The catalog covers **18** coding assistants (install/template mapping). Runnable via **`aikit run`** are only: `codex`, `claude`, `gemini`, `opencode`, `agent`, `auto` (routing mode that resolves to a concrete agent).

**CLI-based:** Claude, Gemini, Qwen, OpenCode, Codex, Auggie, CodeBuddy, Qoder, Q, Amp, Shai

**IDE-based:** GitHub Copilot, Cursor, Windsurf, KiloCode, Roo, Bob

Run `aikit check` to see which are installed on your system (git and VS Code are also checked).

## License

Apache License 2.0 - See [LICENSE](LICENSE)

Need help? [Open an issue](https://github.com/goaikit/aikit/issues)

Contributor guide: [CONTRIBUTING.md](CONTRIBUTING.md)  
Architecture: [architecture.md](architecture.md)  
Testing details: [TESTING.md](TESTING.md)
