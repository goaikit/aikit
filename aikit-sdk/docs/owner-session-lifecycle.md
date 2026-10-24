# Owner recorded-session lifecycle

`attached_bindings(application_id, agent_key, workspace, after_id, limit)` returns
a scoped, bounded SQLite snapshot of non-detached handles. Ended and stale
handles remain listed because they can still block metadata maintenance. Status
is recorded state, never a native-process inventory. `next_id` is the last
returned opaque ID, not proof of another page. Limits are 1–500; installation
files are checked once per page.

`detach_recorded_binding(application_id, agent_key, workspace, binding_id)`
validates the exact scope and atomically:

1. Marks only that handle detached.
2. Records the current journal admission floor for its installation.

New handles cannot bind a SessionStart observation at or below that floor.
An already recorded but unadmitted start therefore cannot silently revive work
after owner detachment. A later recorded SessionStart can be admitted normally.
Other existing non-detached handles are not revoked; repeating a completed
detach does not advance the floor. Legacy `SessionBinding::detach()` delegates
to the same behavior. Installed hooks, settings and historical records remain.
There is no native command, signal, termination or managed replacement session.

For coordinated installation updates, `applied_installation_guard` rechecks the
persisted detached-binding requirement at the consumer commit boundary too. A
handle created after apply cannot authorize a registration commit. The owner can
release that handle without modifying the pending installation plan, then resume
the original update. The regression covers this extra boundary; the 147-test
suite and scoped Clippy passed again after the change.

Schema **15** adds `binding_admission_floors`. Explicit 9–14 upgrades require
settled plans and detached handles, preserve existing data, and seed floors for
legacy detached records from the journal maximum. A malformed legacy detachment
scope refuses migration without replacing it. Older SDK opens refuse the newer
store instead of ignoring the admission policy. Stop older consumers before
upgrading; this does not prove an already-running native process has stopped.

## Explicit legacy owner recovery, 2026-10-23

Older metadata no longer requires an old executable merely to release known
handles before upgrade. Two additive public helpers reuse existing scoped
references, inspection pages and the canonical migration transaction:

- `IntegrationService::attached_bindings_for_upgrade(state, application_id,
  agent_key, workspace, after_id, limit)` reads a bounded schema-9-through-15
  snapshot without initialization/migration. Its statuses reflect journal
  correlation only; it does not inspect active native configuration or liveness.
  Normal `attached_bindings` retains its current-schema/configuration checks.
- `IntegrationService::upgrade_existing_detaching(state, application_id,
  agent_key, workspace, binding_ids)` explicitly detaches 1–500 distinct selected
  scoped handles and upgrades in the same IMMEDIATE transaction. Missing or
  foreign IDs, unselected attached handles, unsettled plans and migration errors
  roll back the whole operation. The normal upgrade still refuses attached
  handles. No configuration, messages or provider processes change.

Close old runtimes first. The migration seeds existing schema-15 admission floors
at the same journal boundary. Repeating the request after successful migration
is read-only when all selected handles remain detached, and never advances those
floors. It refuses new attached handles on current schema; use ordinary explicit
owner detachment instead. Selected IDs are consent, not a native termination or
message-consumption attestation. No new stored schema or parallel lifecycle
registry is introduced.

The Linux integration suite passes 159 tests, including the original refusal
reproduced before implementation, schema-9-through-14 recovery, atomic rollback,
bounded/scoped inspection and old-unbound/fresh-start admission checks. Scoped
SDK lint retains existing warnings; native readiness and Windows acceptance do
not change. Raw logs are retained in the parent workspace at
`.local/legacy-owner-upgrade-20261023/`.

### Earlier Linux qualification, 2026-10-22

On base `daf18cf`, **147 integration tests passed, zero failed** in the isolated
Rust 1.88 Linux environment. Scoped library Clippy passed with warnings denied
and the established format-lint allowance. New cases cover scoped pagination,
foreign-application rejection, unchanged other handles/settings, an old unbound
start rejected after detach, a fresh start admitted, repeat stability, and
legacy migration/malformed-data preservation.

The first build exposed signed SQLite cursor typing errors; the following suite
exposed one old fixture using `{}` as a session reference. It was replaced with a
valid typed legacy reference, while a separate malformed-data case verifies
refusal. Clippy's redundant unwrap was removed; final tests/Clippy passed again.
Raw logs are retained in the parent workspace at `.local/owner-lifecycle-20261022`.

This is recorded lifecycle and replay evidence. Native identity authentication,
protected message admission, acknowledged/consumed delivery and Windows native
acceptance are not qualified by these tests.
