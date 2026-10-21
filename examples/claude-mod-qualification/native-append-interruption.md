# Native append interruption, 2026-10-21

One ordinary Linux Claude Code 2.1.296 session was killed after native append
storage but before its caller received a submit return. The original native
session then resumed without another submission. Same pinned native image and
executable as the [baseline capture](append-acknowledgement-candidate.md).
Starting sources: AIKit `7e41564`, Codelaya `3e18fa6`.

## Executed scenario

1. An empty public-image container completed normal onboarding with the public
   dummy key and no mounts, owner profile or credentials. External networking
   was removed before copying the diagnostic probe or granting workspace trust.
2. `hooks/append-interruption-probe.js` registered one diagnostic slash command.
   The command exclusively persisted a fixed operation/session/payload claim
   before one `prompt.submit`. Checkpoints use the fixed-path helper
   `append-interruption-journal.mjs`, exclusive creation, file fsync and directory
   fsync. This helper is diagnostic and is not AIKit's production journal.
3. Before delegating `session.append`, the probe persisted its native UUID,
   exact provider envelope and session/operation. After `next(event)` returned,
   it persisted the returned row, then held that callback in a bounded helper
   process so the controller could kill at the intended boundary.
4. The controller observed `stored.json`, sent SIGKILL to this owned container
   and inspected authoritative Docker state: exited, code 137, not OOM killed.
   The retained probe log has one attempt and no submit return. The native user
   row with the checkpoint's UUID was already present in the transcript.
5. The same offline container restarted. `claude --resume ORIGINAL_SESSION_ID`
   loaded the original session, and the plugin read its existing durable claim.
   It observed the retained message and submitted nothing. Running the slash
   command again returned “Attempt already claimed”; no second enqueue occurred.
6. Normal exit retained the same user row and UUID. No loopback model-request
   journal was created before the kill or after resume. No actual model-input
   consumption is claimed. All owned native processes/container were removed
   after retention and independent verification.

The native `session.messages()` view on resume contains text/tool summaries but
no row UUID. Matching text in this view cannot independently reconcile a native
row identity. The separate persisted transcript supplied the UUID comparison in
this controlled fixture. Qualify existing spawn-free history readers and native
source/origin/invocation protection before using that path as production proof.

## Implemented SDK consequence, separate evidence

The existing integration journal now offers
[candidate append checkpoints](../../aikit-sdk/docs/message-append-checkpoints.md)
through `SessionBinding`. They retain the immutable reservation, pre-storage row
identity and expected framed digest, plus a separate post-storage digest/evidence
reference. They do not send, infer acknowledgement/consumption, reset an attempt,
or authenticate the native caller. Exact replay is idempotent; changed scope or
row reuse conflicts. The additive schema is 12; older readers refuse it.

**The native run above did not use the new Rust checkpoint methods.** It used
the diagnostic helper. The Rust API has separate non-CLI integration tests for
reopen/lost return, rewrite mismatch, foreign scope/reuse, historical late data,
bounds/corruption and migration. Wiring the owned native module to that API and
qualifying the combined failure path remain required implementation work.

This is one deployment-supervised Linux container kill, not power-loss durability,
standalone Windows child cleanup, all failure boundaries, protected native
reconciliation or a real-model semantic run. No SDK capability or application
readiness is promoted. Windows remains deferred by the owner.

## Reproduce and evidence

Use a fresh empty container for normal onboarding, disconnect external networking
before source copy/trust, select `append-interruption-probe.js` in the copied
plugin manifest, and run the ordinary loopback fixture. Submit
`/aikit-interrupt-append` once. Kill only the owned container immediately after
`stored.json` exists; do not let its 60-second hold expire and call it a crash.
Retain its state, checkpoints, events and native transcript while stopped.
Restart that same container and loopback endpoint, resume its recorded session,
repeat the diagnostic command to verify refusal, then exit normally. Retain
separate before/after transcripts and message-list view.

`verify-append-interruption.mjs EVIDENCE_DIRECTORY` verifies session/operation,
pre/post-storage UUID/envelope, Docker termination, absence of submit return,
one enqueue and one same native user row before/after resume, no new submit and
absence of loopback request logs. It passed in Linux and on retained evidence.

Raw evidence: parent chat `.local/claude-append-interruption-20261021`.
`verification.json` includes all ten capture hashes; no owner profile is retained.

| Source/evidence | SHA-256 |
| --- | --- |
| Executed diagnostic checkpoint helper | `57b4677462c9ff0f69b975f6b94ca69722c29c787b075cc6c29d009f5067d04d` |
| Executed native probe | `b883f161efb3dd4c06c9d2041879b175452c5e09906128f45b9749b1f52de446` |
| Independent verifier | `55e0cea3909a1d9fccb5ed89dea39f6364c715d26790228c059068f9bdcc2ef7` |
| Prepared UUID/envelope checkpoint | `1b361b4d99f67e7bbae7a3b36f88e2b63f718926e703bf2cdcf2479cabaf0e31` |
| Stored-row checkpoint | `f04e72a8c0841d615601c7709c3189dbaf7727d9fe0e2612bedd7362fca77c1d` |
| Native transcript after kill | `57e0a98a1c6b8f059eb60584d8c1ff6415730ce05b5c93f8f82bfd3f36f2c016` |
| Native transcript after resume/exit | `409debdddbf326fe1d76229784ecb0cd2e0bde57faba4b3207782897ce920432` |
