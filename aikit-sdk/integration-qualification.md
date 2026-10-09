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

- The Claude fixture sets an undocumented continuation-limit variable. Its effect
  was not established by the bounded run. Installation does not own that shared
  setting or attest effective managed/user configuration.
- The completion test does not qualify interactive mode, BeforeTool enforcement,
  agent-owned subprocesses, sidecar outage, or native response-loss recovery.
  The separate Write denial scenario above establishes only its bounded path.
- Application callbacks must cooperate with cancellation. Filesystem and SQLite
  I/O remain subject to operating system latency; a universal hard deadline is not
  established by an async timeout.
- Cursor, Codex and Pi now have the adapters described below. Unsupported events
  reject installation; required native capabilities, versions and platforms still
  need implementation and qualification.
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

## Pi extension adapter, 2026-10-06

The adapter was written against Pi upstream
`428a12bc775145afa342530a9eaa652efb3e4422`. The inspected extension types, runner,
agent session and session manager were compared with that exact revision.
See the [extension API](https://github.com/earendil-works/pi/blob/428a12bc775145afa342530a9eaa652efb3e4422/packages/coding-agent/src/core/extensions/types.ts)
and [runtime dispatch](https://github.com/earendil-works/pi/blob/428a12bc775145afa342530a9eaa652efb3e4422/packages/coding-agent/src/core/extensions/runner.ts).
This is source evidence. The initial host command lookup found no Pi executable;
the later isolated npm installation and native result are described below.

One owned generated project extension shares the existing plan/apply/remove journal
and session bindings. Schema 4 marks the new receipt/deletion semantics, so older
SDK readers reject this state. Tests cover planned operations without worktree
mutation, whole-file ownership, refusal to adopt unowned source, drift, unrelated
extensions, interrupted install/deletion, edited-file recovery refusal and v3 JSON
plan migration. Native project trust/loading remains outside the ownership check.
All 58 focused SDK integration tests pass on Windows; SDK library/example Clippy
also passes. The v3 upgrade regression caught and corrected an attempt to repeat
the older binding-table migration. Existing schema 1/2 migration tests remain green.

The generated JavaScript was executed by Node against a callback harness and a
real child handler. It tested literal empty/quoted/Unicode argv, UTF-8 inputs,
prompt/tool denial, three repeated completion denials, preservation of preceding
boundary entries, missing/foreign/duplicate settlement evidence, and exit-error,
malformed/empty/oversized response and timeout behavior. The initial failure was
in the fixture's argv indexing, corrected before the test passed. This harness
does not load Pi's runtime or prove its extension behavior.

Required broader native evidence: installed release compatibility beyond the
bounded scenario below, live reload/removal, parallel/nested tools and mutable tool
inputs, general final-answer capture and settlement/failure correlation, outage recovery,
subagent/session identity and process/deadline behavior. Full tool attribution
and existing-session messaging remain unfinished.

Required enhancements before enforced completion can be claimed:

- Pi's boundary dispatcher permits later handlers to replace the continuation
  decision or entries, and catches handler exceptions. An application needs an
  enforceable final decision contract or qualified exclusive-policy control.
- Pi can refuse continuation when final context is not runnable or the user
  aborts. Distinguish these outcomes from accepted application completion.
- agent_settled has no outcome/decision/invocation identifier. The bridge's
  prior-boundary correlation is local evidence only; accepted completion needs
  an authoritative outcome tied to the specific validated proposal.
- File removal does not unload a running extension. Effective installation and
  invocation identity must be established independently of the owned file hash.

Unknown requirements remain Unknown in capabilities. No native readiness claim is
made from the generated bridge, mechanical fixtures or source inspection.

### Native Pi loop with deterministic provider

The reusable [qualification harness](examples/pi-qualification/README.md) passed
six scenarios against npm `@earendil-works/pi-coding-agent` **1.0.4**, Node
**24.19.0**, Windows x86_64 build 26200, print/JSON mode, and SDK **0f9c971**.
It uses Pi's `fauxProvider` for scripted responses while exercising the actual
extension loader, agent loop, native Write tool and SDK subprocess/SQLite path.
The npm package integrity was
`sha512-+956nfMFHr5lDUVY/2Q4k+YzojzBuCaBXFgj0eSlXVGr7QVliVddKdc1Pz6yVg1dOlJQmb67doOVrlMsIcIdaw==`.

| Scenario | Native and SDK evidence |
| --- | --- |
| Write baseline without SDK hooks | Expected file content and one successful tool result. |
| Three blocked completion proposals, then Allow | Four native model calls, SDK decision sequence Block/Block/Block/Allow, `OK` captured on every proposal. |
| Write with Allow | Expected file content and one AfterTool observation. |
| Write to directory | Native failure and one ToolFailed observation. |
| Write with Block | No target file, one BeforeTool Block, model-facing error result. |
| Provider error | One CompletionFailed, no CompletionProposed, no Final Answer. |

All six processes exited zero without timeout, including the provider-error case.
The harness asserts native/SDK events rather than treating exit zero as completion.
Denied tools do not emit Pi's extension `tool_result`; consumers must use the
BeforeTool Block instead of waiting for a nonexistent ToolFailed notification.
An attempted native write that actually fails does emit ToolFailed.

Each installed scenario matched the SDK start/end records to the native session
ID. The workspace name contained spaces, an apostrophe, dollar sign, semicolon
and Unicode. A separate profile and explicit project approval isolated trust;
startup network/telemetry were disabled. SDK plan/apply updated the extension
between fresh native processes, and SDK removal left it absent. This does not
test unloading an extension from an existing process.

The harness saves streams, session-scoped journal views, version/binary hashes and
a summary only after all assertions and cleanup succeed. These checks establish
bounded native execution with deterministic model responses, not semantic model
quality, accepted application completion or full native enforcement. Capability
requirements remain Unknown/Unsupported until their broader contracts are met.

### Exact tool-intent prediction

Inspection of the installed Pi 1.0.4 `write.js`, `edit.js`, `edit-diff.js` and
`path-utils.js` established the `path/content` Write shape and original-content
batch semantics of `edits[].oldText/newText`. The SDK now translates those fields
through the existing `tool_effect` service. It accepts the native legacy single
edit and prepared array/object/JSON-string shapes without changing original bytes.

Shared `ToolEffect::expected_content` moves existing consumer exact replacement
logic into the SDK and adds disjoint original-content batches. It rejects missing,
ambiguous and overlapping matches, absent/non-UTF-8 edit baselines, and retains
Claude's replace-all semantics. It performs no filesystem operation. Consumers
must compare a successful tool's resulting bytes against this prediction.
The new public `EditTextBatch` variant requires exhaustive Rust consumers to
update; old serialized variants remain unchanged. Integration state schema and
replay privacy are unchanged; original inputs remain omitted from the journal.

These exact predictions do not reproduce Pi's fuzzy matching, Unicode/path
expansion or line-ending normalization. Path forms requiring those expansions
remain Unknown. Exact-match errors can prevent admission; differing actual bytes
cannot establish attribution. Complete Pi attribution is still an open requirement.
Three added regressions cover SDK adapter selection/replay, native input shapes,
original-offset Unicode batches, overlap/ambiguity and shared prediction rules.
All 61 focused SDK integration tests and SDK library/example Clippy pass on Windows.

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

### Windows BOM decoding and path control, 2026-10-09

Cursor 2026.09.02-c22c1a3 in Windows x86_64 print mode was tested with the
same baseline/owned-hook Write request in an ordinary workspace path. At the
merged SDK source `1e136544`, the baseline created its file, but the hooked run
returned validation-unavailable with zero SDK records. Both exited 0 without
timeout. A metadata-only stdin probe observed a leading UTF-8 BOM in all four
native hook inputs; strict JSON decoding rejected them before journaling. No
prompt, tool argument or native identity values were retained by that probe.

The decoder now consumes one leading UTF-8 BOM, after checking the original
1 MiB byte bound. It preserves BOM characters inside JSON strings. Duplicate
markers, oversize input and foreign workspace roots remain rejected with exit 2.
The two regression tests check the application callback's original tool input,
the durable Block decision and existing journal redaction. All 69 focused SDK
integration tests and the handler example build pass on Rust 1.88. Library and
handler example Clippy pass with warnings denied on installed Rust 1.96; this
host does not have the Rust 1.88 Clippy component.

With the fixed handler, the ordinary-path native baseline write succeeded; the
hooked Write was denied with one SDK BeforeTool Block among five records and no
gated file. Both processes exited 0 without timeout. Handler SHA-256:
`5E6361B66A46FFD032FD19B70572E039BDA7DAD85402C8D0A7835CE57C248642`.
Cursor entrypoint SHA-256:
`7C1957BB82B2B31F4BA53D3A9C11404FA263B31F84076E87FCB83539AC8F09A6`.

The original punctuation/Unicode path was rerun with that same handler. Its
baseline succeeded, but owned hooks again produced zero SDK records and zero
Blocks, so the probe failed as required. Use the probe's `-OrdinaryWorkspace`
option to reproduce the control; its default preserves the path stress case.
The owned hooks were removed in both runs. This qualifies only ordinary-path
native dispatch and one tool denial. Capability reports remain conservative;
prompt denial, successful tool outcomes, session replay, completion enforcement
and messaging still require their own evidence.

### Fractional native tool durations, 2026-10-09

The ordinary-path `-ToolPolicy Allow` probe initially created the hooked file
and recorded two tool Allows, but no outcome. Metadata-only forwarding probes
confirmed that native `postToolUse` for Write and `postToolUseFailure` for Read
reached the SDK with BOM-prefixed JSON and nonnegative fractional durations.
The integer-only duration decoder rejected both before journaling. Native tool
output had the documented JSON-stringified shape; payloads, native identities
and environment values were not persisted by the shape probe. Cursor's
[hook reference](https://cursor.com/docs/hooks) declares duration as a number.

The SDK now converts valid fractional milliseconds to canonical whole
milliseconds by truncating the sub-millisecond fraction. Integer values,
including u64::MAX, remain exact. Negative, nonnumeric, null and out-of-range
values remain invalid. A red regression reproduced invalid-tool-duration before
the fix. All 71 focused integration tests pass on Rust 1.88, including success
and failure observation, privacy, integer precision and invalid input cases.
Rust 1.96 library/example Clippy passes with warnings denied.

A fresh uninstrumented native Allow probe passes: baseline and hooked Write
files exist, one Write outcome matches the allowed session/call, two tool Allows
and zero Blocks are recorded. Eight SDK records include the failed preliminary
Read and the successful Write. Both processes exit 0 without timeout; owned
hooks are removed. The native-tested handler SHA-256 is
`44C08BCFCD4F645317A2181652344653E67252E3F8D6506220FA6B8DC9BD74BA`;
subsequent Clippy cleanup changes only bool::then to equivalent bool::then_some.
The installed CLI digest remains the one recorded above. Earlier failed probes
remain retained. This closes the observed duration-decoding loss for these two
native tool outcomes, not completion settlement, all edit paths or messaging.

### Repeatable fresh probe, 2026-10-07

`examples/cursor-qualification/run.ps1` now provides a matched, bounded Windows
probe with an explicitly supplied Cursor installation and current library-only
SDK example. A fresh workspace at SDK source 972807b reproduced the same native
line-54 ParserError on Cursor 2026.09.02-c22c1a3 print mode. The no-hook write
succeeded; the owned-hook run produced no file and zero SDK observations/Blocks.
Both native processes exited 0 without timeout. The probe exited 1 as required
and its removal path left an empty hooks configuration.

The example build passed on Windows Rust 1.96 with the local locked dependency
graph. Its SHA-256 was
`A956A635D964B5108738BA9D7E61C1A852D2A0A8AA264D667B16A85D0EF21B99`;
the supplied Cursor entrypoint digest was
`7C1957BB82B2B31F4BA53D3A9C11404FA263B31F84076E87FCB83539AC8F09A6`.
This refresh strengthens the recorded failure's reproducibility and does not
promote capabilities or establish Rust 1.88/native completion qualification.

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

## Pi invocation scope, 2026-10-06

Wire v2 adds a fresh extension-issued UUID at each session_start. The shared SDK
stores it on HookRequest and SessionRef, validates it inside journal transactions,
and rechecks it after policy callbacks. Binding replay and end-state detection
use the same scope. Reused start IDs, late input/tool/end hooks from an observed
older invocation, and hooks after its recorded shutdown are rejected. Consumers
use session_for_hook before admission; observed_session remains the latest-start
lookup for explicit inspection. No provider decoder belongs in the consumer.

Schema 5 preserves historical journal/binding rows with absent invocation evidence
and rejects readers that would ignore this field. Upgrade the SDK and consumer
together. Update Pi's generated extension using the existing plan/apply lifecycle
between sessions, then restart it; wire v1 fails closed. SessionStarted is now a
required event for Pi installations. Historical bindings remain readable without
inventing an invocation ID.

The focused SDK suite passes 65 tests on Windows, including schema-4 migration,
repeated native session IDs, stale notifications, replacement during an awaited
callback, new binding replay, restart persistence, and Node process bridge scope
rotation. Library/example Clippy passes. The six-scenario native harness was rerun
with Pi 1.0.4 and Node 24.19.0 in Windows print mode: baseline/allowed/denied/failed
writes, repeated blocked completion followed by Allow, and provider failure all
passed with one consistent nonempty invocation ID per run. These native scenarios
use the real Pi loop/tools and the deterministic qualification provider.

The UUID is an extension invocation scope, not a credential or authenticated OS
process identity. A previously unseen delayed SessionStart can still become the
latest observed start; no causal ordering beyond the journal is claimed. Native
concurrency/resume/extension reload, competing handlers and interactive mode need
qualification. NativeSessionIdentity is implemented but remains Unknown. No
message transport or successful-completion evidence is added by this change.

## Pi completion boundary counterexamples, 2026-10-06

The expanded native harness passed eight scenarios on Pi 1.0.4 / Node 24.19.0 /
Windows x86_64 print mode using SDK fff6aa7. Six retain the earlier behavior;
two reproduce native contract gaps. A later extension aborted after a recorded
SDK Allow, but settlement still carried only `type`, matching normal completion.
No current abort signal was available at either boundary. A second scenario
returned continue:false after observing the SDK Block and continue:true; Pi
settled after one model call without an Allow. The probe queries actual SDK
journal evidence before acting, so the scenario cannot pass with the wrong order.

The two probes explicitly load the installed SDK source once before the competing
extension. Earlier setup attempts failed their assertions: one placed the probe
before the SDK; another loaded the SDK twice through Windows path aliases and
was stopped by invocation guards. Those attempts are not boundary evidence. The
six ordinary scenarios still use native project auto-discovery. All fixtures and
raw evidence stay outside the repository.

These findings are exposed only in the matching version/platform/architecture/mode
capability report. RepeatedCompletionBlocking stays Unknown and
SuccessfulCompletionObservation stays Unsupported. Other contexts inherit neither
the experiment nor support. Required native proposal/outcome/gate contracts and
loader deduplication are documented in `integration-enhancements.md`. Do not add
an inferred accepted-completion event to bridge this missing native information.

### Native Pi message queue candidate

The messaging-only qualification mode passed steer, followUp and missing-mode
scenarios twice on Windows Pi 1.0.4 / Node 24.19.0 on 2026-10-07. Positive modes
reach the next deterministic model context once; all API calls return undefined,
including the asynchronously rejected missing-mode case. See the harness README
and integration-enhancements.md for reproduction and the acknowledgement gap.
SDK existing-session messaging remains unavailable; no capability was promoted.
The tests qualify native queue behavior in a trusted fixture, not operation-level
receipts/reconciliation, identity, crash/restart or queue lifetime.