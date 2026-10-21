# Combined SDK append crash recovery, 2026-10-21

One ordinary Linux Claude Code 2.1.296 session was SIGKILLed after the real
SDK's stored-row transaction committed but before its stdout reached the owned
module. The same native session resumed without resubmitting the old review.
This exercises the real journal/module path, unlike the earlier diagnostic-only
checkpoint helper. Starting source: AIKit `cf4adf7`, Codelaya `87648aa`.

## Executed path

The existing SDK module, fixed receiver and checkpoint implementation are
unchanged from the [combined normal-exit run](native-sdk-append-client.md).
The thin `integration_hooks message-recover` example only delegates to existing
`SessionBinding::recover_message_record`; it adds no transport/recovery logic.

The explicit `crash` fixture installs a diagnostic fixed-command wrapper around
the actual `integration_message_bridge` binary. It forwards ordinary actions
unchanged. For `record_append_stored`, it waits for the real child to succeed,
persists/fsyncs its returned checkpoint marker, and withholds stdout. Its hold is
not a new production transport. A 30-second fixture RPC limit gives the
controller time to kill; an RPC timeout is a different outcome, not this test.

An empty public-image container completed normal onboarding with only the public
dummy key, no mounts, owner profiles or real credentials. External networking
was disconnected before code copy/workspace trust. Claude started ordinarily in
the disposable project, discovering the installed owned module without a plugin
override or managed launcher. The endpoint was scripted and loopback-only.

After one actual initial Turn, one frozen Unicode/newline review was staged via
the SDK library. The actual module prepared and stored its native row checkpoint.
The controller observed the post-commit marker and killed the owned container
553 ms after its stored timestamp, inside the 30-second RPC window. Docker
reported exited/137, not OOM killed. The pending submit never produced a durable
raw handoff return. This is deployment-supervised container cleanup, not a claim
about standalone Windows children or power-loss durability.

## Independent evidence and result

Before restarting, a cold journal copy was inspected by Python `sqlite3` in
read-only mode, without SDK code. It contained one Dispatching operation, one
complete original-reservation append checkpoint, zero handoff records and zero
qualified delivery observations. The native user row with that UUID and exact
framed bytes was already present in the saved transcript.

The existing SDK recovery API then changed only Dispatching→Unknown. It retained
the original payload, reservation, session/invocation and checkpoint; it created
no acknowledgement, consumption, failure or new attempt. The original native
session resumed under a new invocation ID. The old binding became Stale; its old
operation remained readable under its original target. Repeating immutable
staging against that historical operation returned the same Unknown receipt.

The native transcript retained one identical user row and one review enqueue
before/after resume. Only the initial human input reached the scripted endpoint
before the kill; the review had no model request. The fresh resume endpoint
journal remained absent. A post-exit read-only SQLite copy confirmed the same
checkpoint, Unknown operation, zero handoffs and zero qualified observations.
No native consumption or acceptance was inferred from stored text.

`verify-sdk-append-crash.mjs` passed in Linux and on retained evidence. Six
verifier checks passed: original capture passes; missing cold checkpoint,
replacement invocation, invented acknowledgement, kill beyond the RPC deadline
and duplicated review enqueue fail. These five mutations are verifier sensitivity
checks, not five additional native crash runs.

## Scope and identities

Pinned image/executable are those in the baseline native report. Rust 1.88 Linux
built the thin recovery example; scoped example Clippy passed with the existing
`-D warnings -A clippy::uninlined_format_args` allowance. SDK/module source is
unchanged from the prior 135-test/22-test integration run; those suites were not
rerun or counted as new evidence here. No consumer dependency, readiness or
Windows policy was changed. Windows remains deferred.

| Executed component/evidence | SHA-256 |
| --- | --- |
| Hooks/recovery example binary | `49918f2e7d93a39f120443279730b5c61ae38ef1d8eead0904b324cca6e88c41` |
| Actual SDK receiver binary | `ccb77c46fed9b21b3b92c01d161ef1c3fa2038a5202db3c05e12710494cccd70` |
| Generated owned module | `5c7e12a9e6ee5f1f477c77939607c123544aae4571df4f620eb65486945a20f4` |
| Setup controller | `75f50d667f7648d479cae56c9ddf121fdc713c4a36d91956526960d04d15e138` |
| Fault-injection wrapper | `287d647e18f3c9f21320522fa8c109ad8a6c26002a0e237d72a7dfc2b0f43c38` |
| Cold SQLite inspection | `814ad44452dc8860969d9e2ba2ffb64bd0cdc2c9b334c30b66314d8a7050aca6` |
| Post-recovery SQLite inspection | `2f7182fe40a83d797a063642843694c9c53f3ca5c9dcb69591e5df58e868b371` |
| Post-resume native transcript | `396a2f40e5e6ce3f14e81c3ff06e28c9325217797c9f5e81ee0bdbdfec311322` |

Raw parent-chat evidence: `.local/sdk-append-crash-20261021`. Its verification
JSON includes all capture hashes. Cold SQLite/WAL files are retained separately;
no owner profile was copied. Build and Clippy logs are `sdk-append-crash-build.log`
and `sdk-append-crash-clippy.log`. Only owned test containers/processes are
removed after evidence retention.

## Reproduce

Follow the normal combined setup's empty onboarding/network-disconnection
sequence. Copy the three embedding examples and fixture into the disconnected
container. Run `setup-sdk-append.mjs setup crash`; start the loopback endpoint
in `/home/node/evidence-crash`, then ordinary Claude in
`/home/node/append-client-crash`. Submit the initial marker once and stage the
review externally with `setup-sdk-append.mjs stage crash`.

Observe `checkpoint-committed.json`, then SIGKILL only that owned container
before its 30-second RPC expires. Inspect Docker termination and retain named
files, the native transcript and a cold SQLite/WAL snapshot before restarting.
Inspect the cold copy read-only. Restart that same container, invoke
`setup-sdk-append.mjs recover crash`, and retain inspection before native resume.
Resume the recorded native session with a fresh loopback request directory.
Inspect the historical operation, repeat its exact immutable staging request
through the library, exit normally, and retain the final transcript/journal.
Run the independent crash verifier and its sensitivity tests on the capture.

This closes one real SDK post-storage/lost-reply kill window. Other failure
boundaries, protected native caller/generation/idle admission, authoritative
native reconciliation, protected actual consumption, full consumer integration,
other providers and Windows remain open. The unchanged correct production
projection is Unknown; this report does not activate delivery capabilities.
