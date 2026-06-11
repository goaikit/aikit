# AIKit

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

**One CLI + gateway for every coding-agent CLI on your machine.** AIKit lets
you run, package, share, and serve AI coding agents the same way regardless
of whether you're using Claude Code, Codex, Gemini CLI, OpenCode, the
built-in `aikit` agent, or anything else in the catalog.

## What you get

- **Run any agent the same way** — `aikit agent run --agent <name> -p "…"`
  works for Claude, Codex, Gemini, OpenCode, and the built-in `aikit` agent.
  Optional structured event stream (NDJSON) for tooling.
- **Multi-turn HTTP API** — `aikit serve` exposes the agent runtime over
  HTTP with both SSE streaming and single-shot JSON responses, selected by
  the standard `Accept` header. Implicit sessions, resume by id, the works.
- **Magic tools (optional)** — build with `--features tools` to mount
  schema-driven form-fill endpoints on `aikit serve` (`/api/v1/aitools/…`),
  including `agents/draft_definition` to draft an agent definition from plain
  English. See [`aikit-magictool`](aikit-magictool/README.md).
- **Templates and package management** — `aikit init` scaffolds a
  Spec-Driven Development project; `aikit install/update/remove/list` manage
  packaged commands, skills, and agent definitions from GitHub or a local
  path.
- **Share your own packages** — `aikit package init/build/publish` produces
  installable artifacts and pushes them to GitHub Releases.
- **MCP config merge** — `aikit agent mcp add` registers an MCP server
  entry in whichever agent's config file is appropriate (Cursor, Claude,
  Gemini, VS Code Copilot, OpenCode, Codex).
- **Rust + Python gateways** — [`aikit-sdk`](aikit-sdk/README.md) (Rust)
  and [`aikit-py`](aikit-py/README.md) (Python) expose the same catalog,
  paths, deploy, detection, and run/event APIs programmatically.

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

Verify with `aikit check` — it reports which agent CLIs are installed.

---

## Features

The rest of this document walks each feature top-down. Every section is
independent; jump to whatever you need.

## 1. Project scaffolding (`aikit init`)

Bootstrap a project with templates for a specific assistant.

```bash
# New project for Claude
aikit init my-project --ai claude

# Or scaffold into the current directory for Cursor
aikit init --here --ai cursor
```

`--ai` accepts any catalog key (`claude`, `cursor`, `gemini`, `codex`,
`copilot`, `opencode`, …). Multiple `--ai` flags add several agents in one
shot.

## 2. Running agents (`aikit agent run`)

A uniform CLI front-end for every supported coding-agent binary.

```bash
# Inline prompt
aikit agent run --agent claude -p "Refactor this function for clarity"

# Prompt from stdin
echo "Add error handling to main.rs" | aikit agent run --agent opencode

# Agent-native streaming
aikit agent run --agent claude --stream -p "Summarize the project"

# Structured NDJSON event stream (one JSON object per line on stdout)
aikit agent run --agent claude --events -p "Summarize the project"

# Combine for streaming events
aikit agent run --agent claude --events --stream -p "Refactor this module"
```

**Runnable backends:** `codex`, `claude`, `gemini`, `opencode`, `agent`,
`aikit` (built-in), `auto` (route to whatever is installed).

**Options:**

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--agent` | `-a` | Runnable agent key | **Required** |
| `--model` | `-m` | Model passed to the agent | Agent default |
| `--prompt` | `-p` | Prompt to run | Reads from stdin |
| `--yolo` | | Auto-confirm, skip checks | `false` |
| `--stream` | | Agent-native partial output flags | `false` |
| `--events` | | NDJSON event stream on stdout | `false` |
| `--progress` | | Live human-readable progress on stderr (conflicts with `--events`) | `false` |

`-a`/`--agent` is mandatory. When `-m` is omitted, no model flag is passed
to the agent — its own default applies.

### NDJSON event format

Each `--events` line is one JSON object: `{agent_key, seq, stream, payload}`.
`payload` is one of `json_line`, `raw_line`, `raw_bytes`, `token_usage_line`,
`quota_exceeded`, `stream_message`, or one of the `aikit_*` variants emitted
by the built-in agent. The same shape is what `aikit-sdk::run_agent_events`
delivers via callback.

```json
{"agent_key":"claude","seq":1,"stream":"stdout","payload":{"stream_message":{"text":"Hello","phase":"final","role":"assistant","kind":"message"}}}
{"agent_key":"claude","seq":2,"stream":"stdout","payload":{"token_usage_line":{"usage":{"input_tokens":100,"output_tokens":50}}}}
```

On **Windows**, the Cursor agent is sometimes `agent.cmd`. If spawn fails,
set `AIKIT_CURSOR_AGENT` to the full path (see
[aikit-sdk README: Windows Configuration](aikit-sdk/README.md#windows-configuration)).

`aikit agent list` and `aikit agent check` round out the agent namespace —
list project-scoped agent definitions, and report which CLIs are installed.

## 3. Multi-turn HTTP API (`aikit serve`)

`aikit serve` exposes the same agent runtime over HTTP. Sessions are
created implicitly on the first call; the server returns the new
`session_id` and the client quotes it back to resume.

```bash
# Defaults: 127.0.0.1:8787, 300s timeout, 10 concurrent runs
aikit serve

