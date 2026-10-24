# Message evidence row consistency

## Defect and correction, 2026-10-23

The message page API previously selected only JSON bodies and used their embedded
cursor as the next replay cursor. A corrupt body could name a cursor beyond
later committed consumption evidence. JSON identity was not checked against its
own SQLite cursor/event columns, and receipt evidence fields were not resolved
back to their scoped journal rows.

The existing APIs now refuse inconsistent data without rewriting or repairing it:

- `message_status` validates acknowledgement/consumption/failure field kinds and
  resolves each field to the same installation/operation/cursor in the journal.
- `message_observations` decodes the actual SQL cursor and event ID alongside
  JSON, validates bounds/reference shape, and requires each returned row to equal
  its corresponding receipt projection. `next_cursor` therefore uses validated
  row identity. Receipt and page reads share one deferred SQLite snapshot.
- Observation replay uses the same validation. Trusted input and stored input
  reuse the existing identity/source/native-reference bounds. Missing rows,
  foreign events, wrong field kinds, invalid references and mismatched receipts
  fail explicitly. There is no resend, cursor reset or automatic corruption fix.

No public type, serialized format or database schema changes. Valid historical
records retain their IDs/cursors/times and remain readable after detach. Matching
consumption-before-acknowledgement remains valid and does not fabricate an ack.
An intact archived journal may be restored through an explicit owner recovery
procedure; preserving a corrupt row does not establish that it is usable.

This validates local row consistency, not the truth of native evidence references.
Native authentication, admission and authoritative acknowledgement/consumption
still require a qualified adapter and retained native proof. Capabilities remain
unchanged.

## Qualification

The new cursor regression fails against the previous library because it returns
the forged JSON cursor. The corrected full Linux Rust 1.88 integration suite
passes **161 tests**. Additional negative cases cover foreign event columns,
empty source/reference, wrong proof kind, missing row and forged receipt cursor;
corrupt bytes are preserved. Existing unknown-hand-off, consumption-before-ack,
late proof, historical binding and migration cases remain passing.

Scoped library Clippy completes with the same 101 existing warnings; this is not
a strict SDK lint pass. The tests use synthetic trusted observations and private
SQL corruption fixtures, not a native provider session or Windows qualification.
Raw red/green/format/lint logs are retained in the parent implementation workspace
at `.local/message-evidence-integrity-20261023/`.

```sh
cargo +1.88.0 test -p aikit-sdk --features integration --locked --offline --lib integration::
cargo +1.88.0 clippy -p aikit-sdk --features integration --locked --offline --lib --no-deps
```
