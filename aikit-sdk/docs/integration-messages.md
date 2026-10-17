# External-session message journal

The [recorded-context RPC preparation](recorded-message-bridge.md) now connects
bounded fixed-command requests to this journal. Its opt-in Claude module now
contains a native handoff candidate with raw return storage. The candidate has
not qualified admission, acknowledgement or actual model-input consumption.

This is the durable local portion of existing-session messaging. It reuses
`IntegrationService`, `SessionBinding`, installation locks, the recorded session
journal and the same `integration.db`. It starts no process/server, installs no
provider hooks and provides no qualified native transport or admission lease.
MessageSubmission/MessageReconciliation assessments are unchanged.

## Reusable surface

| Method | Local behavior |
| --- | --- |
| `stage_message(MessageRequest)` | Persist exact UTF-8 content, SHA-256 and original binding/session/revision/start-cursor/invocation. Return Accepted, meaning local storage only. |
| `message_status(id)` | Read that original binding's receipt, including after detach/end/replacement. Never follow the new binding. |
| `reserve_message(id)` | Atomically mark the sole attempt Dispatching and return an immutable reservation. Requires current recorded scope and a recorded invocation. This is not native idle admission or caller authentication. |
| `recover_message_record(id)` | Conservatively change Dispatching to Unknown after possible lost handoff. Never resend/reset. Accepted remains unsent. Native reconciliation is not implemented by this method. |
| `record_message_observation(reservation, observation)` | Trusted adapter projection of acknowledgement, consumption or evidenced failure. Commit native-reference journal and receipt atomically. |
| `message_observations(id, after, limit)` | Replay immutable native-reference records/cursors for that original operation. |
| `message_handoff(id)` | Read a raw, unqualified return/uncertainty record for that original operation. Never treat it as acknowledgement or consumption. |

These are synchronous local storage operations, consistent with existing binding
lookups. The planned high-level native `submit_message`/`reconcile_message` still
need a qualified adapter; they are not faked by a local enqueue or recovery call.
Use the embedding example `integration_messages` with an existing binding ID and
the frozen payload on stdin. Never point it at a live older consumer's state
before coordinating the schema upgrade.

Operation IDs are unique within an installation, including across replacement
bindings. Exact retry recovers the existing receipt; changed bytes or target
conflict. Empty IDs/content and content above 4 MiB are refused without truncation.
Adapters must impose any smaller native limit before handoff. Accepted storage
does not establish native capability. No configurable scheduler or second
provider registry is added.

## Uncertainty and proof

Reservation commits before possible handoff and holds no SQLite transaction or
installation lock after return. A second reservation is refused for Dispatching,
Unknown, Failed and Acknowledged. A qualified adapter must still hold **native**
session/idle admission through the real handoff. `RecordedSessionGuard` must not
be held over I/O and does not provide that native guarantee.

Unknown accepts later matching evidence but cannot be reset to Accepted. Failed
requires a new owner operation; contradictory proof cannot resurrect it. A receipt
never invents consumption. Consumption may arrive first and establish the known
delivery outcome without fabricating an acknowledgement; a later receipt fills
that separate field. Native-reference event IDs/content are immutable and replay
does not retimestamp them. Cursors represent local append order, not native causal
order. Local recorded time is assigned once in Unix milliseconds.

This library validates correlation, bounds and storage transitions. The caller of
`record_message_observation` is trusted SDK adapter code, not an owner-facing
proof API. References must resolve to retained qualified native evidence; merely
storing a string does not authenticate a provider, prove input inclusion or
promote readiness. Original-scope late proof remains readable after replacement.

## Migration and remaining integration

Schema 10 adds outgoing-message and observation tables; schema 11 adds a separate
raw-handoff table, without replacing existing hook/install/binding/message rows.
Older schema-9/10 SDKs refuse the newer store when opened;
coordinate consumer upgrades and preserve backups. No owner's state was migrated
by development tests. The current Codelaya package still pins schema-9 code and
does not use this draft API yet.

Remaining: native qualification of the owned client; protected native
admission/caller and actual send/consumption provenance; high-level submission
and native reconciliation; Codelaya orchestration/projection integration; crash
and busy/competing-hook native tests; interactive Windows and provider parity.
Fixtures and the local journal do not close those gates.

## Validation, 2026-10-18

- Native Windows Rust 1.88: 97 integration unit tests passed, including five new
  journal cases. The embedding example compiled. Clippy for the library/example
  passed with warnings denied and the existing SDK `uninlined_format_args`
  allowance; this is not a strict whole-workspace lint result.
- Isolated Linux Rust 1.88: 99 integration tests passed after a fresh source copy
  and SDK crate clean. The example compiled offline. Read-only dependency caches
  were mounted without provider profiles/credentials, and networking was disabled.
- Cases cover exact Unicode/NUL payload/reopen, changed-content and replacement
  refusal, sole reservation, no SQLite lock after return, Unknown recovery without
  resend, independent consumption/acknowledgement order, historical late proof,
  changed replay/foreign scope/contradictory failure, atomic rollback on injected
  projection failure, corrupt payload refusal, real v9 table-addition migration,
  missing invocation refusal and input/cursor bounds.

The fixtures deliberately synthesize recorded generations and evidence references.
No native provider send, native admission, authenticated caller or actual model
consumption was tested or qualified by these unit cases. The versioned Codelaya
Windows package remains on its previously qualified schema-9 dependency.
