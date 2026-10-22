# Coordinated hook and callable replacement

`IntegrationService::plan_update_with_agent_tool(spec, completion_policy, tool,
message_handler)` plans replacement of an existing hook installation together
with its generated callable/instructions. The optional fixed message handler is
an explicit full-replacement input; it does not grant native message readiness.
The tool identity must match the application and canonical installation.

The method reuses the existing provider renderer, configuration ownership,
completion-setting merge, tool-file fingerprints and prepare/apply journal.
There is one plan ID and one final receipt. It does not remove/reinstall in
separate plans or publish a hook command that still points at the old generated
startup handler. Existing legacy hook-only planning retains its refusal when
changing a startup handler would leave an owned module inconsistent.

## Guards and recovery

- Recorded bindings for the installation must be explicitly detached at plan,
  apply and recovery. Bindings belonging to another installation are unaffected.
- `bind_existing` holds the installation lock through binding admission and
  insertion, closing the race with the replacement's detachment check.
- The pending plan retains `require_detached`; it is not merely a planning check.
- Apply validates all configuration/tool fingerprints before its first write.
  Existing prepared-plan recovery accepts only exact before/after bytes. External
  edits are preserved and refuse recovery.
- Module files are replaced through the existing ordered file changes, followed
  by the configuration and final receipt commit. A partial replacement remains
  RecoveryRequired; native hooks refuse that prepared generation.
- Exact applied-plan retry remains idempotent. Receipt revision changes still
  invalidate references from the previous installation revision.

Schema **13** makes this persisted guard explicit: a schema-12 runtime refuses
the newer store instead of ignoring the guard. Explicit upgrade supports 9–12
to 13, requires detached bindings and no unfinished installation plans, and
preserves existing message/append records. Existing runtime opens require the
explicit upgrade; no owner state was upgraded in qualification.

These are local recorded-state guarantees. Detaching is not stopping or proving
the death of an owner-started agent. The embedding application still owns
registration authorization, active-Turn preflight, durable cross-store update
identity and owner guidance to close the old agent. Native caller authentication,
protected admission and authoritative feedback consumption remain unqualified.

## Linux qualification, 2026-10-22

On base `abb3f3a`, the final Rust 1.88 offline integration suite passed **141
tests, zero failures**. New tests cover partial module replacement followed by
service reopen/recovery, a consistent new startup/callable receipt, preserved
unrelated settings, detachment checked at plan/apply/recovery, and explicit
schema-12 migration with retained receipt and refusal of unsettled state.
Existing conflict/recovery/message regressions use the same apply machinery.

The initial run had three stale version-boundary fixture failures (expecting 12,
or treating 13 as a future schema). Those fixtures were corrected and the full
suite passed again. Both logs are retained at
`.local/bundle-update-20261022` in the parent workspace. No generated native
module logic or Windows runtime was changed or newly qualified by these tests.
Scoped library Clippy also passed with warnings denied and the established
`clippy::uninlined_format_args` allowance.
