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

The Claude/shared integration milestone had 39 passing tests on Windows. Binding
tests include reopen/idempotency, filtering interleaved sessions, empty advancing
pages, end observation, replacement under a reused native ID, unchanged-settings
reinstallation, detach without hook removal, schema-2 upgrade, and rejecting Allow
when the installation revision changes during the callback. SDK Clippy also passes.
Three additional regressions cover disabled/edited/missing owned configuration
before policy, disabling hooks during policy, and preserving unrelated setting
changes. Drift blocks with exit 2 and no prepared Allow; observations, including
delivered failure notifications, remain durable. These are local configuration
checks, not effective managed/user configuration or protection against a change
after the last check.

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

## Native pre-tool denial, 2026-10-06

On Windows x86_64 with Claude Code 2.1.269 in print mode, the built example was
installed in a new disposable workspace with all eight events and a ten-second
timeout. Its handler arguments were `hook STATE WORKSPACE 0 deny-tools`.
The external harness invoked Claude with `--tools Write --allowedTools Write`,
`--max-turns 4`, JSON output and the installed local settings. The prompt asked
for one Write creating `blocked.txt`, then `BLOCKED` if denied, without retries.

Observed: one native BeforeTool observation for Write, one matching Block, no
AfterTool or ToolFailed observations, no `blocked.txt`, and native exit 0 with
`is_error: false`, two turns and final answer `BLOCKED`. One Stop was allowed;
SessionEnd followed. All eight journal rows belong to the same native session.
Normal native Write permission was enabled, so the denial exercised the hook.
The SDK did not launch the agent; the external qualification harness did.

To reproduce, follow the installation steps above but use the handler arguments
in this section and add `completion_failed` to the installed events. No
continuation-cap override is needed. Verify the absent file, native result and
matching observation/decision together; a recorded Block alone is insufficient.
This qualifies the bounded Write denial path. Edit, shell, MCP, subagents,
interactive mode and effective settings remain outside this evidence, so the
broader PreToolDecision requirement remains Unknown.

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
- The completion test does not qualify interactive mode, BeforeTool enforcement,
  agent-owned subprocesses, sidecar outage, or native response-loss recovery.
  The separate Write denial scenario above establishes only its bounded path.
- Application callbacks must cooperate with cancellation. Filesystem and SQLite
  I/O remain subject to operating system latency; a universal hard deadline is not
  established by an async timeout.
- Cursor now has the initial subset described below. Codex and Pi external adapters
  return Unsupported. Required native capabilities, versions and platforms still
  need implementation and live evidence.
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

## Codex adapter, 2026-10-06

The initial adapter uses the existing installer, journal, observation bindings,
decision encoder and extracted command-string transport. Its six events cover
session start/end, prompt admission, pre/post tool observations and Stop proposals.
Distinct tool/turn failure events remain unsupported; no Interrupt substitution
is made. A requested unsupported event rejects the entire installation plan.
SessionEnd requires a configured timeout of at most three seconds.

