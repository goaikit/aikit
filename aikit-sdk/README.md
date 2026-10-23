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

The same feature provides a reusable [native application tool bridge](agent-tools.md)
for selected JSON operations and instructions. Claude rendering and controlled
Linux native execution are implemented. [Owned tool-file planning](owned-agent-tools.md)
reuses the existing installer; authenticated callers and effective qualification
remain required before application mutations can be enabled.

The explicit [native completion bridge candidate](native-completion-bridge.md)
reuses that module, the owned hook handler and installer. It defaults off and
does not establish native ordering, caller authentication or readiness.

The additive `integration` feature exposes `IntegrationService`, owned hook
configuration and one `HookHandler::decide` callback. Construction never launches
an agent. Claude hooks, a Cursor prompt/tool/session subset, and a Codex
prompt/tool/Stop/session subset, and a Pi extension bridge are implemented. Other known catalog keys return
`IntegrationError::Unsupported`.
Existing-session observation bindings are available;
native message delivery and contextual control qualification remain pending.

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

`plan_install_with_completion_policy(spec, CompletionBlockingPolicy::RequireUnbounded)`
explicitly requests owned native continuation-limit configuration. Currently Claude
configures `env.CLAUDE_CODE_STOP_HOOK_BLOCK_CAP="0"`; unsupported adapters fail
explicitly. The preview includes exact setting changes; update/removal preserve
the previous value and refuse unsafe restoration under dependent Stop hooks.
Ordinary updates retain a previously selected policy. See
[completion-policy.md](completion-policy.md) for schema compatibility, native
qualification limits and reproducible checks. This does not promote capabilities.

`handle_hook(installation_id, input, handler).await` consumes bounded native bytes
and returns stdout, stderr and an exit code. The provider comes from the receipt.
The thin executable must honor all three response fields. `HookHandler` receives
input admission, before-tool and completion proposals; observations are journaled
without calling a decision handler. Callbacks must cooperate with cancellation.
Errors, timeouts and panics block. Allow preserves native permission checks.
Decision requests recheck the owned local hook configuration before calling policy
and after it returns. Disabled, missing or edited owned hooks block without a
prepared Allow. Unrelated settings edits are preserved. The observation is saved
before these checks; notification-only events remain available for recovery.
This does not attest effective managed/user settings or prevent edits after the
last check, and cannot enforce a hook the provider never invokes.

`events(installation_id, after, limit)` returns immutable observation and prepared
decision rows with separate cursors. Neither a saved Allow nor SessionEnd proves
successful Turn completion. Tool arguments/results reach the callback but are
omitted from replay; persist necessary derived application evidence during the
callback. Records have `tool_payload_omitted` to make this visible. Final answers
are retained. The current schema retains events without pruning.

`tool_effect(&HookRequest)` translates the original BeforeTool payload to
ReadOnly, ReplaceFile, EditText or Unknown. The Claude adapter normalizes exact
Write/Edit intent; Bash, MCP and unrecognized tools remain Unknown. This helper
does not edit files or prove success/attribution. Consumers compare the intent
with observed file bytes and persist derived evidence before returning Allow.
Replay omits inputs and therefore cannot reconstruct intent after the fact.

Installation uses Claude exec-form command plus argument vectors. Windows needs
a real executable, not a `.cmd`/`.bat` shim. The installer does not own global
continuation-limit settings or override managed settings. Effective settings and
native deadline behavior require qualification for the deployed version and mode.

### Contextual capabilities

`IntegrationService::capabilities(agent_key, SessionMode)` performs a fresh local
`--version` probe through the existing Backend registry, command resolution and
availability timeout. It reports host OS/architecture, intended mode, bounded
version output, probe errors and typed capability assessments. It starts no agent
session and does not install or alter hooks. Run this blocking operation outside
an async executor thread; it is an explicit diagnostic, not a per-hook hot path.

Each assessment separates adapter implementation from Supported, Unsupported or
Unknown and includes its scope. SDK installation/replay/binding/detach contracts
do not attest native execution. Native evidence currently matches only the recorded
Windows x86_64 Claude 2.1.269 print-mode scenarios. Other contexts remain Unknown.
Repeated blocking/effective settings and hard native deadlines remain Unknown even
in that context. Successful completion, existing-session messaging/reconciliation
and native process identity remain Unsupported. Missing external adapters report
Unsupported even if their managed runner supports similar operations.

