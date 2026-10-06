# External hook integration qualification

Evidence recorded 2026-10-06. This covers the SDK installation, hook and existing
session observation-binding slice. Message delivery, native process-identity
qualification and application review policy remain pending. No agent is launched
by `IntegrationService`.

## Local mechanical checks

Run from the repository root with its supported Rust toolchain:

```text
cargo test -p aikit-sdk --no-default-features --features integration --lib integration:: --locked
cargo test -p aikit-sdk --no-default-features --features integration --lib mcp_deploy:: --locked
cargo clippy -p aikit-sdk --no-default-features --features integration --all-targets --locked -- -D warnings
cargo test -p aikit-cli gateway:: --lib --locked
cargo build -p aikit-sdk --no-default-features --features integration --example integration_hooks --locked
```

Windows checks used Rust 1.96.0 (`cargo +1.96.0`). Installation tests exercise
configuration preservation, stale plans, ownership collisions, interrupted writes,
interrupted removal, unsafe input and schema upgrade. Hook tests cover native
responses, repeated decisions, timeout/error/panic blocking, malformed requests,
append-only cursors and omission of tool contents from replay. Existing MCP and
managed gateway tests establish compatibility within their tested scope.

The current focused integration suite has 36 passing tests on Windows. Binding
tests include reopen/idempotency, filtering interleaved sessions, empty advancing
pages, end observation, replacement under a reused native ID, unchanged-settings
reinstallation, detach without hook removal, schema-2 upgrade, and rejecting Allow
when the installation revision changes during the callback. SDK Clippy also passes.

The additional tool-intent fixture covers exact Write/Edit fields, raw content,
replace-all semantics and malformed inputs. `IntegrationService::tool_effect`
normalizes intent without touching files or claiming a successful edit. Shell,
MCP and unknown tools remain Unknown. Native tool attribution still requires
application evidence and live qualification.

## Live scenario

| Property | Observed configuration/result |
| --- | --- |
| Provider/version | Claude Code 2.1.269 |
| Platform/mode | Windows, native executable, `-p` print mode |
| Hook handler | Built `integration_hooks` example; SDK exec-form installation |
| Native limit setting | Workspace fixture sets `CLAUDE_CODE_STOP_HOOK_BLOCK_CAP=0` |
| Installed events | SessionStarted, InputSubmitted, BeforeTool, AfterTool, ToolFailed, CompletionProposed, SessionEnded |
| Application callback | Block first ten completion proposals per session; require nonempty final answer |
| Result | Process exit 0; result success; eleven turns; final response `OK` |
| Journal | Eleven completion observations, ten Block decisions, one Allow decision |
| Tool observations | Zero |

The initial hook scenario passed twice. A third run used the schema-3 implementation
and the real `SessionBinding` inside the callback. It produced the same counts and
result. An external example caller reopened the active binding during the run and
observed status Observed; after SessionEnd it reported Ended. All 26 records for
that run carried the current installation revision.
Count observations separately from decisions, and filter by the native session ID
when a state directory contains earlier runs. The example's prepared decision
records alone do not prove native action: the outer native process result and
repeated hook invocations establish this scenario's result.

### Reproduction

1. Create a disposable workspace and a separate private state directory. Write
   `.claude/settings.local.json` with
   `{"env":{"CLAUDE_CODE_STOP_HOOK_BLOCK_CAP":"0"}}`.
2. Build the example. Supply an `InstallSpec` to its `plan STATE` command:
   application ID `sdk-example`, key `claude`, absolute workspace and executable
   paths, arguments `["hook", STATE, WORKSPACE, "10"]`, all seven events from the
   table in snake_case, and `timeout_seconds: 10`.
3. Inspect the returned plan and apply its ID. Check `status` reports Configured.
4. From an external shell in the disposable workspace, start:

   ```text
   claude -p "Reply with exactly OK. If a hook asks you to continue, reply OK again. Do not use tools." --settings .claude/settings.local.json --max-turns 15 --output-format json
   ```

5. Read `events STATE INSTALLATION_ID`. Verify the native result and journal counts
   above for that session, plus nonempty `request.final_answer` on Stop proposals.
6. Use `remove-plan` and `apply` to remove the owned hooks. The existing environment
   setting must remain. Keep the disposable state/output for inspection as needed.

## Limits and required follow-up

The example's fresh version-only probe on this Windows x86_64 host returned
Claude `2.1.269 (Claude Code)`, Cursor `2026.09.02-c22c1a3`, Codex `codex-cli
0.160.0`, and Pi `binary_not_found`. Claude's print-context report had seven scoped
Supported contracts, three Unknown guarantees and four Unsupported operations.
The other external hook adapters remained Unsupported. Detecting installed Cursor
or Codex does not implement their external adapters or qualify native behavior.

- Contextual reports now distinguish SDK implementation, supported scoped contracts,
  unsupported operations and unknown native qualification. Tests reject changed
  version, platform, architecture and mode, missing probes, absent requirements
  and borrowing managed-runner flags. Requirement-aware binding rejects before
  persisting a handle. Twelve availability tests cover the reused probe path and
  cache behavior, including bounded version evidence. The SDK example exposes the
  report without launching an agent session.
- A subsequent Windows x86_64 Claude 2.1.269 print-mode invalid-model check through
  the external-hook consumer emitted CompletionFailed at cursor 4, with no tools,
  Stop or SessionEnd and no Final Answer. Native exit was 1 and is_error was true
  despite the result subtype being success. The consumer recovered its application
  state through journal replay. This bounded scenario is the failure-observation
  evidence in capability reports, not proof of successful completion.

- The added CompletionFailed/StopFailure contract is covered by a native-shaped
  fixture: configuration, notification-only replay, no error text promoted to a
  Final Answer, and a session binding that remains observed. This fixture does not
  itself qualify a failure emitted by a live native process; the separate bounded
  native scenario above supplies that evidence. Existing installations
  must be explicitly updated to include the new event.
- Workspace formatting checks on Windows report existing CRLF differences and
  unsupported rustfmt options. Touched integration files are formatted separately;
  unrelated source formatting is preserved.

- The fixture configures the native continuation limit explicitly. Installation
  does not own that shared setting or attest effective managed/user configuration.
- This test does not qualify interactive mode, BeforeTool enforcement under real
  edits, agent-owned subprocesses, sidecar outage, or native response-loss recovery.
- Application callbacks must cooperate with cancellation. Filesystem and SQLite
  I/O remain subject to operating system latency; a universal hard deadline is not
  established by an async timeout.
- Cursor, Codex and Pi external adapters return Unsupported. Their required native
  capabilities, versions and platforms still need implementation and live evidence.
- The binding is based on installation revision plus observed SessionStart cursor.
  It detects recorded replacement/resume boundaries, but cannot identify an old
  delayed hook if the native payload reuses an ID without an invocation epoch.
  Process liveness, exclusive workspace admission and control capability are not
  established by a binding. Native messaging/reconciliation remain pending.
- Detach revokes the persisted application handle and sends no provider command.
  Mechanical tests preserve installed config/history and continue processing hooks
  after detach. Live interactive detach/process-identity qualification is pending.
  Managed gateway transports cannot substitute for user-started topology.
- Removal does not stop an agent. SessionEnd is an observation, not accepted Turn
  completion. Tool argument/result contents are deliberately absent from replay.

Protocol references: [Claude hooks](https://code.claude.com/docs/en/hooks) and
[Claude environment variables](https://code.claude.com/docs/en/env-vars).
