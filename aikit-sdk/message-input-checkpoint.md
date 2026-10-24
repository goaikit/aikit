# Operation-correlated native input candidates

2026-10-24, parent SDK `669e893`. This extends the existing append checkpoint;
it does not add a daemon, delivery state, native transport or capability grant.

## Public surface

- `SessionBinding::prepare_message_input(reservation, MessageInputIntent)` retains
  the first exact pre-dispatch step against an already stored, unchanged native
  append row. Fields: native Turn, safe-integer step index, complete API snapshot
  SHA-256, exact native frame SHA-256 and canonical step-event SHA-256.
- `record_message_input_dispatched(reservation, intent, MessageInputDispatched)`
  retains a separate candidate matching the same step-event digest, with core
  trace digest and a reference to separately retained native evidence.
- Existing `message_append_checkpoint(operation_id)` retrieves both records,
  including after reopen/detach. Exact repeats are idempotent; changed reservation,
  Turn, step, snapshot, frame or dispatch evidence conflicts. A new preparation
  requires the original observed binding. A late exact dispatch can be retained
  after detach through the binding API; it never revives a target or sends.
- Existing recorded-message bridge adds `prepare_input` and
  `record_input_dispatched` actions; replies reuse `append_checkpoint`. The live
  bridge continues to require the current observed inherited context.

The stored append body becomes **record version 2** only when input is prepared.
Version 1 records remain readable and serialize without a new empty field.
Version 2 requires its input and unchanged stored frame; old readers explicitly
require version 1 and therefore refuse it instead of silently rewriting away
input evidence. Update the SDK handler and generated module together. The SQLite
schema remains 15 and uses the same table/transaction/lock. No migration or new
dependency is introduced.

## Generated Claude observer

For pinned native Claude 2.1.308 only, after the generated sender observes exact
core storage it retains one pending candidate. Its first main `turn.step`:

1. Requires engine/core origin, the current main Turn and a valid step index.
2. Reads `$.session.messages({as: 'api'})`; requires one exact complete frame and
   a serialized snapshot of at most 4 MiB. Stores hashes before delegation.
3. Streams `yield* next(event)` exactly once and preserves its result.
4. Requires one matching settled engine/core `turn.step` trace, with returned or
   passed outcome and unchanged canonical received event. Retains its digest
   (trace bound 256 KiB) separately after delegation.

No post-dispatch API snapshot is used to infer the original input. A capture or
commit-response failure stops future message pumping; it never redelegates,
resends, or substitutes a later step. Subagent steps do not consume the pending
main candidate. Unsupported older clients retain their existing append behavior.

These are **adapter claims/candidates**, not authenticated acknowledgement or
consumption. The API view can differ from later model input; other handlers may
mutate context after this observer. A core trace alone is not an exact input
witness. Hashes require separately retained source evidence; the evidence URI
does not create a native durable lookup API. Production readiness stays refused.
The same operation's delivery/observation records remain unchanged.

## Scoped qualification

Linux Docker Rust 1.88 locked/offline: 180 integration tests pass. The four new
tests cover reopen, Unknown preservation, detach/late dispatch, unchanged ticket,
stored rewrite, input/dispatch conflicts, corrupt record refusal and actual
recorded bridge decoding/scope checks. Existing two unused-guard warnings remain.
Thirty-six generated module tests pass, including seven new input fault tests
for missing/duplicate/oversized frames, foreign scope, synthetic/changed/duplicate
core traces, lost commit replies and changed RPC responses. These use synthetic
native APIs and do not qualify live dispatch or native recovery.