`CapabilityReport::require` returns structured unmet requirements; Unknown cannot
pass. `bind_existing_requiring(reference, mode, required)` checks them before
creating a binding, then applies normal current-reference validation. The original
observation-only `bind_existing` and `BackendCapabilities` flags retain their
semantics. Reports describe a probe of this host and an intended mode, not proof
of a particular running process's version, mode, effective settings or liveness.

```text
integration_hooks capabilities STATE claude print
integration_hooks capabilities STATE cursor interactive
```

### Existing-session failure observations

`HookEvent::CompletionFailed` installs Claude's `StopFailure` notification. It
records a failed turn without calling the policy callback or closing the session
binding. Error details and the native error-message field are not retained as a
Final Answer. Consumers can recover their application state from this observation;
it does not establish successful completion. Existing installations need an
explicit install update to subscribe to this additional event.

### Existing-session observation binding

`SessionBinding::admit_recorded(projected_through)` supplies a short-lived
`RecordedSessionGuard` over the existing installation lock and journal database.
Applications can stage their transaction, refuse new/revoked recorded generations
and retain this guard through commit. This is recorded-state coordination, not
native caller authentication or a capability promotion. See
[consumer order and qualification limits](recorded-session-admission.md).

Resolve `observed_session(installation_id, native_session_id)` after a SessionStart
hook, then call `bind_existing(&reference)`. Repeating the bind returns the same
active binding ID. `binding(id)` reopens it after application restart. The handle
exposes `reference`, `status`, `events(after, limit)` and idempotent `detach`.

The reference contains the installation revision and the recorded start cursor.
A later SessionStart for the same native ID invalidates it, including conservative
invalidation on resume/compaction. Install/update/removal also invalidates existing
bindings, even when the configuration bytes happen to be identical. Status is
Observed, Ended, Stale or Detached. Ended observations remain readable; Ended is
not proof of a successful Turn completion. Reopening never reactivates a detached
handle. Detach does not remove hooks, delete history or signal a native process.
Dropping a Rust handle releases its borrow; there is no native resource to stop.

Session pages reuse installation cursors. The limit bounds scanned records: a
page can be empty while advancing past another session's events. Keep reading
until the cursor stops advancing. Stale/detached handles return typed errors.
Schema 3 retains older events with an absent installation revision; they cannot
establish a new binding until a new SessionStart is observed.

Hook consumers should resolve `session_for_hook(&request)` before binding: it
checks the invocation scope as well as the native session ID. Pi wire v2 uses a
fresh extension-issued UUID at every `session_start`; shared journal transactions
reject old invocations, reused start identifiers and hooks after shutdown. The
SDK rechecks the scope when committing a callback decision. Pi installations must
include SessionStarted. Other adapters expose `invocation_id: None` and retain
their unqualified delayed-hook identity semantics.

Schema 5 preserves historical rows and prevents older readers from ignoring the
new scope. Upgrade consumers together; update Pi's owned extension through
plan/apply between sessions and restart it. Wire v1 is rejected. The UUID is not a
credential, process authentication or liveness attestation. A previously unseen
delayed start and native event ordering still need qualification. Binding alone
does not provide exclusive workspace admission, a transport or permission to send.

### Library-only example

Native hook failures keep empty stdout and exit status 2. Their stderr includes
a bounded fixed diagnostic code, for example `owned_context_invalid`, `state_busy`,
`configuration_conflict` or `session_replaced`. These codes identify a local
failure category; they do not establish caller authenticity or authorize replay.
Raw error details, caller payloads and database messages are not included.
Callback failure/timeout still returns the existing blocking decision.

The diagnostic change has 162 passing Linux SDK integration tests, including
fixed-code privacy and failure-status checks. SDK library Clippy completes with
101 existing warnings; this is not a strict clean lint pass or new native proof.

Build `cargo build -p aikit-sdk --no-default-features --features integration --example integration_hooks`.
The resulting executable offers these operations:

