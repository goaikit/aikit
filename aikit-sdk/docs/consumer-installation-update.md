# Consumer installation update transactions

An embedding application can durably write its own update intent before SDK
planning, without losing or duplicating a plan across a lost reply:

- `plan_update_with_agent_tool_id(operation_id, spec, policy, tool, message_handler)`
  uses the caller's operation ID as the durable plan ID. The exact canonical
  request returns the same preview in planned, prepared and applied phases.
  Different content or scope under that ID conflicts. The normal combined-plan
  ownership, rendering and detached-binding guards remain unchanged.
- `applied_installation_guard(plan_id)` accepts only that exact current applied
  revision and a configured, non-drifted installation. Hold the returned guard
  across a short application metadata commit. It prevents cooperating SDK
  installation writers from superseding the revision during the commit. Never
  await, call native transport, or recursively invoke SDK status/writers under it.
- `discard_planned_update(operation_id, application_id, agent_key, workspace)`
  cancels only an unstarted caller-ID update in the specified installation. It
  removes a planned preview without changing any provider file. Prepared,
  applied, legacy non-idempotent or foreign plans refuse cancellation. An absent
  intent is harmless, provided it is not the current applied revision.

The existing combined-plan convenience method remains available. Schema 13 is
unchanged: its mandatory detached guard still protects apply. The new request
digest is additive metadata retained with the completed preview. No new runtime
or provider transport is introduced.

## Linux evidence, 2026-10-22

On base `685b703`, **143 integration tests passed, zero failed**, using offline
Rust 1.88 in the isolated Linux validation image. Scoped library Clippy passed
with warnings denied and the established format-lint allowance.

New tests exercise a lost plan reply, stable pre/post-apply retries, changed
request rejection, partial-write recovery, a held revision guard refusing a
writer, superseded-revision rejection, one retained plan identity, and cancellation
before apply while rejecting foreign/prepared/applied records. These are library
and filesystem contract tests; they do not qualify native sessions or Windows.
Raw logs are retained in the parent workspace at `.local/update-journal-20261022`.
