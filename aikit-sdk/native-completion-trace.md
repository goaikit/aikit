# Claude completion observer: require a matching native engine trace

The owned Claude module now checks `next.origin` and the settled `next.trace`
before forwarding a main-thread `AIKitNativeTurnFinished` to the existing handler.
Delegation alone is insufficient: a later module can answer without reaching the
engine. The previous observer would still forward the incoming outcome.

For an installation with a session handler, require an engine/core dispatch
before delegation, then exactly one engine/core `turn.complete` trace entry with
outcome `returned` or `passed`. Its received Turn ID, agent ID, reason, abort flag
and exact answer must match the incoming event. Missing, skipped, rejected,
duplicate, foreign or rewritten core runs produce no outcome handler call. The
downstream callback is never retried, and its successful return is preserved.
Subagent and tool-only exports retain their existing behavior.

This reuses the provider's existing origin/trace API; no public method, transport,
schema or dependency is added. Re-render/update the owned module through the
existing installation journal because its fingerprint changes.

This is a downstream-chain consistency check, not authentication of the stdio
child, proof of the whole effective configuration or a non-bypassable Stop gate.
An earlier handler could keep this module from running; absent observation is
unavailable evidence, never accepted completion. Mandatory capabilities remain
Unknown/Unsupported until their full contextual requirements are qualified.

## Validation

The new negative module tests fail on the previous implementation. Linux Docker
Node 22 passes all 30 existing/new outcome, completion and message module tests.
New cases refuse missing/foreign/changed/skipped/rejected/duplicate core traces
and foreign dispatch origins without handler I/O. Matching answer/abort/refusal/
error paths preserve their outcomes and return values. These are engine stubs;
native qualification is recorded separately and Windows checks remain deferred.

## Native consumer qualification, 2026-10-24

Three fresh network-isolated ordinary Linux Claude 2.1.308 sessions used the
actual generated module and Codelaya handler pinned to SDK runtime `2f8b3b6`:

- Healthy automatic discovery: two distinct completed no-edit Turns across idle
  `/compact`, one invocation, no observer warning and retained normal-exit history.
- Automatic-discovery competitor: a user-tier competitor ran above the observer
  and skipped it. No native outcome was recorded; Allow alone stayed unaccepted,
  then normal exit saved Incomplete/recovery. This failed the intended downstream
  ordering premise and is retained as separate upstream-bypass evidence.
- Separate disposable profile-ordering case: the SDK module actually loaded at
  prepend tier, above the user-tier competitor. The competitor answered without
  reaching core; the SDK's exact trace-rejection error appeared in native debug.
  No outcome was emitted, and exit preserved Incomplete/recovery and work bytes.

The first negative setup attempt passed the literal `undefined` as a fault and
was rejected before any native launch; the qualification helper now has an
explicit `none` selector. No submitted trial was repaired or relabeled. Only
disposable container settings changed; no owner profile/policy/credentials did.

SDK integration: 174 passes. Consumer integration/completion: 26 passes.
Independent native evidence checks pass, with 16 healthy-control mutations,
12 upstream-bypass mutations and 13 downstream-guard mutations (41 checks, not
41 native sessions). All replies use bounded deterministic loopback endpoints.
No build container ran during the native trials. Containers were stopped after
regular-file capture, preserving the owner's separate preview.

[Downstream protocol, exact binaries, raw evidence locations and limitations](https://github.com/aroff/codelaya/blob/feat/windows-local-workflow/examples/registered-native-qualification/native-completion-trace-protocol.md).
The raw origin/trace checks establish only these observed chain cases, not N1–N3
in full. In particular, the upstream-bypass result preserves the need for
effective configuration/protected execution evidence. No production activation,
feedback acknowledgement/consumption or native Windows qualification follows.