```text
integration_hooks plan STATE                   # InstallSpec JSON on stdin
integration_hooks plan-unbounded STATE         # same spec; explicit completion policy
integration_hooks apply STATE PLAN_ID
integration_hooks remove-plan STATE INSTALLATION_ID
integration_hooks status STATE INSTALLATION_ID
integration_hooks events STATE INSTALLATION_ID
integration_hooks hook STATE WORKSPACE BLOCKS [deny-tools]
integration_hooks bind STATE INSTALLATION_ID NATIVE_SESSION_ID
integration_hooks binding-status STATE BINDING_ID
integration_hooks detach STATE BINDING_ID
```

For this example, set application ID `sdk-example`, agent key `claude`, executable
to the absolute built example path, and arguments to `hook`, absolute state path,
absolute workspace path, and a block count. Register the events needed by the
scenario, including SessionStarted. The example binds to that observed start and
blocks the first BLOCKS completion proposals in its session-scoped replay, then
allows a nonempty final answer. This is a qualification gate, not
a review policy. Append `deny-tools` to the handler arguments to block every
BeforeTool request for a native denial check. An external caller starts the
native agent.

Native qualification and its limits are recorded in
[`integration-qualification.md`](integration-qualification.md).

### Pi extension hooks

The `pi` key installs one generated `.pi/extensions/aikit-<installation-id>.js`
file. Its path is scoped to application, provider and workspace. Plans persist
desired bytes outside the worktree; apply writes the file, and removal deletes
only an unchanged owned file. Unrelated extensions/settings remain untouched.
Schema 4 protects generated-source receipts and deletion plans from older readers;
existing JSON installation plans and receipts migrate without losing ownership.
Do not downgrade the integration state database.
Owner maintenance can use the explicit, existing-only
[`IntegrationService::upgrade_existing`](docs/integration-state-upgrade.md)
for schemas 9/10/11 to 12. Runtime `open_existing` still refuses implicit upgrades;
metadata migration neither qualifies native capabilities nor updates hooks.
Schema 12 adds [candidate native append checkpoints](docs/message-append-checkpoints.md)
to retain stable pre-storage row identities independently of qualified receipts.

Pi loads this extension through its own project-trust/resource lifecycle. Reload
or restart explicitly between sessions after installation/update/removal; removing
a file does not revoke code already loaded into Pi. Configured status checks file
ownership, not actual execution or effective settings. The library never starts Pi.

The bridge registers Pi events and invokes the configured native executable with
literal argv, bounded JSON stdin/stdout and a process timeout, without a shell.
Use `hook-pi STATE WORKSPACE BLOCKS [deny-tools]` with the library example. Pi uses
Node internally; this adds no Node runtime requirement to Codelaya's own binary.
SDK development tests for the bridge require Node on PATH.

All eight current HookEvents have translations: session_start, input, tool_call,
tool_result (using its isError flag), agent_before_settle, correlated failed
agent_settled, and session_shutdown. Failure settlement uses the same session's
latest pre-settlement outcome and never assumes every settlement failed. Completion
Block appends a custom-message reason and asks for continuation; Allow preserves
other extensions' decisions. Every repeat runs application policy again. Native
tool results are never rewritten by observation. `tool_effect` translates native
`write` and `edit` inputs into `ReplaceFile` and `EditTextBatch` intent. Batch
replacements are matched against the original content. `ToolEffect::path()` and
`expected_content(before)` let applications predict exact bytes from their own
immutable baseline without copying provider parsing or replacement logic.
Compare the observed result before attributing an edit; these methods execute
no tool, read no file, and establish no authorship by themselves.

Pi's native fuzzy matching, newline/BOM normalization and path expansion are not
reimplemented. Exact prediction rejects missing/ambiguous/overlapping matches;
a differing native result must remain unattributed. Expanded paths (`@`, `~`,
file URLs, special spaces and Windows shell drive forms) return Unknown, as do
other Pi tools. Support for those semantics remains unfinished.
`EditTextBatch` is an additional public enum variant: exhaustive consumers must
handle it when upgrading. Existing serialized variants and fields are unchanged.

An opt-in [native qualification harness](examples/pi-qualification/README.md)
exercises the installed Pi loop with Pi's deterministic provider. Pi 1.0.4 on
Windows print mode passed repeated completion blocking, native tool allow/deny,
tool failure and provider failure. This bounded path does not qualify handler
ordering, general settlement correlation, subagents or deadline guarantees.
Later boundary handlers can override a
continuation and non-runnable context can prevent it. Accepted completion and
existing-session messaging are still unavailable. See `integration-qualification.md`
for the exact inspected upstream revision and remaining enhancement requirements.