# Public bind, longer timeout, require an API key
aikit serve --host 0.0.0.0 --port 8787 \
  --run-timeout-secs 600 --max-sessions 20 \
  --api-key "$(openssl rand -hex 32)"
```

**Endpoints:**

| Method | Path | Purpose |
|--------|------|---------|
| `GET`  | `/healthz` | Liveness health check |
| `GET`  | `/readyz` | Readiness health check |
| `GET`  | `/api/v1/agents` | List runnable agents (each with `available` + `auth` status) |
| `POST` | `/api/v1/messages` | Send a turn; creates or resumes a session |
| `GET`  | `/api/v1/sessions` | List active and recently completed runs |
| `GET`  | `/api/v1/sessions/{id}` | Inspect one run |
| `DELETE` | `/api/v1/sessions/{id}` | Abort and close a run |

`GET /api/` redirects `308` to `/api/v1`.

**Two response shapes on `/api/v1/messages`, selected by the `Accept` header:**

```bash
# SSE (incremental — default when Accept is missing or */*)
curl -sN -X POST http://127.0.0.1:8787/api/v1/messages \
  -H 'Accept: text/event-stream' \
  -H 'Content-Type: application/json' \
  -d '{"agent":"aikit","content":"Say hello."}'

# Single JSON body (runs to completion, returns assistant text + session_id)
curl -s -X POST http://127.0.0.1:8787/api/v1/messages \
  -H 'Accept: application/json' \
  -H 'Content-Type: application/json' \
  -d '{"agent":"aikit","content":"Say hello."}' | jq .
```

To resume, pass the `session_id` back in the body. Any other explicit
`Accept` returns `406 Not Acceptable`. Full reference:
[webdocs/serve.mdx](webdocs/serve.mdx).

The SSE stream carries the agent's activity as typed events — `session`,
`text`, `reasoning`, `tool_use`/`tool_result`, `token_usage`,
`step_finish`, sub-agent and `context_compressed`, then `done` (and
`error` on failure). The single-JSON shape mirrors this with `content`,
`exit_code`, an aggregated `usage` object when reported, `session_id`
when known, and `error` (`agent_error`, or `unauthenticated` for auth
failures). Frame richness varies by backend; the built-in `aikit` agent
emits the full set.

**Logs and failure diagnosis:** the server installs a `tracing` subscriber
on stderr that honours `RUST_LOG`
(`RUST_LOG=aikit::serve::run=debug aikit serve` shows every SDK event
mapped to a frame). When an agent exits non-zero with no recognised
output, the sync JSON body promotes the captured stderr tail into
`error: { code: "agent_error", message: <stderr tail> }` — no more silent
`{"content":"", "exit_code":0}`.

### Magic tools (`--features tools`)

Release binaries may omit this layer unless built with the `tools` Cargo
feature. When enabled, `aikit serve` also exposes magic-tool routes under
`/api/v1/aitools/`:

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/v1/aitools` | List registered tools |
| `GET` | `/api/v1/aitools/{ns}/{tool}/schema` | Input/output JSON Schemas and modes |
| `POST` | `/api/v1/aitools/{ns}/{tool}` | One-shot: validated input → **Draft** JSON |
| `POST` | `/api/v1/aitools/{ns}/{tool}/sessions` | Start a multi-turn refinement session |
| `POST` | `/api/v1/aitools/{ns}/{tool}/sessions/{id}/messages` | Send a turn (SSE or sync JSON via `Accept`) |
| `POST` | `/api/v1/aitools/{ns}/{tool}/sessions/{id}/finalize` | Produce the final **Draft** |

