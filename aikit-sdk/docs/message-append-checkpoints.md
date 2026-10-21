# Native append checkpoints

The integration feature exposes candidate row correlation through the existing
`SessionBinding` and immutable `MessageReservation`:

- `prepare_message_append(reservation, MessageAppendIntent)` records the stable
  native row identity **before** its storage handler runs.
- `record_message_append_stored(reservation, native_row_id, MessageAppendStored)`
  retains a candidate storage observation for that same row.
- `message_append_checkpoint(operation_id)` reads the original operation's
  checkpoint, including historically after detach, restart or replacement.

These methods call no provider, send nothing and grant no admission lease.
Fields are adapter claims, not authenticated native events. They create no
`MessageProof`, acknowledgement or consumption and never change delivery state.
Missing data is unknown; it never permits resubmitting an uncertain operation.

## Why both hashes and phases exist

The [real Linux interruption fixture](../../examples/claude-mod-qualification/native-append-interruption.md)
stored a message, then lost its pending submit return to SIGKILL. A pre-storage
checkpoint can retain the row UUID needed for later lookup. Claude's native
message-list API returned text without UUIDs; it cannot independently resolve
this reference. Reuse existing history readers where sufficient; retain origin,
version and invocation proof separately. Do not add a second managed session or
search for matching text and declare delivery.

The [native competing-handler fixture](../../examples/claude-mod-qualification/native-append-adversarial.md)
showed that stored content may change while submit return and Turn-start text
remain original. `MessageReceipt.payload_sha256` remains the frozen logical
payload hash. `intent.framed_payload_sha256` is the expected native envelope;
`stored.stored_payload_sha256` records what the adapter actually observed.
A differing hash is retained as a mismatch, never overwritten with the expected
value. Neither an exact hash nor successful checkpoint storage qualifies a
production native receipt on its own.

## Embedding sequence

In an adapter with an already reserved original operation:

```rust
use aikit_sdk::integration::{MessageAppendIntent, MessageAppendStored};
use sha2::{Digest, Sha256};

// At the native append boundary, before delegating native storage:
let prepared = binding.prepare_message_append(&reservation, MessageAppendIntent {
    native_row_id: native_row_id.clone(),
    native_source: "claude-code/2.1.296/session.append".into(),
    framed_payload_sha256: format!("{:x}", Sha256::digest(&expected_native_bytes)),
})?;
// Release the local transaction before native I/O. The adapter must separately
// protect native identity/admission. No such protection is supplied here.
let actual = native_storage_handler()?;
let stored = binding.record_message_append_stored(&reservation, &native_row_id,
    MessageAppendStored {
        stored_payload_sha256: format!("{:x}", Sha256::digest(&actual.bytes)),
        evidence_reference: actual.retained_reference,
    })?;
```

This illustrative embedding sequence takes native values from the adapter; it is
not a standalone runnable example. The library regression tests are non-CLI
consumers exercising the actual methods. The opt-in owned Claude module now
captures these phases through the existing fixed-command bridge; see the
[combined native SDK run](../../examples/claude-mod-qualification/native-sdk-append-client.md).
Raw submit returns remain separate. Normal-exit persistence is qualified only
within that bounded Linux fixture; combined SIGKILL recovery and production
native proof remain open.

On an interrupted handoff, existing `recover_message_record` conservatively
retains Unknown. Read the old checkpoint and independently reconcile its stable
row/source against the original target. Only qualified native proof may later
use the separate `record_message_observation` projection. Never prepare a new
attempt or follow a replacement target to resolve an old operation.

## Durability and compatibility

Each write uses the existing installation lock and one immediate SQLite
transaction. No transaction survives return. One operation has one intent;
one native row identity cannot be claimed by another operation in the same
installation. Identical replay returns the same original timestamps. Changed
scope, row, digest or evidence conflicts. A new intent requires the original
active Dispatching target. Existing exact replay and late stored observations
can inspect/extend historical candidate data without reviving its target.

Schema 12 adds `message_append_checkpoints`; no existing message/handoff/evidence
record is reinterpreted. Explicit existing-state upgrade supports 9/10/11→12,
requires detached bindings and no pending install plan, and is transactional.
Runtime `open_existing` refuses implicit upgrade. Current-schema maintenance is
idempotent. Older SDK readers reject the new schema instead of losing candidate
correlation semantics. This is metadata maintenance, not an executable/hook
update, native requalification or consumer dependency promotion.

## Executed Linux checks, 2026-10-21

Follow-up wiring passed 135 integration tests, 22 generated-module tests and
scoped Clippy; the linked combined-native report retains the later source hashes.
The initial checkpoint-only source/results below retain their original scope.

Rust 1.88.0, offline Docker with read-only dependency caches and the dedicated
validation target volume:

- 131 integration-module regression tests passed before the final migration-table
  preflight tightening. This includes the six initial checkpoint tests.
- Final source: all seven `integration::messages::tests::append_` tests passed,
  including malformed schema-11→12 migration rollback.
- Final source: all seven `explicit_upgrade` regression tests passed.
- Final source: scoped integration-feature library Clippy passed with
  `-D warnings -A clippy::uninlined_format_args`, the existing format allowance.

Commands use `--lib --no-default-features --features integration --locked --offline`;
test runs use `--test-threads=1`. The checkpoint tests are direct non-CLI consumers.
They do not invoke a native provider. These counts overlap and must not be summed
into independent scenario coverage. No Windows check was run for this change.

Final tested source SHA-256:

| Source | SHA-256 |
| --- | --- |
| `message_append.rs` | `d2e480c0c2f5edda8b037d65e8e3f79c2088a2ae3518e0679c047880fc90193b` |
| `install.rs` | `6102d7d9db7142dca7c08d5f4cd4a9653de523b1a6de8da37722a8107815f075` |
| `messages_tests.rs` | `8a7ec0f8e721625d7131bd55739112a0ea3dfb3e8312f3b8aa0a12b6449fff9b` |
| `install_tests.rs` | `7e87a5e5e7696ba6ea15335978ea7a8ec0957eb3a4b3d59969955320b5c988a5` |
| `mod.rs` | `cd70927f4071c522986cca01a822de6c05c77305623e8e03e16f44e925363237` |

Development lock SHA-256:
`ef111e91aec4cc31cc1810e077dfc357d1d5010539b7d239434744792c9fbf06`.
Raw parent-chat `.local` logs are `docker-sdk-append-regression-20261021.log`,
`docker-sdk-append-final-20261021.log`, `docker-sdk-append-upgrade-20261021.log`
and `docker-sdk-append-clippy-20261021.log`. The earlier unpinned-toolchain attempt
failed before build; explicit `+1.88.0` then used the existing offline toolchain.