### Codex hooks

The `codex` catalog key supports SessionStarted, InputSubmitted, BeforeTool,
AfterTool, CompletionProposed and SessionEnded in project `.codex/hooks.json`.
ToolFailed and CompletionFailed reject the whole plan before any configuration
change. A specification including SessionEnded must use a timeout of 1-3 seconds,
matching that event's native limit. The example uses `hook-codex` with the same
STATE/WORKSPACE/BLOCKS arguments as `hook`.

Command transport shares the Cursor literal executable/argv serializer and its
native-process byte-stream regression. The SDK writes no trust records and never
bypasses native hook review. The owner must trust the project and current hook
definitions using Codex's `/hooks` flow. Configured status only checks ownership;
it does not attest trust, effective settings or actual invocation.

Codex turn_id maps to prompt_id. Nullable Stop messages remain absent when null.
Allow returns an empty JSON object; pre-tool denial and Stop continuation use
the existing shared decision encoder. Every repeated Stop invokes policy again.
PostToolUse becomes an `AgentEventPayload::Hook` observation with AfterTool phase
and payload `{call_id, tool_name, outcome: "unknown"}`. Its arbitrary output cannot
prove success or failure, so it never fabricates a successful ToolResult. Raw
tool output/input is not retained in replay. Tool effect remains Unknown.

The adapter reuses the installer, journal and binding contracts. Native control,
effective trust, subagent attribution, accepted/failed completion, messaging and
hard deadlines remain unqualified or unavailable. See `integration-qualification.md`
before claiming a supported native workflow.

### Cursor hooks

Use the same service with catalog key `cursor`. The owned `.cursor/hooks.json`
installer supports SessionStarted, InputSubmitted, BeforeTool, AfterTool,
ToolFailed and SessionEnded. It preserves unrelated entries, checks version 1,
and sets `failClosed` on owned commands. Unsupported completion events reject the
entire plan before configuration changes. The example uses `hook-cursor` instead
of `hook`, with the same arguments; `deny-tools` exercises tool denial.

Cursor command strings use POSIX literal quoting on Unix. On Windows the SDK
encodes a constant PowerShell script that starts the native executable with exact
arguments and copies byte streams; Windows PowerShell must be available on PATH.
The additional process is included in the native timeout and does not establish
a hard deadline. A native-process test covers empty/quoted arguments, punctuation,
Unicode paths, UTF-8 streams and nonzero exit propagation. Unix execution still
needs platform qualification.

The decoder requires one workspace root matching the installation and rejects
foreign cwd/session identity. Conversation/generation IDs identify observations;
they do not establish native process identity. Tool output is decoded from the
native JSON string and omitted from replay. Tool effects remain Unknown until
Cursor edit-intent schemas are qualified. Session notifications never decide
admission or accepted completion. Stop continuation, final-answer capture and
messaging remain unimplemented; the full Codelaya requirements cannot pass.

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

## Per-user skill folders

`AgentConfig` paths are project-relative. `user_folders(key)` returns the
per-user layout of a catalog agent, relative to the home directory: its
configuration folder and every per-user skills folder it reads (its own
folder first, then the shared `SHARED_SKILLS_DIR`, `.agents/skills`, when the
agent reads it).

| Key | Config folder | Skills folders read (under `~`) |
|-----|---------------|---------------------------------|
| `claude` | `.claude` | `.claude/skills` |
| `gemini` | `.gemini` | `.gemini/skills`, `.agents/skills` |
| `copilot` | `.copilot` | `.copilot/skills`, `.agents/skills` |
| `cursor` | `.cursor` | `.cursor/skills`, `.agents/skills` |
| `qwen` | `.qwen` | `.qwen/skills`, `.agents/skills` |
| `newton` | `.newton` | `.newton/skills` |
| `opencode` | `.config/opencode` | `.config/opencode/skills`, `.agents/skills` |
| `codex` | `.codex` | `.codex/skills`, `.agents/skills` |
| `pi` | `.pi` | `.pi/agent/skills`, `.agents/skills` |
| `windsurf` | `.codeium/windsurf` | `.codeium/windsurf/skills` |
| `kilocode` | `.kilocode` | `.kilocode/skills`, `.agents/skills` |
| `auggie` | `.augment` | `.augment/skills`, `.agents/skills` |
| `roo` | `.roo` | `.roo/skills`, `.agents/skills` |
| `amp` | `.config/amp` | `.agents/skills` |
| `codebuddy`, `qoder`, `shai`, `q`, `bob` | `.codebuddy`, `.qoder`, `.shai`, `.amazonq`, `.bob` | none known |

