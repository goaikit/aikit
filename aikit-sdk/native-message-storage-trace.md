# Claude feedback storage trace: unqualified implementation

Status, 2026-10-24: **WIP; Linux execution and native qualification pending.**
Do not consume this change as a qualified consumer pin or promote readiness.

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

## Validation to run

Run the three generated module test files with Node 22 in Linux Docker, then
the SDK integration suite on Rust 1.88. New engine-stub cases cover 14 malformed
core observations, matching `passed`, and existing rewrite/lost-commit behavior.
These are fault cases in tests, not executed native sessions.

Next run a fresh ordinary interactive Claude 2.1.308 consumer session using the
actual rendered module: native file edit, enforced name/note repair, completed
first Turn, owner-submitted immutable review, same-session follow-up and normal
exit. Retain raw core-trace shape as well as independent transcript, actual
outgoing input, app/SDK records and package/source identities. Require candidate
row and bytes to correlate; acknowledgement/consumption must remain absent.
If the native trace does not supply the declared shape, retain the failure and
adapt the implementation before claiming compatibility. Qualify a competing
append handler separately; do not infer its effective native behavior from an
engine stub or the earlier completion-chain experiment.

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

## Environment interruption

Docker Desktop was stopped when this work resumed. Authorized startup failed:
`initializing Inference manager` could not remove/access the `dockerInference`
socket in the owner's Docker run directory. The engine pipe never became
available. No Docker data, socket, permissions, configuration or provider policy
was changed to repair it. The owner has been asked to start Docker normally;
Windows provider checks remain deferred. No execution result is claimed here.
