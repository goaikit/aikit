# Current Claude model-dispatch provenance experiment

Predeclared 2026-10-24, before native execution. This tests an existing provider
primitive for AIKit N3; it is not production message delivery or consumption.
No readiness promotion or Windows checks.

Use the existing pinned Linux Claude 2.1.308 image/executable, a fresh container
with network `none`, no owner mounts/settings/credentials and only the public
dummy key. The loopback gateway is bounded to six HTTP requests and 4 MiB per
request; it never proxies. Preserve failures and do not relabel handler ordering.

The observer is configured at prepend tier in a disposable user profile; the
shortcut is at user tier. Require actual native debug to confirm both. Offline
onboarding is explicitly seeded only in this fresh test profile; accept normal
workspace trust in the terminal. Validate both plugins before the run.

In one ordinary interactive terminal, submit `AIKIT_DISPATCH_CONTROL` and wait
for completion, then `AIKIT_DISPATCH_WITHOUT_ENGINE`. Do not use a managed session.
The control must produce one actual main HTTP request and a matching settled
engine/core `turn.step` trace. The shortcut yields synthetic text/stop/usage and
returns without `next`; require the observer to see its API-view marker/result
but no matching core trace and no actual HTTP input containing that marker.
Normal exit must retain the same session and both native Turn observations.

If ordering, schema or streaming semantics differs, retain the failure and
report that premise unproven. Do not change a submitted native trial to obtain
the intended result. Require exact source hashes, native version/image, actual
API inputs, trace entries, transcript and independent verifier before reporting.

This can demonstrate why API-view + returned usage is insufficient, and whether
core traces distinguish omitted native dispatch. It cannot establish exact
wire-input inclusion, immutable native operation receipts, crash durability,
lookup reconciliation, whole-chain configuration or real-model semantics. A
successful core trace must not be promoted to production consumption by itself.