```rust
use aikit_sdk::{detect_present_agents, fewest_skill_dirs, user_skills_dirs};
use std::path::Path;

let home = Path::new("/home/me");
// Absolute skills folders Codex reads.
let _dirs = user_skills_dirs(home, "codex");
// Agents whose per-user config folder exists (no CLI or runnability check).
let present = detect_present_agents(home);
// Fewest folders reaching every present agent, each with the agents reading it.
for (dir, agents) in fewest_skill_dirs(home, &present) {
    println!("{} -> {:?}", dir.display(), agents);
}
```

`fewest_skill_dirs` is a deterministic greedy set cover that prefers the shared
folder. An agent reading two chosen folders sees the same skill twice, so two
folders read by the same agent are only chosen when that agent's
`duplicate_skill_harmless` flag is set. The flag is enabled only after
verifying, per agent, that a duplicate is tolerated; no agent has it yet.
`detect_present_agents_for_current_user()` runs the presence check against the
current user's home directory.

## Deploy one skill entry

`deploy_skill_entry(source_dir, target_dir, id, mode)` places one skill folder
at `<target_dir>/<id>` as a symbolic link (`DeployMode::Link`), a verified copy
(`DeployMode::Copy`) or `DeployMode::Auto` (link on Unix; on Windows a link,
falling back to a copy). Other entries in `target_dir` are never touched.

- `id` must pass `is_safe_id` (a single path component).
- The new entry is built under a temporary name beside the old one and renamed
  into place, so an existing link at `<target_dir>/<id>` is replaced without
  being followed and never written through.
- A copy refuses a source containing symbolic links and is compared to the
  source (file list and bytes) after copying.

`remove_skill_entry(target_dir, id)` removes only that entry; a link is
unlinked without touching the folder it points to.

```rust
use aikit_sdk::{deploy_skill_entry, remove_skill_entry, DeployMode};
use std::path::Path;

let target = Path::new("/home/me/.agents/skills");
let deployed = deploy_skill_entry(Path::new("./my-skill"), target, "my-skill", DeployMode::Auto)?;
println!("{} via {:?}", deployed.path.display(), deployed.mode);
remove_skill_entry(target, "my-skill")?;
# Ok::<(), aikit_sdk::SkillEntryError>(())
```

## Session-start hooks

For a tool that wants to run a command whenever an agent session starts,
`session_hooks` registers that command in an agent's per-user settings, describes
the machine-wide equivalent for administrators, and formats the notice a hook
prints. Only agents whose hook format has been verified are supported
(`session_hook_agents()`); any other key is unsupported.

