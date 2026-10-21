# Recorded-context message preparation RPC

## Opening retained integration state

`IntegrationService::open_existing(state_directory)` opens the SDK's existing
journal without creating a replacement state directory/database. Missing state
returns NotFound; empty/unversioned state is refused. Subsequent connections
through that handle also disable SQLite CREATE, including if the database is
removed after construction. An older schema is refused without changing its
version; owner-controlled `open` can initialize or upgrade before existing-only use.
This is not a read-only or authorization boundary: normal SDK mutations remain
available against the existing journal. Initial owner-controlled setup continues
to use `open`, which may create fresh state. Consumers need no SDK filename or
schema knowledge to select between those construction modes.

Tests cover missing directory, missing/empty journal, removal after construction
and refusing an implicit version-9 upgrade before preserving owned receipts through
an explicit owner-controlled version-11 upgrade.
The Windows integration gate passed 110 cases and scoped Clippy passed with the
existing uninlined-format allowance. Offline Linux passed 112 integration cases
serially, compiled both bridge/hook embedding examples and passed the 14 unchanged
Node cases. These checks do not establish native readiness.

## Local writer contention

`Inspect` uses a deferred SQLite read snapshot and never takes the SDK writer
file lock or reserves a message. `Poll` and `Recheck` retain their existing
exclusive recorded-state checks. Retaining an already-returned `RecordHandoff`
may overlap the resulting native Turn-start writer; only that local lock
acquisition has a 250 ms wait budget. It sleeps between contention checks and
executes the transaction body once after acquisition. It does not repeat native
I/O, retry semantic validation, retarget a message or change hook deadlines.
Other SDK writers retain their existing immediate refusal behavior.

Exhausting the local wait preserves Dispatching, with no invented raw receipt or
proof. Existing explicit/passive recovery must conservatively retain Unknown;
the module still stops rather than retrying native submission. This is bounded
local journal coordination, not protected native admission or delivery proof.
Tests hold the writer while inspecting, refuse Poll, exhaust raw retention's
budget, release a competing writer, and verify exact idempotent raw retention
with no second reservation. Windows integration gate: 107 cases passed, with
scoped library/bridge-example Clippy using the existing uninlined-format allowance.
Offline Linux: 109 integration cases passed serially (plus the separately run
gateway recovery case, which overlaps that suite), both embedding examples
compiled and 14 unchanged Node behavior tests passed. The first new Rust test
compile used a misspelled fixture helper and is retained as failed evidence;
the corrected Windows/Linux gates passed. Native qualification remains separate.

`IntegrationService::handle_recorded_message_request(installation_id, reader)`
provides bounded stdin/stdout preparation for an owned module's fixed command.
The `integration_message_bridge` embedding example delegates directly to it.
The Rust handler calls no native API and grants no message-submission capability.
An explicit opt-in generated Claude module now polls, checks an immutable ticket,
calls the native prompt API once and retains its raw return through this handler.
This is a qualification candidate: actual native admission, qualified acknowledgement/
consumption and Codelaya orchestration remain open. Default tools have no pump.

## Envelope and scope

The version-1 request contains application/installation/agent keys, a
`NativeToolContext`, and tagged action `inspect`, `poll` or
`recheck { operation_id, reservation_id }`, or
`record_handoff { operation_id, reservation_id, outcome }`. Unknown fields, foreign scope,
invalid versions/contexts and envelopes above 128 KiB are refused. There is no
database path, owner identity, arbitrary program or selected binding in the frame.

The caller supplies the trusted expected installation ID from fixed configuration.
The SDK reads only that installation receipt's inherited environment key. It
compares the envelope with inherited module claims, canonical workspace and
current recorded invocation. These are correlation checks, not authentication
of a physical native caller. Required native identity/admission remains unqualified.

The installation must have owned tool files and input observations. A native
module can start before the application's first binding: `poll` then defers
without attaching, reserving or claiming idle readiness. An active application
binding is required to issue/recheck a ticket or retain a handoff. An explicitly
detached binding refuses rather than being treated as a new waiting attachment.
Polling cannot attach or revive a detached binding.

`inspect` runs these same inherited-context checks and reads recorded idle state
without creating a binding/reservation or calling an application/provider. It
lets an embedding application check its own admission before invoking `poll`.
Configured-file drift, replaced generation and unavailable bindings refuse work.
No provider settings, credentials or owner state were changed by fixture tests.

## Recorded idle and one reservation

Inside the existing short installation-lock/SQLite transaction, the latest
relevant main-agent input/tool/proposal/Turn observation must be a native Turn
finish with an outcome. Missing/pending/newer input or tool work defers. This is
only a journal snapshot: a native action can start after the transaction returns.

