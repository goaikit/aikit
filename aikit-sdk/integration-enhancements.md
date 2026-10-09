# External-session integration gaps

These are implementation requests backed by local source inspection and bounded
native evidence. They are not promises of supported behavior. AIKit owns native
translation and qualification; consumer applications keep their own review rules.

## Cursor: Windows native hook dispatch

The 2026-10-09 ordinary-path control isolates an SDK input issue: native Windows
hook stdin prefixes JSON with a UTF-8 BOM. The decoder now consumes one transport
marker while preserving its size, scope and privacy checks. A fresh matched
native run records one SDK tool Block and prevents the gated write. The same
fixed handler still fails the punctuation/Unicode path probe before journaling.
Use `-OrdinaryWorkspace` for the control; retain the default path stress case.
See the qualification record for exact digests and limits. Neither result
promotes completion or messaging contracts.

The fresh 2026-10-07 probe using SDK source 972807b and Cursor
2026.09.02-c22c1a3 reproduced the earlier Windows print failure. Without hooks,
the requested Write created its file. With owned hooks, the native generated
`ps-script-*.ps1` failed parsing at line 54 around
`FromBase64String(''{1}'')`, before the SDK executable ran. Both processes exited
0 without timeout; the gated file was absent and the SDK journal had zero records.

Reproduce using `examples/cursor-qualification/run.ps1` and its README. The
script retains matched native streams, process results, SDK events and executable
digests; it fails if no SDK BeforeTool Block exists. It removes only owned
fixture hooks in `finally`. A failed native wrapper is not an SDK policy denial.

Request a repaired native Windows command-hook wrapper and qualify its literal
command/argument handling, both stdin modes, UTF-8, punctuation/Unicode paths,
stderr/exit forwarding and timeout behavior. Then qualify actual prompt admission,
tool allow/deny/failure and session replay through AIKit. Preserve failClosed and
workspace scope. The SDK encoded bridge's real-process tests cannot repair a
provider wrapper that fails before invoking it. Completion and messaging require
their separate contracts; repairing dispatch alone does not qualify Codelaya.

## Pi: authoritative completion settlement

### Bridge invocation replacement isolation

The local bridge now scopes shutdown cleanup and continuation state to the
invocation captured before its asynchronous handler request. A shutdown that
settles after a replacement SessionStart no longer clears the replacement's
UUID. A stale completion callback still blocks its own decision but cannot set
the replacement's `stop_hook_active`; failure correlation also checks invocation.

Synchronized real Node child handlers reproduced both prior failures: the new
invocation's input became handled after old shutdown, and its first completion
was incorrectly marked as a continuation. The bridge regression now pauses each
old child after its request is recorded, starts the replacement, releases the
child and asserts identity plus first/subsequent continuation state. This is
generated-bridge process evidence, not an installed native Pi lifecycle claim or
completion/delivery capability promotion. The correction remains local pending
upstream publication authorization and is not in Codelaya's pinned dependency.

### Linux Pi 0.82.1 compatibility gap, 2026-10-07

Installed Linux x86_64 Pi 0.82.1, exercised under Bun 1.4.2 with the unchanged
qualification `provider.mjs`, runs four bounded print scenarios: normal, steer,
followUp and missing delivery mode. All exit 0. The installed extension event
union includes `agent_settled` but no `agent_before_settle`; the four actual runs
emit no proposal event and each settles with event keys exactly `type`.
This runtime cannot inherit the recorded Pi 1.0.4 completion proposal scenarios.
The existing generated bridge depends on that proposal event. Report the missing
version-specific native contract before attempting completion qualification;
do not replace it with agent_end/settled or process exit.

Installed source evidence digests:
`dist/core/extensions/types.d.ts`:
`d3fb9d55d312e47df861507aeef41c1183261c3a577588dc2ddb1c97cc909d6e`;
`dist/core/agent-session.js`:
`d300f57a70b7ca3f86e8e41b5336b0579268cbcecfedf1d99c176f2add2dd39b`.
The extension API declares `sendUserMessage` void and the runtime forwards its
asynchronous rejection to the extension error stream. Native queue probes reuse
provider SHA-256 `13ab6aca2793570b8eb945bcdc6bb0b34dfeba5757ff35536315263b715420c0`:
steer/followUp each deliver one fixture message in the second model context;
missing mode returns undefined, then reports the already-processing error with
no consumption. No SDK hooks are installed, profile/state are isolated, and no
external model account is used. This is a candidate queue API, not durable SDK
messaging or completion enforcement. Native identity, interactive mode, queue
acknowledgement/reconciliation and restart lifetime remain unqualified.

The local SDK capability report now marks CompletionDecision,
RepeatedCompletionBlocking, FinalAnswerCapture and FailedCompletionObservation
Unsupported for exactly Linux/x86_64/print/Pi 0.82.1. The translation code remains
implemented, but its required proposal event is unavailable in this recorded
context. Regression cases retain Unknown for other modes, platforms,
architectures, versions and missing probes; local install/replay/binding/detach
semantics are unchanged. This change is not yet published or consumed by the
Codelaya pinned dependency. It does not claim native enforcement or add a fallback.

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

### Pi native queue feasibility, 2026-10-07

The existing qualification harness now has a MessagingOnly mode. Two runs on
Windows Pi 1.0.4 / Node 24.19.0 use the real extension API, native loop and a
controlled provider. sendUserMessage with steer or followUp delivers one Unicode
fixture message to the next model context. Without a streaming delivery mode,
the wrapper still returns undefined, then asynchronously reports the already
processing error; no fixture message reaches the model. Thus neither void return
nor absence of a synchronous exception proves acceptance or consumption.

This advances the candidate bridge feasibility, not IntegrationService messaging.
Before exposing a supported session transport, qualify or request a correlated
native acknowledgement/error contract: stable operation id and session generation,
observable enqueue/consume/reject evidence, lookup/reconciliation and explicit
shutdown/restart queue lifetime. SDK durable records must preserve uncertainty
across the dispatch boundary and never replay a message merely because a native
error or acknowledgement was lost. Reuse this existing API where its semantics
match, without substituting managed gateway process ownership for attachment.
