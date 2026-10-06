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

## Managed gateway persistence

Enable the additive `integration` feature to reuse
`aikit_sdk::integration::gateway_store::Store`. This is the existing managed
gateway SQLite implementation, shared with the CLI through its compatibility
re-export. Session payloads and command receipts remain the canonical
`runner::session` types; existing storage/recovery behavior is preserved.

Opening a persistent store acquires exclusive host ownership and recovers
interrupted managed sessions and ambiguous commands. Do not open it independently
from each external hook invocation. User-started hooks use the separate
`IntegrationService` below, which does not open the managed gateway store.

## User-started agent hooks (integration feature)

The additive `integration` feature exposes `IntegrationService`, owned hook
configuration and one `HookHandler::decide` callback. Construction never launches
an agent. The current native adapter is Claude; other known catalog keys return
`IntegrationError::Unsupported`. Session binding and message delivery are pending.

```rust,no_run
use aikit_sdk::integration::{IntegrationService, InstallSpec};
# fn example(spec: InstallSpec) -> Result<(), Box<dyn std::error::Error>> {
let service = IntegrationService::open("/private/application-state")?;
let plan = service.plan_install(spec)?; // inspect paths, events and fingerprints
let status = service.apply_install(&plan.id)?;
let removal = service.plan_remove(&plan.installation_id)?;
// Applying the removal is a separate explicit application action:
// service.apply_install(&removal.id)?;
# Ok(())
# }
```

Keep state outside the reviewed worktree and private to the owner. Unix state
directories require mode 0700; Windows uses the caller's inherited ACL. Installation
receipts identify exactly owned entries. Stale plans, changed owned hooks and
ambiguous recovery refuse mutation. Unrelated hooks/settings survive update/removal.
Retry the same plan ID after interruption; do not synthesize a replacement plan.
Pending journals may contain existing configuration values; completed plans discard
their config bodies. `Configured` means matching config, not qualified enforcement.

`handle_hook(installation_id, input, handler).await` consumes bounded native bytes
and returns stdout, stderr and an exit code. The provider comes from the receipt.
The thin executable must honor all three response fields. `HookHandler` receives
input admission, before-tool and completion proposals; observations are journaled
without calling a decision handler. Callbacks must cooperate with cancellation.
Errors, timeouts and panics block. Allow preserves native permission checks.

`events(installation_id, after, limit)` returns immutable observation and prepared
decision rows with separate cursors. Neither a saved Allow nor SessionEnd proves
successful Turn completion. Tool arguments/results reach the callback but are
omitted from replay; persist necessary derived application evidence during the
callback. Records have `tool_payload_omitted` to make this visible. Final answers
are retained. The current schema retains events without pruning.

Installation uses Claude exec-form command plus argument vectors. Windows needs
a real executable, not a `.cmd`/`.bat` shim. The installer does not own global
continuation-limit settings or override managed settings. Effective settings and
native deadline behavior require qualification for the deployed version and mode.

### Library-only example

Build `cargo build -p aikit-sdk --no-default-features --features integration --example integration_hooks`.
The resulting executable offers these operations:

```text
integration_hooks plan STATE                   # InstallSpec JSON on stdin
integration_hooks apply STATE PLAN_ID
integration_hooks remove-plan STATE INSTALLATION_ID
integration_hooks status STATE INSTALLATION_ID
integration_hooks events STATE INSTALLATION_ID
integration_hooks hook STATE WORKSPACE BLOCKS
```

For this example, set application ID `sdk-example`, agent key `claude`, executable
to the absolute built example path, and arguments to `hook`, absolute state path,
absolute workspace path, and a block count. Register the events needed by the
scenario. The example blocks the first BLOCKS completion proposals per native
session, then allows a nonempty final answer. This is a qualification gate, not
a review policy. An external caller starts the native agent.

Native qualification and its limits are recorded in
[`integration-qualification.md`](integration-qualification.md).

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

