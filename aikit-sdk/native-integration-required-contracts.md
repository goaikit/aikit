# Required native contracts for user-started agents

Implementation request, audited 2026-10-23 against SDK runtime `765a1ce`.
Track under [draft PR #196](https://github.com/goaikit/aikit/pull/196); this
repository has issues disabled. This requests reusable SDK/provider work, not a
new application policy framework. Nothing here promotes a capability.

## Current API investigation, 2026-10-24

[Actual native API/history reuse](native-api-reuse-20261024.md) verifies a kernel
socket peer fact, a durable append UUID and an existing stored-review lookup
path. AIKit already has a HistoryReader; its Claude dependency had a leaf-selector
bug returning zero messages behind control rows. Dependency `57ad0ec` fixes it:
the unchanged adapter reads 32 messages and the exact review UUID/full frame.
Do not invent another parser or classify stored-row lookup as absent. This is
not yet Codelaya receipt-reconciliation wiring or authoritative consumption.
Canonical history omits ingress provenance; live generation/configuration and
N1/N3 qualification remain. No public API/schema/capability is added or promoted.

The [installed API-view review and guard](claude-api-view-provenance.md) identifies
another SDK implementation gap that existing native core traces can close.
Hookable conversation reads now require one matching core result and an unchanged
final API value. This scoped check does not attest final model input, effective
policy or durable receipt lookup. Do not classify its API availability as a
provider blocker or promote N1-N3 on the strength of a plain returned array.

The [continued-work follow-on](message-input-checkpoint.md#continued-work-and-distinct-new-feedback-after-recovery)
verifies current binding admission for explicit new owner work and a distinct new
review in the recovered native session. Original history and uncertain target
remain unchanged; fresh native Name/Note/completion and new feedback succeed.
This closes a healthy functional-path gap through existing APIs without granting
production receipt authority or changing the remaining size/policy/Windows gates.

The [in-flight input interruption run](message-input-checkpoint.md#in-flight-input-interruption-and-original-session-resume)
now verifies version-two preparation survives supervised process loss and
original-session resume without an invented dispatch or resend. The already
Unknown review stays Unknown; interrupted work becomes Incomplete/recovery.
This qualifies one current implementation window, with acknowledgement/consumption
and production capability still unqualified. It is not evidence of an absent
provider API or a substitute for the remaining size/context/policy matrix.

The [native competing-handler comparison](message-input-checkpoint.md#competing-handler-matrix-on-the-same-runtime)
qualifies the candidate guard against two specific later handlers on 2.1.308:
changed step effort and synthetic text/usage do not create a matching dispatch
observation. A matched unchanged control does. Synthetic unnamed completion is
blocked and repaired normally; its later requests never replace the original
prepared step. All operations remain Unknown. This closes scoped qualification,
not production receipt/reconciliation or arbitrary handler-policy enforcement.

The [operation-correlated input candidate implementation](message-input-checkpoint.md)
now reuses the existing append record and public message bridge to retain a
pre-dispatch snapshot digest and a separate matching settled-core observation.
Record version 2 is explicit; SQLite schema remains 15. These candidates do not
advance delivery, acknowledge or consume. Fresh matching native qualification
and the failure matrix remain required before this can close N3.

The [fresh complete frozen-review comparison](recorded-admission-wait.md#fresh-consumer-comparison)
now verifies full framing in pre-dispatch snapshots and three actual feedback
main requests after compaction, in the original Claude 2.1.308 session. AIKit
adds an opt-in bounded wait before unchanged recorded admission checks. The
first consumer failure is retained; the fresh trial completes both Turns.
Production receipt/reconciliation and the competing-handler matrix remain
unfinished implementation and qualification work, not demonstrated absent APIs.

The work packages below are remaining integration/qualification requirements,
not proof that Claude lacks each primitive. The [new native input comparison](../examples/claude-input-qualification/README.md)
shows that an API snapshot **after** a real dispatch can contain an appended row
that was absent from that request. A pre-dispatch snapshot agreed in this case.
Implement and qualify that existing API/core-trace path before requesting a new
input-witness API. Its full framing, race, compaction and recovery matrix remains
unfinished; durable receipt/lookup is separately unresolved.

AIKit now [confirms the exact reservation on Recheck](message-reservation-confirmation.md)
before native handoff. This closes an SDK consistency gap without claiming a
native lease. Claude's documented inbox socket/token is also an existing delivery
and process-context candidate; it does not by itself attest the mandatory handler
chain. Preserve the distinction between SDK work, unqualified native behavior and
a demonstrated native counterexample. No new provider blocker is asserted here.

The [completion trace implementation](native-completion-trace.md) now uses an
existing native primitive to reject downstream short-circuit/contradictory core
outcomes before forwarding them. This closes a concrete observer code gap; it
does not supply a whole-chain configuration witness or finish N1–N3.

The [current native dispatch probe](../examples/claude-dispatch-qualification/README.md)
confirms that core traces distinguish a real main-loop request from synthetic
handler text/usage. Its broad no-HTTP-send premise failed because separate native
title requests contained the same prompt; that failure is retained. The core
trace still supplies no exact input witness or durable operation receipt in this
capture. Extend the existing provenance primitive; do not request it as though
no native trace API exists, or promote title input to target-Turn consumption.

The consumer's [fresh current-version lost-response run](https://github.com/aroff/codelaya/blob/feat/windows-local-workflow/docs/native-feedback-recovery-current.md)
retains the exact native row across supervised SIGKILL and recovers the original
operation to Unknown without a new main request. In that capture, native queue
records have no payload/operation ID. Their one-enqueue count does not reconcile
a named operation; the original payload-correlation verifier premise fails and
is retained separately. This qualifies one recovery window, not native receipt
lookup, power-loss durability or acknowledgement/consumption.

## Existing public surface to reuse

- `IntegrationService` installation planner/journal, capability reports,
  `handle_hook`, `HookHandler::decide`, native event normalization and replay.
- `SessionRef` / `SessionBinding` for installation revision, native session,
  generated invocation and application binding. Current recorded guards check
  journal identity; they are not native process authentication or liveness.
- `SessionBinding::stage_message`, `reserve_message`, `message_status`,
  `recover_message_record`, `record_message_observation`, `message_observations`.
- `MessageReceipt` keeps immutable content/digest, one reservation and separate
  acknowledgement/consumption/failure. `MessageProof` references retained evidence.
- Owned callable/instructions/completion/message modules and append checkpoints.
  Their raw handoff/storage returns remain candidates, not production proof.

Keep provider framing, configuration, native transport and qualification in
AIKit. Keep consumer review policy and its projection/cursor transaction in the
consumer. Do not add another daemon, provider registry, integration crate,
message state machine or managed-session substitution.

## Three blocking work packages

| ID | Missing native fact | SDK work using existing surfaces | Required acceptance evidence |
| --- | --- | --- | --- |
| N1 | The actual native caller/generation and effective mandatory handler chain for this decision or handoff, with trustworthy provenance and invalidation on change | Inspect the provider's real API first. Validate a native-host-verifiable witness at binding and decision/dispatch boundaries. Extend existing reference/context checks only where the real primitive requires it; a generated nonce, disk fingerprint, version string or middleware settings return cannot attest facts it does not supply. | Reject foreign/unseen delayed/replaced contexts; configuration/version changes invalidate admission; no stale context can act after a replacement. Qualify the actual terminal surface. No account login/credential forwarding is a substitute for local invocation evidence. |
| N2 | Mandatory completion denial and correlated final native acceptance after all handlers, including native failure/deadline handling | Reuse `HookHandler`, owned completion module and native Turn observations. Establish protected execution plus effective continuation/failure behavior; retain the latest valid proposal/revision/answer through final acceptance. | Missing/stale authoring repeatedly blocks. Competing handlers, timeout, worker loss, cancellation and abort-after-Allow never create false successful acceptance. Report inability to enforce as unavailable/failed, not native completion. |
| N3 | Atomic original-session admission through queue/handoff, independently correlated durable acknowledgement and actual model-input consumption | Implement the real provider transport around existing immutable reservation/receipt APIs. Validate native framing and resolve retained proof references before `record_message_observation`; reconcile through lookup, never resubmit Unknown. No SQLite transaction across native I/O. | Busy-to-idle same-session delivery; exact frozen UTF-8 payload in native input; separate ack/consumption in either arrival order; crash before/after handoff, lost ack, changed content, duplicate IDs, session/generation replacement and restart queue lifetime. A raw return, model echo or scripted endpoint capture alone is insufficient production proof. |

These are behavioral contracts, not finalized new Rust signatures. Do not invent
a generic attestation API until an actual provider primitive supplies the stated
facts. If the current provider has no such primitive, report a concrete provider
enhancement request and keep the relevant assessment Unknown/Unsupported. If it
does, implement/qualify its adapter rather than replacing it from scratch.

## Evidence already available and its limits

The [live replacement-session follow-up](https://github.com/aroff/codelaya/blob/ac4a21d/docs/native-worker-live-recovery.md)
adds actual fresh owner work plus one new review without a native CLI restart.
Old Incomplete history is unchanged; two new Turns have native accepted outcomes
under the replacement invocation. SDK retains one Unknown handoff with no proof
promotion. Existing sidecar reconciliation after exit projects the application
review and matching HTTP history; live UI settlement was not exercised. This
closes the scoped fresh-work question, not missing old-outcome recovery or N1/N3.

The current [full consumer worker-replacement report](https://github.com/aroff/codelaya/blob/8b52ff0/docs/native-worker-replacement.md)
adds one after-Allow N2 safety case with unchanged generated SDK/application:
the host invokes the SDK catch after replacement, lost Turn context blocks, and
the durable consumer Turn remains Incomplete/recoverable with no ending snapshot.
The later abort is not journaled because the module lost Turn identity. Preserve
that explicit outcome-observation gap; do not reconstruct completion authority
from the retained Allow. Fresh owner work after replacement remains to qualify.

Current 2.1.308 [mod budget/worker recovery](../examples/claude-mod-qualification/current-mod-failure-20261024.md)
demonstrates an existing solution for two N2 failure primitives: native timeout
and actual wedged-worker replacement invoke the exact constant SDK fallback,
whose Block reaches the next model request. Full generated SDK/Codelaya context
recovery after replacement, protected execution and other failure cases remain
to qualify. This is no longer a missing generic provider worker-loss mechanism;
reuse it before proposing new interfaces. No N1/N2/N3 capability is promoted.

Current 2.1.308 [managed Stop failure comparison](../examples/claude-mod-qualification/current-stop-failure-20261024.md)
qualifies protection against the tested short-circuit competitor using the
existing built-in managed guard. Corrected root-owned baseline Blocks then
Allows; missing/timed-out commands still yield settled native core completion
despite `onFailure:"block"`. Three earlier trials failed the ownership gate and
remain retained. N2 is not closed for this exact command path; the separate
owned mod fallback's worker loss/protected execution remains unqualified. This
is not a claim that every native API lacks enforcement, or that Codelaya itself
incorrectly completed a Turn. No capability or dependency pin changes follow.

Claude Linux interactive 2.1.308 automatically discovers the owned directory at
user tier without profile/marketplace installation. Bounded native consumer
trials demonstrate authoring, repeated name/note Blocks, accepted completion,
cancellation/recovery, compaction and exact executable-loss recovery. SDK
integration has 174 passing tests on runtime `765a1ce`; none supplies N1–N3 in
full. This does not transfer older 2.1.296 counterexamples to a new version.

- [Current client inspection](../examples/claude-mod-qualification/current-client-20261023.md):
  account identity is not local invocation authentication; middleware settings
  reads are not an independent handler-chain witness.
- [Stop-chain comparison](../examples/claude-mod-qualification/stop-chain-protocol.md)
  and [guarded command failures](../examples/claude-mod-qualification/stop-failure-protocol.md):
  exact Linux 2.1.296 counterexamples, not universal claims about all versions.
- [Generated completion bridge](native-completion-bridge.md): constant fallback
  and context/cancellation checks exist; native worker loss/protected execution
  remain separate gates. Do not discard this implementation and start again.
- [Append candidate](../examples/claude-mod-qualification/append-acknowledgement-candidate.md):
  real stored envelope differs from logical payload; raw storage and input
  evidence need validated framing, provenance and generation scope.
- [Provider-specific gaps](integration-enhancements.md): Cursor dispatch and
  completion gaps, Codex queue non-idempotence, Pi proposal/settlement/continuation
  limitations retain their exact platform/version/mode scope.

## Consumer consequence and completion

Applications can register and read retained history while these gates fail.
Production activation receipts and qualified owner grants must remain refused
until required contextual capabilities and actual-session checks pass. An
activation implementation cannot replace the missing native facts with a
caller-supplied readiness flag. No owner policy/credentials should be changed
implicitly to manufacture a passing configuration.

Finish N1–N3 first for the actual Claude terminal, then equivalent Cursor Agent
CLI, Codex CLI and Pi adapters. Record source/runtime hashes, native version,
surface, platform/architecture, reproducible commands and retained evidence for
each contract. Linux Docker checks may continue; deferred Windows acceptance
remains required for the downstream Windows delivery. Successful builds,
Storybook, synthetic journals and Claude-only success do not finish that target.
