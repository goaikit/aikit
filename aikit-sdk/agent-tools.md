# Native application tool bridge

The `integration` feature exports `AgentToolSpec`, `AgentToolPlugin`,
`render_agent_tool_plugin`, `AgentToolCall` and `NativeToolContext`. Applications
provide their JSON operation schema, instructions and one fixed executable/argv.
AIKit renders the provider module; the CLI only reads the spec and calls the
library. The example `agent_tool_plugin` demonstrates export and bounded decoding.
It does not launch or install an agent.

## Contract

`AgentToolSpec` contains `application_id`, `installation_id`, `tool_name`,
`description`, `input_schema`, `instructions`, `handler: HookCommand` and
`timeout_ms`. Use a stable installation ID for each registered checkout. IDs and
the generated namespace are not credentials. The handler reuses existing hook
command validation: an absolute existing executable, fixed bounded arguments,
no placeholder expansion and no shell. On Windows it must be a native `.exe`.
Keep secrets out of the spec, instructions and arguments.

The optional `native_completion_bridge` boolean defaults to false in JSON (Rust
struct literals must specify it). It opts into a Claude Stop callback only
through an owned installation with a session handler and input/completion hooks.
See [the completion candidate contract](native-completion-bridge.md) for binding
guards, response translation, limits and outstanding qualification. Standalone
export rejects this option; it does not gain an unowned completion handler.

`AgentToolSpec::read` accepts at most 256 KiB of JSON. Rendering validates the
spec, an object input schema of at most 64 KiB with no external schema retrieval,
a nonempty description of at most 4 KiB, instructions of at most 32 KiB and a
timeout of 1–30,000 ms. The combined native tool name must fit 64 bytes.

`render_agent_tool_plugin("claude", &spec)` returns three fixed files:
`.claude-plugin/plugin.json`, `hooks/hooks.json` and `hooks/register.js`.
The name derives from application and installation identity; a content-derived
manifest version changes when the module changes. `files()` supports embedding;
`invocation_environment_key()` names its literal installation-scoped process
environment variable; that name and its generation claims are not credentials.
`export_to_new_directory` creates a new directory and refuses existing paths.
A failed export may leave its new partial directory for inspection.

**Export is not installation.** It creates no receipt, enables no settings and
promotes no capability. `IntegrationService::plan_agent_tool` now provides owned
project-file planning on an existing hook installation, with the same
`apply_install`/status/update/removal/recovery journal. See
[owned tool installation](owned-agent-tools.md). Replacing a directory alone
does not establish ownership or native readiness.
Cursor, Codex and Pi rendering currently returns `Unsupported`; unknown catalog
keys return `UnknownAgent`.

## Native call and result

The Claude module registers one tool at session start. Its input is
`{"request": <application object>}`. It appends a named instruction block while
preserving existing prompt blocks and metadata. A conflicting block name fails
instead of replacing context. Subagent calls are refused.

The module invokes the fixed handler with a JSON envelope on stdin:

```json
{
  "schema_version": 1,
  "application_id": "example",
  "installation_id": "checkout-identity",
  "agent_key": "claude",
  "tool_name": "review",
  "native_context": {
    "session_id": "native-session",
    "invocation_id": "d986c821-08c9-45e7-a782-b4672533cfba",
    "cwd": "/absolute/checkout"
  },
  "request": {"operation": "application-defined"}
}
```

`session_id` and `cwd` come from the native session API; `invocation_id` is a
UUID generated when the module loads. They remain outside model-controlled
`request`. `AgentToolCall::read` enforces the version, bounded identity/context
shape, object request and 128 KiB envelope/64 KiB request limits, with redacted
decode errors. It does not validate the application's request schema.

**Native metadata is not authentication.** Before reading protected context or
mutating state, the application must authenticate the invocation against trusted
installation/credential data, validate the current recorded and native session
generation and scope the request. Neither a UUID nor a session ID establishes
that authority. This bridge supplies no new credential store or session registry.

Owned installation now adds a startup callback through the existing hook handler
and carries the module's invocation context into that handler's process environment.
It bootstraps a recorded generation matching tool calls, without treating JSON
metadata as authority. Standalone exports do not configure that handler. See
[recorded tool generations](agent-tool-generations.md) for startup ordering,
legacy compatibility and authentication limits.

Return bounded JSON on stdout. Exit zero yields a normal tool result; a nonzero
exit with valid JSON yields an application error result. Invalid JSON, exceptions,
either native truncation flag, or stdout over 1 MiB produce a static redacted
denial. There is one process attempt and no automatic retry. A timeout or lost
result may leave an application operation uncertain; its durable operation ID
must be reconciled by the application. Stderr is not returned to the model.

Claude's registered tool uses its internal MCP transport. This does not expose
an application MCP server or change Codelaya's optional read-only MCP contract.

## Qualification, 2026-10-17

- Rust 1.88 integration tests: 84 passed on Windows, 86 on Linux in Docker.
  These are selected SDK integration tests, not the full workspace suite.
- The example builds on both platforms. SDK/example Clippy passed with the
  existing `uninlined_format_args` allowance; full strict SDK Clippy is not green.
- Six offline tests in Claude 2.1.296's Linux plugin engine passed: literal
  argv/envelope, structured failure, truncation, exception/no retry, instruction
  preservation and subagent refusal. Native actions in these tests are mocked.
- A separate ordinary interactive Linux terminal loaded the final generated
  plugin and called the compiled Rust inspector. The exact Unicode result was
  present once in the following actual API request, with matching native session
  IDs and preserved application/other instructions. Replies were scripted by a
  local gateway; no account credentials or owner profile were mounted.

The [fixture guide](examples/agent-tool-qualification/README.md) records hashes
and reproduction. These results do not qualify caller authentication, owned
activation, Codelaya mutations, real model instruction following, protected
completion ordering/deadlines, review delivery/consumption or native Windows
operation. Required integration readiness remains false.
