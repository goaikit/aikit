# Claude feedback storage trace

Status, 2026-10-24: Linux module/SDK checks and one actual native consumer
feedback session pass on runtime `1efbcd2`. This qualifies the healthy candidate
capture path only. Production acknowledgement, consumption and readiness remain
unqualified; Windows checks remain deferred.

## Concrete implementation gap

The owned message module previously recorded `record_append_stored` after
checking the delegated row ID and text, without examining whether the actual
native core ran. A matching downstream return alone is insufficient provenance.
The retained append checkpoint is still only candidate evidence, but it should
not be populated from an unverified downstream result.

The change reuses Claude's existing `next.trace`. Before recording storage it
requires exactly one engine/core `session.append` entry with `returned` or
`passed` outcome. The received row must name the reserved UUID, prompt door,
main conversation and full owned-plugin origin. Both the core input and output
must match the delegated result's exact text, and the core output must retain
the reserved UUID without denial. A genuine stored rewrite can still be
recorded against the separate original intent digest, then stops the pump.

Missing, duplicate, foreign, skipped, rejected, expired, caught or contradictory
core evidence stops capture. The module delegates exactly once, preserves its
return, retains handoff uncertainty, and never resubmits the reserved review.
No Rust API, schema, new transport or acknowledgement/consumption promotion is
introduced. Existing historical checkpoints remain candidates.

## Validation

Linux Docker Node 22 passes all 32 generated module tests. Against the previous
helper, the new storage fault test fails because an unverified return creates a
stored checkpoint. Rust 1.88 passes all 174 SDK integration tests. New engine-stub
cases cover 14 malformed core observations, matching `passed`, and existing
rewrite/lost-commit behavior. These are test cases, not native fault sessions.

A fresh ordinary interactive Claude 2.1.308 session used the real Codelaya
CLI/library and exact generated module: native UTF-8 Write, two name/note Blocks
and repair, completed initial Turn, one actual owner CLI review, same-session
follow-up and normal exit. Twelve main requests (13 total) passed independent
input/transcript/app/SDK verification. One matching candidate append checkpoint
survived exit, both Turns remained Completed and the original Turn/Git/file
bytes were preserved. App/SDK stayed Unknown with no acknowledgement or
consumption. Raw trace entries were not separately archived; the unchanged
generated guard and its successfully stored checkpoint establish only that its
checks passed. Do not infer a broader native trace contract from this run.

Session: `2ddddeb1-5920-44c9-bd01-fb39b3e67e77`. Same native image/executable as
the previous 2.1.308 reports. The consumer report retains exact source/binary
identities, preliminary setup/startup and verifier failures, corrected automatic
user-tier loader checks, offline onboarding fixture, and 21 altered-evidence
checks. Qualify competing append handlers and restart/consumption separately;
do not infer their behavior from an engine stub or completion-chain experiment.

## Concrete remaining Claude contracts

The inspected engine-generated 2.1.308 declarations provide:

- `prompt.submit`: entered or queued text/origin, or drop. No immutable operation
  receipt or authoritative status lookup keyed by AIKit's operation ID.
- `session.append`: row UUID, origin, input and stored-row result. The trace can
  check actual core execution; it does not by itself prove crash durability,
  original-session admission through all handlers or later model consumption.
- `turn.step`: Turn/index/model/message count and a streaming response. The API
  view from `session.messages({as: 'api'})` is not the exact complete wire input.
  A downstream handler may answer without sending a request. A returned usage
  or an echoed review is insufficient consumption evidence.

The remaining provider request is stable, generation-scoped native admission
and reconciliation plus a correlated engine observation after actual model
dispatch identifying inclusion of the exact review bytes/digest. First qualify
existing native trace behavior before concluding a provider API change is
required. See [N1–N3](native-integration-required-contracts.md).

## Earlier environment interruption

Docker Desktop was stopped when this work resumed. Authorized startup failed:
`initializing Inference manager` could not remove/access the `dockerInference`
socket in the owner's Docker run directory. The engine pipe never became
available. No Docker data, socket, permissions, configuration or provider policy
was changed to repair it. A subsequent live check found Docker running and
accessible through the approved pipe access. All results above then ran inside
Linux containers. No Windows provider qualification or Docker repair is claimed.