Merge one MCP server definition into the config file each assistant expects. **Supported keys** (see `mcp_supported_agents()` and `MCP_SUPPORTED_AGENT_KEYS`): `cursor-agent`, `claude`, `gemini`, `copilot`, `opencode`, `codex`. **Aliases** (same as CLI): `cursor` → `cursor-agent`, `vscode` → `copilot`.

| Key | Project file (under `project_root`) | Global scope |
|-----|--------------------------------------|--------------|
| `cursor-agent` | `.cursor/mcp.json` | `~/.cursor/mcp.json` |
| `claude` | `.mcp.json` | `~/.claude.json` |
| `gemini` | `.gemini/settings.json` | `~/.gemini/settings.json` |
| `copilot` | `.vscode/mcp.json` | VS Code user `mcp.json` (macOS: `~/Library/Application Support/Code/User/…`; Linux: `~/.config/Code/User/…`; Windows: `%APPDATA%\Code\User\…` or `~\AppData\Roaming\…` if unset) |
| `opencode` | `opencode.json` | User `opencode.json` via XDG / Roaming fallback |
| `codex` | `.codex/config.toml` | `~/.codex/config.toml` |

**JSON agents** (`cursor-agent`, `claude`, `gemini`): root `mcpServers.<name>`. **Copilot**: root `servers.<name>` with `type` `stdio` or `http`. **OpenCode**: root `mcp.<name>`. **Codex**: `[mcp_servers.<name>]` in TOML.

**Public API:** `add_mcp_server`, `mcp_config_path`, `normalize_mcp_agent_key`, `mcp_supported_agents`, `parse_env_pairs`, `parse_header_pairs`, `MCP_SUPPORTED_AGENT_KEYS`, `AddMcpServerOptions`, `McpScope`, `McpServerTransport`, `McpDeployError`. Errors are `Result<_, McpDeployError>` (unknown agent, unsupported agent, missing home, I/O, JSON/TOML, duplicate name without `overwrite`, bad `KEY=value` pairs).

On **Windows**, VS Code user MCP and OpenCode global paths fall back to `<home>\AppData\Roaming\...` when `%APPDATA%` or `dirs::config_dir()` is missing.

### HTTP transport

```rust
use aikit_sdk::{
    add_mcp_server, AddMcpServerOptions, McpScope, McpServerTransport,
};
use std::path::Path;

let path = add_mcp_server(AddMcpServerOptions {
    agent_key: "gemini".into(),
    scope: McpScope::Project,
    project_root: Path::new(".").to_path_buf(),
    server_name: "remote".into(),
    transport: McpServerTransport::Http {
        url: "http://127.0.0.1:8730/mcp".into(),
        headers: None,
    },
    overwrite: false,
})?;
println!("{}", path.display());
# Ok::<(), aikit_sdk::McpDeployError>(())
```

### Stdio transport

```rust
use aikit_sdk::{
    add_mcp_server, AddMcpServerOptions, McpScope, McpServerTransport,
};
use std::{collections::HashMap, path::Path};

let path = add_mcp_server(AddMcpServerOptions {
    agent_key: "claude".into(),
    scope: McpScope::Project,
    project_root: Path::new(".").to_path_buf(),
    server_name: "fs".into(),
    transport: McpServerTransport::Stdio {
        command: "npx".into(),
        args: vec![
            "-y".into(),
            "@modelcontextprotocol/server-filesystem".into(),
            ".".into(),
        ],
        env: Some(HashMap::from([("FOO".into(), "bar".into())])),
    },
    overwrite: false,
})?;
println!("{}", path.display());
# Ok::<(), aikit_sdk::McpDeployError>(())
```

**Tests:** `AIKIT_MCP_TEST_HOME` overrides the home directory used for global path resolution when `aikit-sdk` is built with `cfg(test)` only (not in normal library builds).

## Instruction files

For agent guidance files (`AGENTS.md`, `CLAUDE.md`, `GEMINI.md`):

- `instruction_file(...)`
- `resolve_instruction_file(...)`
- `instruction_file_with_override(...)`
- `instruction_file_agents()`