Built-in tool: `agents/draft_definition` — draft an `AgentDefinition` from a
description. Reusable library: [`aikit-magictool`](aikit-magictool/README.md).

## 4. Packages

Distribute commands, skills, and agent definitions as `aikit.toml`
packages. Install them per project for whichever assistant you use.

### Install / update / remove / list

```bash
# Install from GitHub (owner/repo) or local path; --ai picks the target shape
aikit install username/my-tools --ai cursor
aikit install ./my-tools --ai claude

aikit list                       # what's installed (and where)
aikit update my-tools            # bump to latest
aikit update my-tools --breaking # allow major version bump
aikit remove my-tools            # uninstall
```

### Authoring and publishing

```bash
# Create a package skeleton with aikit.toml
aikit package init my-tools --description "My AI commands"

# Validate aikit.toml and template files before build
aikit package validate

# Build distributable artifacts
cd my-tools
aikit package build

# Publish to a GitHub release
aikit package publish username/my-tools

# Or use the dedicated release command if you produced .genreleases/
aikit release v1.0.0 --notes-file release_notes.md
```

A package's `aikit.toml` describes name, version, description, and an
`[artifacts]` mapping that determines what files land where. The same
package can target multiple assistants because the artifacts mapping is
per-agent.

## 5. MCP server registration (`aikit agent mcp`)

Merge one MCP server entry into whichever agent's config file is
appropriate (`mcpServers` for Cursor/Claude, VS Code `servers`, OpenCode
`mcp`, Codex `[mcp_servers.*]`).

```bash
# List supported agents and target paths
aikit agent mcp list

# Stdio MCP server (repeat --arg per argv token; --env KEY=value per var)
aikit agent mcp add --agent claude --scope project --project . --name fs \
  --command npx --arg -y --arg @modelcontextprotocol/server-filesystem --arg .

# HTTP MCP server (repeat --header KEY=value per header)
aikit agent mcp add --agent gemini --scope project --project . --name api \
  --url https://api.example.com/mcp --header X-Auth=secret
```

Six catalog keys are supported: `cursor-agent` (alias `cursor`), `claude`,
`gemini`, `copilot` (alias `vscode`), `opencode`, `codex`. Use
`--overwrite` to replace an existing server id. Full reference:
[webdocs/mcp.mdx](webdocs/mcp.mdx).

## 6. Programmatic use

Both crates expose the same capabilities as the CLI:

- **Rust:** [`aikit-sdk`](aikit-sdk/README.md). Add it to `Cargo.toml` and
  call `run_agent`, `run_agent_events`, `add_mcp_server`, `agent`,
  `all_agents`, etc. The SDK also provides a structured agent pipeline
  (`Pipeline`, `AgentRunner`, `ResponseValidator`, `ReportRenderer`,
  `TemplateRenderer`) for template rendering → agent invocation → JSON schema
  validation → report generation, plus `AgentDetector` for probing which
  agents are installed, and `SessionStore` for session persistence.
- **Python:** [`aikit-py`](aikit-py/README.md). `pip install aikit-py`;
  same surface area as the SDK (`run_agent`, `run_agent_events_py`,
  `add_mcp_server`, …).

---

## Configuration

- **GitHub auth:** Set `GITHUB_TOKEN` or `GH_TOKEN` in `.env`, or use
  `--token`/`--github-token` on `install`/`init`/`release`.
- **Server auth:** `aikit serve --api-key <key>` or `AIKIT_SERVE_API_KEY`.
- **Logging:** `RUST_LOG` follows standard `tracing-subscriber` syntax —
  e.g. `RUST_LOG=aikit::serve::run=debug,aikit_sdk=info`.
- **Package manifest:** Each package has an `aikit.toml` (name, version,
  description, `[artifacts]`). Required for local installs and publish.

Example `.env`:

```bash
GITHUB_TOKEN=your_github_token_here
```

## Supported AI assistants

The catalog covers **18** coding assistants for install/template mapping.
`aikit agent run` accepts: `codex`, `claude`, `gemini`, `opencode`,
`agent`, `aikit`, `auto`.

**CLI-based:** Claude, Gemini, Qwen, OpenCode, Codex, Auggie, CodeBuddy,
Qoder, Q, Amp, Shai

**IDE-based:** GitHub Copilot, Cursor, Windsurf, KiloCode, Roo, Bob

Run `aikit check` to see which are installed on your system.

## License

Apache License 2.0 — see [LICENSE](LICENSE).

Need help? [Open an issue](https://github.com/goaikit/aikit/issues).
