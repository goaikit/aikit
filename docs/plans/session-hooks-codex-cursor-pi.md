# Session-start hooks for Codex, Cursor and Pi

Status: planned. Date: 2026-10-17. Decision: [ADR 0025](../adr/0025-a-session-hook-can-be-a-file-aikit-owns-whole.md).

## Problem

`aikit-sdk::session_hooks` registers a session-start hook for two agents, Claude Code and
Gemini CLI. Both keep hooks in the same JSON shape (`hooks.SessionStart[].hooks[]`), and the module
assumes that shape. FastSkill, the first caller, wants the hook for four agents: Claude Code,
Codex, Cursor and Pi. Codex fits the current shape; Cursor uses a different JSON shape; Pi has no
command hooks at all, only TypeScript extensions.

## Solution

Three new rows in the hook table, two new ways of holding a hook, and per-agent notices. Every
public function keeps its name and meaning, so callers that iterate `session_hook_agents()` pick
the new agents up without code changes, apart from building hooks from an argument vector.

### What each agent supports

Checked against each agent's documentation on 2026-10-17. Each row is enabled only after the
manual check in "Verification" passes on a real install, as for the existing rows.

| | Codex CLI | Cursor | Pi |
|---|---|---|---|
| Event | `SessionStart`; matcher `startup\|resume` (it also fires on `clear`, `compact`, `fork`) | `sessionStart`, on each new conversation; not awaited | `session_start` extension event; reasons `startup`, `new`, `resume`, `fork`, `reload`; handlers are awaited |
| User file | `~/.codex/hooks.json` | `~/.cursor/hooks.json` | `~/.pi/agent/extensions/<id>.ts` |
| Shape | `hooks.SessionStart[].hooks[]` with `matcher`, as Claude Code | `{"version": 1, "hooks": {"sessionStart": [{"command": …, "timeout": …}]}}` | A whole TypeScript file (ADR 0025) |
| Entry name field | none | none | the file name |
| Approval | a new or changed user hook runs only after the user trusts it in `/hooks`; trust is tied to the hook's hash | none for user hooks | none for user extensions; project extensions need project trust |
| Administrator level | `[hooks]` in `/etc/codex/requirements.toml` (`%ProgramData%\OpenAI\Codex\requirements.toml` on Windows), scripts in `managed_dir`; trusted by policy | `/etc/cursor/hooks.json`, `/Library/Application Support/Cursor/hooks.json`, `C:\ProgramData\Cursor\hooks.json` | none |
| Output | plain stdout becomes model context; `{"systemMessage": …}` is shown to the user as a warning | only JSON `additional_context`, which reaches the model, not the user | the extension decides; `ctx.ui.notify` shows the user and adds nothing to the model context |

### Hook rows

Each row gains a `layout` that says how the hook is held:

- **`Grouped`**: the current behaviour, a group under `hooks.SessionStart` with a `matcher`.
  Claude Code (matcher `""`), Gemini CLI (`"*"`) and Codex (`"startup|resume"`).
- **`Flat`**: an entry directly in `hooks.sessionStart`, with `version: 1` at the top level. A new
  file gets `version: 1`; a file whose `version` isn't `1` is refused. Cursor.
- **`OwnedFile`**: one file per hook id that aikit writes whole (ADR 0025). Pi.

Rows also carry the event key (`SessionStart` or `sessionStart`), the hook timeout (10 seconds
where the agent takes one), and the approval note. Codex's note: "Codex runs the new hook only
after you trust it in `/hooks`. It asks again whenever the hook changes."

The writing-safety rules of the module apply to `Grouped` and `Flat` unchanged: merge into the
existing file, keep every other key in order, write atomically, resolve symbolic links, refuse
anything that doesn't parse as the expected shape.

### Codex: hooks in `config.toml`

Codex also reads inline `[hooks]` tables in `~/.codex/config.toml`, and warns when one layer uses
both files. When `~/.codex/config.toml` defines hooks, `register_session_hook` refuses
with a new `SessionHookError::OtherHookFile` naming that file, and writes nothing. Defining hooks
means a `hooks` key holding anything but `hooks.state`: that is where Codex keeps the trust
records `/hooks` writes (`[hooks.state."<file>:session_start:0:0"] trusted_hash = ...`, seen on
Codex 0.163.0), so it is there on every machine whose user trusted our hook. aikit doesn't
write TOML in this change.