`poll` reserves the oldest Accepted record for this exact binding. A Dispatching
operation prevents a second ticket. Already-reserved/Unknown records are never
returned for resend. The RPC supports a 64 KiB payload ceiling, below the journal's
4 MiB storage ceiling; excessive content refuses before reservation or handoff,
without truncation. The existing journal performs the atomic sole reservation.

`recheck` compares the current Dispatching operation/reservation with the same
recorded scope and idle snapshot. It holds no lock over I/O and does not return
the payload again. A lost attempt changed to Unknown never becomes ready again.
Wrong reservation IDs conflict. When rechecked while idle, the optional
`recorded_state` identifies that exact attempt. Unknown/Failed/Acknowledged retire
the module's local ticket without sending/resetting it; the next polling tick
may handle a different queued operation. Accepted after reservation is an
inconsistency and stops the pump. Older replies without this optional field
retain their conservative waiting behavior. No wire-version/schema migration or
readiness promotion is introduced.

The version-1 reply contains the exact `SessionRef`, optional reservation and
explicitly named `recorded_idle`/`recorded_ready`. A successful check is **not a
native admission lease or permission to send**. The future adapter must qualify
admission through the real handoff, and preserve Unknown when no safe proof is
available. No high-level native `submit_message`/`reconcile_message` is claimed.

## Validation scope

New synthetic journal/RPC cases cover busy deferral, input arriving after a
reservation, single polling ticket, no readiness after Unknown, original scope,
missing/foreign inherited claims, missing owned module, generation replacement,
detach without reattachment, excessive payload before reservation and malformed
frame/version/state-path rejection. The fixtures supply synthetic owned-module
starts; the initial classic-start fixture was corrected after it correctly
deferred without inherited context. This is not native loader evidence.

The existing CLI example is an embedding seam, not a usable agent transport.
It requires an already installed, journaled and application-bound session; it
does not create one. Current Codelaya/package SDK pins and readiness are unchanged.
Polling can reserve an operation, so it is not a read-only status command. Use
`message_status` for inspection. The owner initializer can upgrade it to schema 12;
do not run the example against an older live consumer's state before a coordinated
upgrade. Development tests touched only disposable fixture state.

Final validation: native Windows Rust 1.88 passed 100 integration tests; isolated
offline Linux passed 102 after a fresh source copy and SDK crate clean. The stdio
example compiled on both; Windows library/example Clippy passed with warnings
denied and the existing `uninlined_format_args` allowance. Provider profiles and
credentials were absent from the Linux container and external networking was
disabled. No native send/admission/receipt/consumption qualification is implied.

## Opt-in native client and raw handoffs

Schema 12 additionally provides [candidate append checkpoints](message-append-checkpoints.md)
through the reusable library. The generated client described below still retains
raw submit returns; it is not yet wired to pre/post-append checkpoint capture.

`plan_agent_tool_with_message_bridge(installation_id, spec, fixed_handler)` uses
the existing owned file plan, fingerprints and recovery/removal machinery. It
requires input observations and an existing session handler. `AgentToolSpec`
stays source-compatible; `plan_agent_tool` still renders no timer, native prompt
call or message event handler. Updating to or from the candidate changes the
owned generation, so restart the native session before qualifying it.

The fixed bridge handler must delegate bounded stdin to
`handle_recorded_message_request`. For a disposable embedding fixture, use:

```text
integration_hooks plan-tool-message STATE INSTALLATION_ID ABSOLUTE_BRIDGE_EXE STATE INSTALLATION_ID < tool-spec.json
integration_hooks apply STATE PLAN_ID
```

`ABSOLUTE_BRIDGE_EXE` is the compiled `integration_message_bridge` example.
This example does not install Codelaya or authorize a live consumer upgrade.
Hook setup, native startup, application binding and frozen staging must already
exist. Never use the owner's older live state as an example fixture.

The module uses a single 250 ms timer and one in-flight poll. Main Turn activity
defers handoff; a busy recheck retains the original ticket. It compares the exact
recorded target, invocation, native session, workspace and payload SHA-256, then
marks the attempt immediately before `$.prompt.submit({text: exactContent})`.
No await separates the last local busy/end check and that native call. Session
end disables the pump. Lost RPC/commit responses or corrupt scope/payload stop
it without retrying. Native rejection/throw is uncertain, never a proven failure.
The timer and local check do **not** establish a protected native admission lease
against competing native handlers, reload, replacement or prompt rewriting.

