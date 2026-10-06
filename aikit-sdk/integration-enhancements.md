# External-session integration gaps

These are implementation requests backed by local source inspection and bounded
native evidence. They are not promises of supported behavior. AIKit owns native
translation and qualification; consumer applications keep their own review rules.

## Pi: authoritative completion settlement

Observed on Pi 1.0.4, Windows x86_64 print mode, Node 24.19.0 and SDK fff6aa7:

- An extension ran after a recorded SDK Allow and called `ctx.abort()`. The native
  settlement event contained only `type`, exactly as in a normal completion case.
  `ctx.signal` was absent at both the proposal and settlement boundaries. The
  earlier boundary's `outcome: completed` did not identify the later abort.
- Another extension ran after a recorded SDK Block, observed `continue: true`,
  returned `continue: false`, and the agent settled after one model call. There
  was no subsequent SDK Allow.

Reproduce using `examples/pi-qualification/run.ps1`. Both cases run Pi's real
extension dispatcher and agent loop with a deterministic provider. The probe
verifies the preceding SDK decision before acting. These are counterexamples to
completion inference and enforced continuation in this context, not broad claims
about every platform/version/mode.

### Required native API contract

1. Issue a stable invocation/run/proposal identifier at each completion proposal.
   Settlement must reference the proposal actually accepted after all handlers.
2. Report the final outcome after aborts, errors, retries, continuations and all
   handler processing: completed, aborted, failed, or an explicit unresolved state.
   A pre-handler outcome cannot substitute for this final result.
3. Provide a required-gate registration mechanism with monotonic denial: later
   extensions cannot cancel its Block. If continuation cannot run, surface a
   blocked/failed state. Owner termination remains an abort, not accepted completion.
4. Include the final assistant message identity or immutable evidence reference so
   AIKit can associate the accepted answer with the authorized proposal.

AIKit should translate that native evidence into a separate correlated completion
observation, persist it in the existing journal, and qualify abort/override/replay
cases before promoting capabilities. An application must recheck its authorized
review revision before closing a Turn. No new generic policy framework is needed.
Until this contract exists, retain Unknown repeated-blocking qualification and
Unsupported successful-completion observation. Do not infer success from Allow,
agent_settled, SessionEnd or exit code.

## Pi: extension identity and loading

The SDK now assigns invocation scope through the generated extension. The native
loader should canonicalize Windows path aliases before deduplicating explicitly
listed and automatically discovered copies of the same extension. Native process
identity, resume/reload ordering and previously unseen delayed starts still need
an authoritative runtime contract and qualification. The SDK UUID is not a token
for authentication or proof of liveness.

## Existing-session messaging

AIKit still needs native dispatch, durable operation receipts and reconciliation
for user-started sessions. Managed gateway transports own the processes they start
and must not be used as an attachment mechanism. Pi's public extension messaging
functions are a candidate bridge; qualify their queue acknowledgement and lifetime
before connecting them to the existing session binding. Preserve immutable
operation IDs/content and never blindly replay an unknown handoff.