The earlier [actual native pre-dispatch comparison](recorded-admission-wait.md#fresh-consumer-comparison)
remains at SDK `de7e171`; its acceptance does not transfer to this new generated
observer. A fresh matching consumer/native run is recorded below; late mutation,
clipping/size, competing handler and interruption qualification remain required.
Windows checks remain deferred. Local logs: consumer workspace
`.local/input-checkpoint-20261024/sdk-integration.log` and `generated-module.log`.

Native API references: [hook chaining and streaming](https://code.claude.com/docs/en/plugins/mods/events),
[mods API](https://code.claude.com/docs/en/plugins/mods/api). Their availability is
not a guarantee of durable, operation-correlated native consumption.

## Fresh matching native consumer comparison

Codelaya [source `39e706a`](https://github.com/aroff/codelaya/commit/39e706a)
pins runtime `8ff3d89`. Nineteen scoped callback/delivery/completion tests and
matching binaries pass. A fresh ordinary interactive native Claude 2.1.308
session completes authoring, idle compaction, one frozen owner review and
same-session feedback. Version-two intent/dispatch records match the independent
pre-dispatch API snapshot and actual settled core event/trace, and remain exact
after ordinary native/sidecar shutdown. Fourteen altered-evidence checks pass.

[Exact consumer protocol, hashes, retained failures and limits](https://github.com/aroff/codelaya/blob/39e706a/docs/native-input-checkpoint.md).
An empty hello handshake invalidated the old fixed request-count assumption;
the final verifier explicitly retains it separately from nine main, one title
and one compact request. The corresponding stale-index mutation-helper failure
is also retained and corrected. No native submitted state was repaired.
These are still candidates: app/SDK remain Unknown with no acknowledgement,
consumption, native receipt qualification, readiness or Windows promotion.

## Competing-handler matrix on the same runtime

Codelaya [qualification source `5ab9259`](https://github.com/aroff/codelaya/commit/5ab9259)
uses unchanged runtime `8ff3d89` / application binaries `39e706a`. Three fresh
ordinary native 2.1.308 sessions keep the generated SDK at user tier, independent
observer at prepend tier and a controlled competitor at append tier. Actual
native logs and traces verify this placement.

- An unchanged delegated step retains preparation and matching dispatch.
- A first feedback step rewritten from medium to low effort reaches real core
  dispatch and HTTP input, but cannot attest the original prepared event. The
  SDK retains preparation without a matching dispatch observation.
- A synthetic first step supplies text/usage without `next`, so no main request
  or core trace exists for it. The SDK retains only preparation. Native completion
  blocks the unnamed answer; later genuine requests repair and complete the same
  Turn, without substituting their dispatch for the original step-zero candidate.

All reviews stay Unknown with one frozen handoff and no acknowledgement,
consumption, proof observation or resend. Live/exit/post-shutdown records agree.
Twenty-one altered-evidence checks pass. No new unit/build/Windows result is
claimed. [Exact protocol, sessions, hashes and retained harness issues](https://github.com/aroff/codelaya/blob/5ab9259/docs/native-input-competitors.md).
This verifies the existing guard in those cases; interruption, context-size and
arbitrary late-mutation/effective-policy qualification remain incomplete.

## In-flight input interruption and original-session resume

Codelaya [qualification source `69fe14d`](https://github.com/aroff/codelaya/commit/69fe14d)
uses the same unchanged runtime `8ff3d89` / application binaries `39e706a` in one
fresh ordinary native 2.1.308 session. After initial authoring and manual idle
compaction, the controlled endpoint receives the first actual feedback request,
reads the committed version-two input preparation and holds its response. The
verified isolated container receives SIGKILL 483 ms after that held-request marker.

Cold inspection precedes all application/native restarts and retains the exact
preparation, stored row and one already-returned handoff, with no dispatch or
proof observation. App/SDK were already Unknown at this later boundary. The
same native session resumes under a new invocation without a work prompt;
unfinished work becomes Incomplete/recovery, original Completed work and all
review/checkpoint bytes stay unchanged, and no model request or resend occurs.
Normal exit preserves the state. Nineteen altered-evidence checks pass, including
duplicate frozen native rows and invented dispatch/proof/replay.

[Exact protocol, timing, identities, hashes and scope](https://github.com/aroff/codelaya/blob/69fe14d/docs/native-input-recovery.md).
This is one deployment-supervised process-loss window, not arbitrary power loss,
native receipt authority, continued new work after recovery or Windows acceptance.
No runtime/schema/capability grant changes; acknowledgement/consumption/readiness
remain unqualified. Context-size and effective-handler controls remain open.

## Continued work and distinct new feedback after recovery

Codelaya [qualification source `0f44323`](https://github.com/aroff/codelaya/commit/0f44323)
continues in that same recovered native session/store with unchanged SDK `8ff3d89`
and application binaries `39e706a`. A new explicit owner prompt creates a fresh
binding under the current invocation of the original native session. Native
context/name/Write/fresh-note/completion succeed, then a distinct new owner CLI
review receives complete same-session feedback and accepted completion.

The original Completed and Incomplete records and old Unknown review remain
exact, including old target/reservation/row/handoff/pending input. The new review
has a separate target binding/current invocation and matching input candidate.
Nine actual main requests and twelve altered-evidence checks pass. Final history
is Completed, Incomplete, Completed, Completed; both reviews remain Unknown.
No manual binding, authority restoration, reinstall or uncertain resend was used.

[Exact protocol, identities, hashes and retained verifier-premise failure](https://github.com/aroff/codelaya/blob/0f44323/docs/continued-work-after-recovery.md).
Normalized input events retain correlation IDs rather than raw prompt bytes;
the verifier uses actual transcript/HTTP bytes plus the allowed event/Turn ID.
This closes that bounded functional qualification gap. Context-size, effective
configuration, qualified receipt/reconciliation and Windows remain incomplete.
No runtime/schema/readiness grant or new Rust/build result is claimed.
