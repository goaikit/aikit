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