`record_handoff` stores a bounded raw return (at most 96 KiB of serialized JSON)
or an explicit unavailable/throw outcome with the original immutable reservation
and local timestamp. It can arrive while the newly started Turn is busy. Exact
replay retains the same timestamp; changed replay or foreign attempt conflicts.
The additive schema-11 `message_handoffs` table is separate from qualified
`message_observations`. `SessionBinding::message_handoff(id)` resolves the raw
record, including historically after detach. The embedding inspection command is
`integration_hooks message-status STATE BINDING_ID OPERATION_ID`.

Both returned and uncertain outcomes conservatively leave an unresolved dispatch
**Unknown**. They create no acknowledgement/consumption observation. Existing
independent qualified proof is never downgraded. A native throw, transformed
return, dropped prompt, unavailable raw result or lost storage response cannot
cause a second attempt. A stale/detached/replaced context refuses new RPCs; a lost
late return can therefore remain unresolved and requires historical inspection.
This is not native reconciliation. No high-level native submit/reconcile method,
consumer dependency upgrade or readiness promotion is claimed.

### Candidate validation, 2026-10-18

- Windows Rust 1.88: 103 integration tests passed, including durable raw return,
  uncertain outcome, changed replay/foreign attempt, schema-10 migration and
  owned module opt-in/removal. The sandbox attempt failed creating disposable
  temporary fixtures; the normal-context run passed. Neither run launched Claude.
- Node behavior tests: 12 passed across default and opt-in modules. Fault cases
  include overlapping timers, new main work/session end during recheck, native
  throw, changed session/target/digest, truncated RPC and lost storage response.
- Windows library and both bridge/hook examples passed Clippy with `-D warnings`
  and the existing SDK `uninlined_format_args` allowance. Cargo reported its
  existing hard-link cache fallback warning. This is not whole-workspace lint.
- Offline Linux: 105 integration tests passed serially after a fresh SDK build;
  both examples compiled and all 12 Node tests passed. The first parallel run
  passed 104 tests but failed the existing managed gateway-store restart test
  with `another host process owns this session store`. That exact test passed
  separately, then in the serial suite. Its parallel failure remains an observed
  validation limitation; neither its cause nor a production fix is established.
  Raw failing and passing logs are retained separately. The initial container
  launch used the image's unrelated default fixture entrypoint; the corrected
  run explicitly selected `/bin/sh`. No profiles/credentials were mounted and
  external networking was disabled.

Those tests used synthetic native APIs/journal generations. They alone did not
prove module loading/handoff or close the native Windows policy-service blocker.
Codelaya remains on its schema-9 pin.

## Native generated-client follow-up, 2026-10-18

The [ordinary Linux qualification](../examples/message-client-qualification/README.md)
now exercised the actual generated module, owned default project loading,
fixed stdio RPC and durable journal. The first native attempt exposed the
pre-attachment startup bug: NotFound stopped the pump before its first Turn.
That unsent Accepted operation was retained separately and was never resent.
The fix above defers initial polling without implicit attachment, while retaining
detach refusal. The test envelope had silently created a binding; it now reads
the observed reference without mutation and tests the initially empty table.

A fresh run submitted the complete immutable payload once, to its original
session. Independent outgoing-input capture found the exact payload once in the
feedback request. The native raw return contained exact text/plugin origin and
survived normal exit/reopen. Resume reused the native session with a new
invocation, without changing/resending the old receipt. A separate owner prompt
made one additional request, containing the old payload once in history. The
original Unknown record/raw return also survived a subsequent container SIGKILL
after receipt storage and completed owner work. No graceful end was synthesized.

Windows 104 and Linux 106 serial SDK integration tests passed after the fix;
scoped Windows library/example Clippy passed with the existing allowance. Six
offline evidence mutations were rejected. Native containers used only synthetic
context/public dummy key, no mounted profiles/credentials, and no external
network after trust. Both were removed after retaining evidence.

This proves this limited Linux native path and post-receipt persistence. It does
not qualify native caller authentication, protected admission, actual production
acknowledgement/consumption provenance, mid-handoff crash recovery, prompt
rewrite/drop, real-model semantics, Codelaya orchestration or Windows/provider
parity. Unknown and all capability/readiness assessments remain unchanged.

### Application callback preparation, 2026-10-18

Added non-mutating `inspect` and exact-attempt `recorded_state` so an embedding
application can enforce its own preflight before exposing a reserved ticket,
and a retired Unknown/terminal ticket does not block later distinct operations.
Windows passed 105 integration tests; offline Linux passed 107 serial tests and
compiled both bridge/hook examples. All 14 default/opt-in Node behavior tests
passed on both platforms. Scoped Windows library/bridge Clippy passed with the
existing SDK allowance. New cases are synthetic context/state-machine tests;
they do not constitute a fresh native application qualification or admission lease.