The [official hook reference](https://learn.chatgpt.com/docs/hooks), retrieved on
2026-10-06, informed the adapter. The installed CLI was previously identified as
0.160.0; native execution of this adapter has not been qualified. Hook trust is
an independent native requirement. No SDK operation writes native trust records,
changes managed policy or bypasses hook review. Configured status cannot establish
that the native process will invoke a hook.

Five Codex fixtures cover owned configuration/removal and unchanged native config,
unsupported events, timeout validation, permissions/Stop responses, repeated
callbacks, nullable final messages, turn/workspace validation, outcome-unknown
PostToolUse, replay privacy, binding/detach, drift and conservative capabilities.
All 51 focused SDK integration tests pass on Windows, including the shared
native executable argv/UTF-8/exit-code bridge regression. These are local fixture
and process-transport results, not native Codex hook evidence.

PostToolUse can follow a nonzero command exit. Its arbitrary tool_response is not
a stable success discriminator: replay retains an AfterTool Hook with only
call_id, tool_name and outcome=unknown. It never emits a fabricated successful
ToolResult. Raw output is discarded. Tool-intent attribution remains Unknown.

Required native qualification includes project and exact-hook trust, command
dispatch, prompt/tool denial, repeated Stop continuation, final messages,
compaction/restart boundaries, competing hooks, subagent identity and deadlines.
Successful completion, general failed completion and existing-session messaging
remain unavailable. Codelaya must retain its complete requirements and reject
installation/admission until the missing contracts are provided and qualified.

## Cursor adapter, 2026-10-06

Reuses the existing owned installer, recoverable journal, application callback
and observation binding. The six supported events are SessionStarted,
InputSubmitted, BeforeTool, AfterTool, ToolFailed and SessionEnded. Completion
requests reject installation explicitly; the required final Codelaya workflow
remains incomplete. The official [Cursor hooks reference](https://cursor.com/docs/hooks)
and the installed Windows CLI 2026.09.02-c22c1a3 informed this mapping.

Six Cursor tests cover config ownership/version drift, decision encodings,
bindings and privacy, invalid/foreign/multiroot input, capability limits, and
command serialization. One of them compiles a tiny native Rust fixture and runs
the actual Windows PowerShell command: empty and quoted arguments, punctuation,
Unicode paths, UTF-8 stdin/stdout/stderr, and exit 23 are preserved. This regression
caught and fixed both inherited-stdin assumptions and PowerShell task-result
pollution of stdout. Test compilation requires the development Rust compiler.
This test proves the process bridge, not a live Cursor decision.

All 45 focused integration tests and SDK library/example Clippy passed on Windows.
Two bounded native Cursor print attempts returned `BLOCKED`, exited 0 and created
no target file, but neither produced a native SDK observation. A disposable input
recorder installed for the second attempt was not invoked. The attempts therefore
do not qualify native hooks or identify whether the cause is native command
dispatch, configuration loading or ordinary tool permissions. The fixture used a
workspace path containing spaces, an apostrophe, dollar sign, semicolon and Unicode.
Native execution remains Unknown; diagnose the installed Cursor dispatch boundary
before promoting this adapter. No setting or capability is relaxed to bypass it.

### Matched native dispatch diagnosis

A subsequent baseline removed the owned hooks in the same workspace and ran the
same one-Write request with stream-json output. Cursor created `baseline.txt` with
the requested content and returned `CREATED`. Reinstalling the SDK hooks and
requesting `gated.txt` produced a rejected tool result and no file. Both processes
exited 0 without timeout. This distinguishes ordinary tool permission from the
installed hook failure.

The rejected native result named a generated `ps-script-*.ps1`, line 54, with
`Missing ')' in method call` around `FromBase64String(''{1}'')`. This expression
is absent from the SDK's encoded command; the native wrapper failed before the
SDK executable. No native SDK observation was recorded. The generated temporary
script was removed by Cursor, so its complete template was not inspected.

Required upstream work: repair Cursor's Windows command-hook wrapper generation
and verify both stdin transport modes with literal punctuation/Unicode workspace
paths. Reproduce with an ordinary native hook executable that reads stdin, writes
valid permission JSON, and exits 0, retaining the enclosing shell error and the
handler invocation marker. Do not disable `failClosed` or relax workspace checks.
After repair, repeat prompt admission, tool allow/deny/failure and session replay;
the independent SDK byte-stream regression is insufficient for native support.

The exact Windows x86_64 print/version report now includes this observed failure
under PreToolDecision, which remains Unknown. A differing version, platform,
architecture or mode does not inherit this finding. The focused SDK suite now
has 46 passing tests, including this scope regression.

Current SDK reports four supported local contracts, two implemented but Unknown
native requirements, and eight Unsupported requirements for Cursor. Stop is a
follow-up mechanism; no forced completion gate or accepted completion is inferred.
SessionStart is asynchronous and can arrive after input; consumers must reject
admission without current start evidence and retry explicitly. Cloud and multiroot
sessions, subagent attribution, effective settings, native deadlines, tool-intent
normalization, completion/failure observation and existing-session messaging remain
unqualified or unimplemented.
