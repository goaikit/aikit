# Recorded-context message preparation RPC

`IntegrationService::handle_recorded_message_request(installation_id, reader)`
provides bounded stdin/stdout preparation for an owned module's fixed command.
The `integration_message_bridge` embedding example delegates directly to it.
It calls no native API, sends no prompt, captures no native receipt and grants no
message-submission capability. The module's polling/send loop, actual native
admission, acknowledgement/proof capture and Codelaya orchestration remain open.

## Envelope and scope

The version-1 request contains application/installation/agent keys, a
`NativeToolContext`, and tagged action `poll` or
`recheck { operation_id, reservation_id }`. Unknown fields, foreign scope,
invalid versions/contexts and envelopes above 128 KiB are refused. There is no
database path, owner identity, arbitrary program or selected binding in the frame.

The caller supplies the trusted expected installation ID from fixed configuration.
The SDK reads only that installation receipt's inherited environment key. It
compares the envelope with inherited module claims, canonical workspace and
current recorded invocation. These are correlation checks, not authentication
of a physical native caller. Required native identity/admission remains unqualified.

The installation must have owned tool files, input observations and an existing
active application binding. Polling cannot attach or revive a detached binding.
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
`message_status` for inspection. Opening a store can upgrade it to schema 10;
do not run the example against an older live consumer's state before a coordinated
upgrade. Development tests touched only disposable fixture state.

Final validation: native Windows Rust 1.88 passed 100 integration tests; isolated
offline Linux passed 102 after a fresh source copy and SDK crate clean. The stdio
example compiled on both; Windows library/example Clippy passed with warnings
denied and the existing `uninlined_format_args` allowance. Provider profiles and
credentials were absent from the Linux container and external networking was
disabled. No native send/admission/receipt/consumption qualification is implied.