These helpers provide deterministic paths and fallback behavior per agent.

## Run agents

Runnable keys: `codex`, `claude`, `gemini`, `opencode`, `cursor`, `pi`.

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

## Structured pipeline

`Pipeline` chains template rendering → agent invocation → JSON schema validation → report
generation with optional automatic retry.

```rust
use aikit_sdk::pipeline::{Pipeline, OutputFormat};
use aikit_sdk::agent_runner::AgentRunner;

let result = Pipeline::new(
    "Answer the question: {{question}}",
    r#"{"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"]}"#,
)
.max_retries(2)
.output_format(OutputFormat::Json)
.run(
    &[("question", "What is 2+2?")],
    AgentRunner::new().agent("claude"),
)?;

println!("{}", result.report);   // pretty-printed JSON
println!("{}", result.attempts); // number of attempts used
# Ok::<(), aikit_sdk::pipeline::PipelineError>(())
```

For Markdown output, set `.output_format(OutputFormat::Markdown)` and provide a
`.report_template("Answer: {{answer}}")`. Top-level JSON keys become template slots;
`{{report_body}}` is always available as the full pretty-printed data.

**`PipelineError` variants:** `TemplateSlotMissing`, `AgentInvocation`, `ValidationFailed`,
`MaxRetriesExceeded`, `ReportRender`.

## AgentRunner

`AgentRunner` is a builder for running a single agent invocation inside a pipeline.

```rust
use aikit_sdk::agent_runner::AgentRunner;

let runner = AgentRunner::new()
    .agent("claude")
    .model("claude-sonnet-4-5")
    .working_dir("/path/to/project");

let text = runner.run("Summarize the project")?;
# Ok::<(), aikit_sdk::pipeline::PipelineError>(())
```

## AgentDetector

`AgentDetector::detect()` probes all runnable agent keys and returns availability status.

```rust
use aikit_sdk::agent_runner::AgentDetector;

for info in AgentDetector::detect() {
    println!("{}: installed={}", info.key, info.installed);
}
```

## Template rendering

`TemplateRenderer` renders `{{slot}}` templates in a single pass. Use `\{{` and `\}}` to
emit literal braces. Missing slots return `PipelineError::TemplateSlotMissing`; unused
slots are silently ignored.

```rust
use aikit_sdk::template::TemplateRenderer;
use aikit_sdk::pipeline::PipelineError;

let rendered = TemplateRenderer::render("Hello, {{name}}!", &[("name", "world")])?;
# Ok::<(), PipelineError>(())
```

## JSON validation

`ResponseValidator` extracts the first ` ```json ` fenced block from agent output (falling
back to bare JSON) and validates it against a JSON Schema.

```rust
use aikit_sdk::validation::ResponseValidator;

let schema = r#"{"type":"object","properties":{"score":{"type":"integer"}},"required":["score"]}"#;
let validated = ResponseValidator::validate(r#"{"score": 9}"#, schema)?;
println!("{}", validated.data["score"]);
# Ok::<(), aikit_sdk::pipeline::PipelineError>(())
```

## Report rendering

`ReportRenderer` produces Markdown or JSON output from validated agent data.

```rust
use aikit_sdk::report::ReportRenderer;
use serde_json::json;

let data = json!({"name": "Alice", "score": 42});
let md = ReportRenderer::render_markdown("Name: {{name}}, Score: {{score}}", &data)?;
let js = ReportRenderer::render_json(&data)?;
# Ok::<(), aikit_sdk::pipeline::PipelineError>(())
```

## Session store

`SessionStore` persists multi-turn sessions to `~/.aikit/sessions/` (or `$AIKIT_SESSIONS_DIR`).

```rust
use aikit_sdk::session_store::{SessionStore, SessionFile};

let store = SessionStore::open();
// load, save, update_index, last_for_cwd
```

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
- Site docs (MCP): `../webdocs/mcp.mdx`
- Contributor guide: `CONTRIBUTING.md`

## License

Apache-2.0
