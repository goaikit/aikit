# Eval artifacts are an additive-only contract

## Status

accepted

## Context

`aikit-evals` writes three artifacts per run: `result.json` per trial, `trace.jsonl` per trial, and `summary.json` per case set. They were introduced as an internal record of what happened. They are no longer that.

`fastskill` reads all three. Its `eval score` subcommand re-derives verdicts from a completed run directory, and its CI keeps two recorded runs committed in the repository — one that must score pass and one that must score fail — as the deterministic oracle for the scoring path. Those fixtures are checked in. They are not regenerated on each run, and they cannot be: regenerating them needs a live agent and a paid provider call, which is the thing the fixtures exist to avoid.

That makes the artifact shape a published interface with at least one consumer whose test suite is pinned to bytes on disk. [ADR 0019](0019-codex-decode-emits-typed-tool-frames.md) already ran into this from the other side and decided the `command_count` field keeps its name even though the check it reports on was renamed to `max_tool_calls`, reasoning that "artifacts are a consumed contract; renaming a configuration knob does not justify breaking readers." That was a single-field decision made in passing. This ADR generalises it, because the measurement-integrity work adds six fields at once and the question stops being incidental.

The failure mode is specific and quiet. A renamed field does not fail to compile downstream. `serde` deserializes the record with the old name absent, the `Option` lands as `None`, and a scorer reports "unknown" or skips a case. A removed field behaves identically. The consumer's tests keep passing against fixtures written before the rename, and the divergence surfaces only when someone regenerates a fixture months later and the numbers move. Every property that makes these artifacts useful as a regression oracle also makes a schema break invisible.

## Decision

**The eval artifact schema is additive-only.** New fields may be added at any time. Existing fields are never renamed, never removed, and never change meaning or type.

Concretely, for `TrialResult`, `CaseResult`, `CaseSummary`, `CaseTrialsResult`, `SummaryResult`, `IsolationReport` and every `TracePayload` variant:

- Every field added from this point carries `#[serde(default)]`, so an artifact written by an older version still deserializes.
- Every field added is optional in meaning as well as in type. Absent means "this version did not record it", never "zero" and never "false". A reader that cannot distinguish those two is reading it wrong.
- A field whose name becomes wrong keeps its name and gains a doc comment explaining the divergence, following `command_count`.
- A concept that genuinely changes shape gets a new field beside the old one. The old field keeps being written for as long as any consumer reads it.

**Enum variants may be added; existing variants may not be renamed or removed.** `CaseStatus` in particular is serialized lowercase into every artifact. `skipped` is currently never constructed and is nonetheless reserved rather than deleted, because deleting it would make any future artifact carrying it unreadable by the version that deleted it.

**Removing anything is a coordinated, deliberate act**, not a refactor. It requires: a released version of every known consumer that no longer reads the field, regenerated fixtures in those consumers, and a superseding ADR. There is no deprecation timer that makes it automatic.

## Consequences

- The artifact structs accrete. `TrialResult` gains `exit_code`, `terminal`, `cost_usd` and a full token breakdown in this change, and it will keep growing. That is the intended cost: a struct that is slightly untidy is cheaper than a consumer that silently reports the wrong number.
- `fastskill`'s committed pass and fail fixtures keep scoring identically across an `aikit-evals` bump. This is the property the rule exists to protect, and it is testable: score the committed fixtures after the bump and the verdicts must not move.
- Cleanup PRs that rename an artifact field are rejected on sight, regardless of how much better the new name is. The naming debt is recorded in doc comments and in the glossary instead. `CONTEXT.md` names Trial outcome and Case verdict as separate concepts while `CaseStatus` serves both, and that entry says explicitly that the vocabulary leads the refactor rather than ratifying the type.
- Writers must never emit a field they cannot populate honestly. `cost_usd` is `None` when the backend reports no cost; it is never an estimate derived from a local price table, because a stale estimate is indistinguishable from a real number once written to an artifact.
- The rule binds the schema, not the values. Computing an existing field differently — for example excluding errored trials from `pass_rate` — is a behaviour change governed by its own reasoning, and it does move fixture verdicts. Fixtures regenerate for that; they must not have to regenerate for a rename.
