# Inspecting interrupted installation work

`IntegrationService::pending_installation_plan(application_id, agent_key,
workspace)` returns the existing **prepared** plan for the canonical installation,
or `None`. It does not expose unrelated installations, create a plan, apply it,
or turn an unapplied preview into recovery work. The lookup checks persisted plan
identity, canonical workspace and removal-kind consistency. No schema migration
is required.

This lets a consumer distinguish interrupted removal from interrupted
installation/update without reading the private SDK database. For example, a
repeat remove command may resume only when `plan.removal` is true, using the
original `apply_install(&plan.id)`. Apply retains its existing fingerprint and
ownership checks; a conflicting external edit remains untouched and the plan
remains pending. Callers must keep their own owner authorization and lifecycle
locks. Lookup itself grants no permission to apply an arbitrary plan.

An interruption after the SDK receipt commit has no pending plan. Consumers must
also tolerate the installation already being absent while they finish their own
registration transaction. This API does not make the two databases atomic,
terminate a running external agent, or qualify native hooks and message delivery.

Tests inject the SDK's real post-file-write/pre-receipt error, reopen the service,
inspect the original plan and finish it. Other tests cover unrelated application,
provider and checkout scopes, non-removal inspection without application,
conflicting external edits, and corrupted persisted scope.

## Linux qualification, 2026-10-22

On base `0d4e0aa`, the integration suite passed 138 tests with zero failures in
the isolated Rust 1.88 Docker environment. After the final removal-kind
consistency guard, all three lookup tests passed again and scoped strict library
Clippy passed (`-- -D warnings -A clippy::uninlined_format_args`). Counts overlap.
No generated module, native provider API, SDK schema or Windows runtime changed.
Raw logs are retained in the parent workspace at
`.local/setup-recovery-20261022`. This is installation recovery evidence, not
native session/delivery qualification.