| Key | Per-user location (under `~`) | Machine-wide file (Linux / macOS / Windows) | Runs on | Entry identified by | Notice |
|-----|-------------------------------|---------------------------------------------|---------|---------------------|--------|
| `claude` | `.claude/settings.json` | `/etc/claude-code/managed-settings.json` / `/Library/Application Support/ClaudeCode/managed-settings.json` / `C:\Program Files\ClaudeCode\managed-settings.json` | every start (`matcher` `""`) | the id as a whole word in `command` | `{"systemMessage": ...}` |
| `gemini` | `.gemini/settings.json` | `/etc/gemini-cli/settings.json` / `/Library/Application Support/GeminiCli/settings.json` / `C:\ProgramData\gemini-cli\settings.json` | every start (`matcher` `"*"`) | `name` set to the id | `{"systemMessage": ...}` |
| `codex` | `.codex/hooks.json` | `requirements.toml`: `/etc/codex/` (Linux and macOS) / `C:\ProgramData\OpenAI\Codex\` | `startup` and `resume` | the id as a whole word in `command` | `{"systemMessage": ...}` |
| `cursor` | `.cursor/hooks.json` | `/etc/cursor/hooks.json` / `/Library/Application Support/Cursor/hooks.json` / `C:\ProgramData\Cursor\hooks.json` | a new conversation | the id as a whole word in `command` | `{"additional_context": ...}` |
| `pi` | `.pi/agent/extensions/<id>.ts` | none | `startup`, `new` and `resume` | the file's first line | plain text |

- **Claude Code, Gemini CLI, Codex**: hooks live under `hooks.SessionStart[]`,
  each group holding `{"matcher": ..., "hooks": [{"type": "command", "command": ...}]}`
  (Codex entries also carry `"timeout": 10`). Codex is refused, with nothing
  written, when `~/.codex/config.toml` already has a `hooks` table.
- **Cursor**: entries sit directly under `hooks.sessionStart[]` as
  `{"command": ..., "timeout": 10}`, with `"version": 1` at the top level; a
  file with another `version` is refused.
- **Pi**: each hook is its own TypeScript extension file, rendered whole from a
  fixed template ([ADR 0025](../docs/adr/0025-a-session-hook-can-be-a-file-aikit-owns-whole.md)).
  Its first line is `// aikit session hook <id>; generated, do not edit`; a file
  without that line is never overwritten or deleted. The extension runs the
  hook's program without a shell and shows what it prints with `ctx.ui.notify`
  when Pi has a UI. A Pi hook must be built with `SessionHook::from_argv`.

The API:

- A `SessionHook` has a stable `id` (ASCII letters, digits, `-`, `_`).
  `SessionHook::from_argv(id, program, args)` keeps the argument vector and
  derives a shell `command` from it, quoted for the platform's shell; the id
  must be one of the arguments. `SessionHook::new(id, command)` takes a shell
  command directly, which must hold the id as a whole word.
- `register_session_hook(home, key, &hook)` adds the hook, or replaces the
  existing one with the same id; it returns `HookChange::Added`, `Updated` or
  `Unchanged` (nothing written).
- `unregister_session_hook(home, key, id)` removes only that hook, then any
  group, event list or `hooks` object the removal emptied (`Removed` or
  `Unchanged`; a missing file is `Unchanged`).
- `has_session_hook(home, key, id)` checks the per-user location;
  `admin_hook_present(key, id)` checks the machine-wide file.
- `admin_hook_file(key)` and `admin_hook_entry(key, &hook)` give the
  machine-wide file for this platform and an `AdminHookEntry { format, text }`
  to place in it: `AdminHookFormat::Json` (a fragment to merge) or, for Codex,
  `AdminHookFormat::Toml` (tables to add to `requirements.toml`). Agents with
  no machine-wide file (Pi) return `None` and `Unsupported`. Administrator
  files are never written.
- `format_notice(key, text)` returns the one line a hook prints on standard
  output to show a notice, in the agent's format (table above).
- `SessionHookSupport::approval_note` is set when the agent asks the user to
  review hooks changed outside it (Claude Code: its `/hooks` menu; Codex: trust
  in `/hooks`, asked again whenever the hook changes).

Settings files are merged, never clobbered: every other key and hook keeps
its place, and a file that is not valid JSON or whose `hooks` or event list
has an unexpected type is refused untouched. Writes go to a temporary file in
the same folder and are renamed into place (pretty-printed, two-space indent,
trailing newline, existing permissions kept). A symbolic link is resolved and
its target replaced, so the link stays a link; a dangling link is refused.

```rust
use aikit_sdk::{register_session_hook, session_hook_support, HookChange, SessionHook};
use std::path::Path;

let home = Path::new("/home/me");
let hook = SessionHook::from_argv(
    "mytool-session",
    "/usr/local/bin/mytool",
    ["on-session-start", "--hook", "mytool-session"],
);
for key in ["claude", "codex", "cursor", "pi"] {
    if register_session_hook(home, key, &hook)? != HookChange::Unchanged {
        if let Some(note) = session_hook_support(key).and_then(|s| s.approval_note) {
            println!("{note}");
        }
    }
}
// Inside the hook program: print a notice the agent shows the user.
println!("{}", aikit_sdk::format_notice("claude", "mytool is ready").unwrap());
# Ok::<(), aikit_sdk::SessionHookError>(())
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
- `detect_present_agents(home)`: agents whose per-user config folder exists,
  whether or not they are runnable (see [Per-user skill folders](#per-user-skill-folders))

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