### Pi: the extension file

`register_session_hook(home, "pi", hook)` writes `~/.pi/agent/extensions/<id>.ts` from a fixed
template. The file:

- starts with the marker line `// aikit session hook <id>; generated, do not edit`;
- imports nothing, so Pi loads it with no packages installed;
- registers a `session_start` handler that, for reasons `startup`, `new` and `resume`, runs the
  hook's program with its arguments through `pi.exec(program, args, { timeout: 10000 })`. It
  passes no shell, so nothing needs quoting;
- shows a non-empty trimmed stdout with `ctx.ui.notify(text, "warning")` when `ctx.hasUI`;
- catches every error, so a missing program never breaks the session.

The program and arguments are embedded as JSON string literals. A file at that path without the
marker line is refused (`SessionHookError::NotOurs`) and left as it is. `unregister_session_hook`
deletes the file only when its marker names the same id. `has_session_hook` checks the marker.
The rendered file is a pure function of the hook, so registering the same hook twice reports
`HookChange::Unchanged`.

aikit uses the default agent folder, `~/.pi/agent`. A user who sets `PI_CODING_AGENT_DIR` gets no
hook; that is recorded as a known gap, not handled.

### Hooks built from an argument vector

Pi runs a program with arguments, not a shell command. `SessionHook` gains
`SessionHook::from_argv(id, program, args)`, which keeps `program` and `args` and derives
`command` by quoting them for a POSIX shell (and for `cmd.exe` on Windows). The id rule still
applies: one of `args` must equal the id. Registering a Pi hook built with only a `command`
returns `SessionHookError::NeedsArgv`.

### Administrator entries

`admin_hook_entry` returns a new `AdminHookEntry { format, text }`, where `format` is `Json` or
`Toml`, instead of a bare `serde_json::Value`:

- Claude Code, Gemini CLI and Cursor: the JSON fragment to merge into `admin_hook_file`, in each
  row's shape.
- Codex: the TOML `[[hooks.SessionStart]]` fragment for `requirements.toml`.
- Pi: `Err(Unsupported)`, because Pi has no administrator-level location. `admin_hook_file("pi")`
  is `None`.

`admin_hook_present` reads Codex's `requirements.toml` as TOML. This is a breaking change to one
function; FastSkill is its only caller and moves in the same release.

### Notices

`format_notice(key, text)` returns, per agent:

- Claude Code, Gemini CLI, Codex: `{"systemMessage": "<text>"}`.
- Cursor: `{"additional_context": "<text>"}`. Cursor shows the user nothing; the text reaches the
  model only.
- Pi: the plain text, which the extension passes to `ctx.ui.notify`.

## Testing

The seam stays the module's public functions over a temporary home directory, as in the existing
`session_hooks` tests. For each new row:

- register into a missing file, an empty file, a file with other hooks, and a file of the wrong
  shape (refused, untouched);
- register twice (`Unchanged`), change the command (`Updated`), unregister (`Removed`), unregister
  again (`Unchanged`);
- Codex: `config.toml` that defines hooks is refused; Codex's own `hooks.state` trust
  records are not;
- Cursor: `version` other than `1` is refused;
- Pi: the rendered file for a known hook matches a checked-in golden file; a file without the
  marker is refused and never deleted; a marker naming another id is left alone;
- `admin_hook_entry` per row against golden fragments; `format_notice` per row.

A smoke test runs the golden Pi file through `bun build --no-bundle` to check it parses as
TypeScript, without a Pi install.

## Verification

Before a row is enabled, on a machine with the agent installed:

1. register the hook for a test program that appends a line to a file;
2. start a session, then resume one, and check the line was appended each time;
3. for Codex, check `/hooks` asks for trust, and asks again after the command changes;
4. make the program print a notice and check where it shows;
5. record the agent version in the row's doc comment.

## Out of scope

- Hooks for any other agent, including GitHub Copilot.
- Writing hooks into Codex's `config.toml`, or into any administrator file.
- Pi installs that use `PI_CODING_AGENT_DIR`.
- Making a skill visible in the session whose hook deployed it.
