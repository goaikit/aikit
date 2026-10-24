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

The existing combined-plan convenience method remains available. Schema **14**
fences the retained caller request identity: older schema-13 receipt writers can
ignore and drop that field when rewriting a completed plan, so they must refuse
opening the newer store. Explicit 9–13 upgrades require detached bindings and
settled installation plans and retain the original request digests. Schema 13's
mandatory detached guard still protects apply. Stop old consumers before an
upgrade; this is not proof that an already-running old process has terminated.
No provider transport is introduced.

## Linux evidence, 2026-10-22

The initial transaction API passed 143 integration tests on base `685b703`.
The final schema-14 fence on base `b359ddb` passed **144 integration tests, zero
failed**, using offline Rust 1.88 in the isolated Linux validation image. Scoped
library Clippy passed with warnings denied and the established format-lint
allowance. Counts overlap.

New tests exercise a lost plan reply, stable pre/post-apply retries, changed
request rejection, partial-write recovery, a held revision guard refusing a
writer, superseded-revision rejection, one retained plan identity, and cancellation
before apply while rejecting foreign/prepared/applied records. These are library
and filesystem contract tests; they do not qualify native sessions or Windows.
The additional schema-13 fixture refuses an ordinary runtime open and unsettled
upgrade, then preserves caller identity and its exact applied revision after an
explicit compatible upgrade.
Raw logs are retained in the parent workspace at `.local/update-journal-20261022`.
