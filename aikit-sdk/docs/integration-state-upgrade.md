# Explicit existing integration-state upgrade

`IntegrationService::upgrade_existing(state)` is an owner maintenance operation
for existing schema 9/10/11 journals. It reuses the initializer's additive message
table migration and returns `StateUpgradeReport { previous_schema,
current_schema, changed }`. The current schema is 12. It opens no native agent,
transport, model or provider configuration.

```rust,no_run
use aikit_sdk::integration::IntegrationService;

# fn upgrade() -> Result<(), Box<dyn std::error::Error>> {
let report = IntegrationService::upgrade_existing("/private/application-state")?;
let service = IntegrationService::open_existing("/private/application-state")?;
assert_eq!(report.current_schema, 12);
# Ok(()) }
```

The state directory and nonempty database must already exist. Unknown/newer,
unversioned and pre-9 journals are refused. The expected core tables/columns are
checked; missing history is not recreated as a schema upgrade. Schema 10 must
retain its outgoing-message and observation tables. An already-current journal
returns `changed: false`, without schema mutation.

For a real migration, every recorded binding must be detached and every
installation plan applied. Otherwise it returns Conflict or RecoveryRequired
without migrating, detaching, finishing a plan or terminating anything. Resolve
these conditions using the compatible owner runtime first. The SDK installation
lock and one immediate SQLite transaction cover guard checks and schema changes;
a late migration failure rolls back the added tables and version together.

Existing owned installation receipts, generations, hook observations, immutable
message payloads, reservations and delivery evidence are retained. Unknown stays
Unknown; acknowledgement and consumption are not fabricated. Exact historical
message lookup/retry semantics remain unchanged. The operation neither reserves
nor sends/replays a message, and never adopts another session.

This is not native process supervision or a quiescence lease: detached bindings
do not mean the native process stopped. The embedding must stop/drain its own
sidecar writers and revoke application admission before maintenance. It must
separately coordinate review schema/executable changes, durable recovery/backups,
owned hook updates, provider requalification and subsequent activation. The SDK
report does not establish readiness, permit downgrade or make old binaries
compatible with the new schema.

`open` retains its existing create/legacy-migrate initializer semantics for
compatibility. Runtime hook/replay/status paths should use `open_existing`, which
never creates or implicitly upgrades a missing/older journal. Use this explicit
operation for supported owner maintenance instead of treating a runtime reopen
as permission to migrate.

## Validation scope

Native Windows Rust 1.88 fixture tests cover missing/empty/unsupported state,
owned configuration/receipt preservation, idempotence, attached bindings,
planned/prepared operations, another writer, missing core history, rollback on a
late SQL error, and exact retention of an Unknown message after detach and schema
10 upgrade. These storage tests exercise no native provider and grant no new
capability support.
