# Existing-session feedback feasibility, 2026-10-18

`hooks/feedback-probe.js` is explicitly opt-in. The default plugin manifest still
loads `register.js`. This is an ephemeral diagnostic rendezvous, not the SDK's
durable messaging implementation or a readiness override.

## Native evidence

Claude Linux 2.1.296, ordinary interactive terminal, fresh unprivileged profile,
public dummy key and scripted loopback replies. No host credentials, policy
changes, permission bypass or managed agent launch. External networking was
removed before trusting/loading the disposable workspace. Native plugin
validation recognized `clock.every`, `fs.exists/read/write`, `prompt.submit` and
`session.id`. The image is the same pinned image recorded in
[the native gateway report](native-gateway-probe.md).

After a Codelaya Turn completed, its real owner CLI froze a review with Unicode
content. A background timer read that exact payload, verified its SHA-256 and
target session, and called `$.prompt.submit({text})` once while idle. No second
CLI or typed follow-up was used. Native return contained the exact payload and
plugin origin. The original session started a new native Turn, and the next
actual outgoing main API input contained the complete frozen payload once.
Subsequent tool-result requests retained it. The follow-up named a new application
Turn and completed; the original Turn was byte-for-byte unchanged in serialized
application state. Both Completed records survived normal exit.

The independent Codelaya `verify-feedback.mjs` checked native/session/generation
correlation, frozen review bytes, actual API inputs, agent authoring and persisted
records. It passed inside the container and against retained Windows-host files.
Nine main requests: six initial authoring/edit/note requests, three follow-up
context/name/answer requests. No production acknowledgement/consumption was
invented: the owner review remained Queued, with null evidence projections.

| Retained item | SHA-256 |
| --- | --- |
| Outgoing requests | `0c8062639ab8c47e8333aa9dd7b86d8e91a12217b1c8f26e27f92829bdf76a8c` |
| Native probe observations | `c270cf7ba77be598024ea91c9af6f48767ea00834fa632cc17e8674ab47efc1d` |
| Executed feedback probe | `53e86ed0d389bcc5d31c8b3c0f473ad76ad58e707bb9f4e8449a9b0a5cecd871` |

Raw synthetic contexts remain local in the Codelaya implementation chat under
`.local/native-completion-generated/feedback-accepted`. The first attempt is
retained separately as `feedback-framing-negative`: delivery woke the correct
session, but the scripted gateway did not recognize string-form user messages
after the first feedback tool result. Codelaya correctly blocked the unnamed
Turn until the bounded fixture ended. The parser was fixed to handle both native
text representations, followed by the fresh successful session above. That
existing review was never resent. All owned containers were removed.

## Remaining reusable implementation

- Implement the planned `SessionBinding` messaging/status/reconciliation API in
  the existing integration store, with immutable operation ID/content/target.
- Reuse owned module installation and fixed-command transport; replace this
  fixture file with a durable SDK claim/observation protocol. One attempt in
  memory is not crash-safe. Never hold SQLite transactions over provider I/O.
- Enforce original generation, detach/replacement and native busy/input admission
  through handoff. Idle-only feasibility does not qualify those races.
- Persist native acknowledgement separately from actual input consumption. A
  successful prompt return or Turn start alone is not consumption evidence.
  This fixture independently inspected the outgoing request at its test endpoint;
  it does not establish a production consumption contract.
- The retained target-generated declarations (hash in the gateway report) expose
  `$.session.messages({as:"api"})`, described as normalized messages from which
  the next request is built, with omissions and a 4096-message limit. Qualify this
  candidate at native `turn.step` against actual outgoing inputs before using it;
  plain transcript text is insufficient. `/plugin-types` is not a command in
  this target client; use the previously retained generated declarations.
- Native Windows, busy queueing, process death/unknown reconciliation, competing
  prompt transformations, real-model semantics and other providers remain open.

No production Rust code, capability assessment or consumer SDK pin changes in
this evidence step.

## Native API-view candidate, 2026-10-18

A subsequent fresh session added `turn.step` sampling to this opt-in probe.
Before delegating the stream it captured `session.messages({as:"api"})`, native
session/Turn/index; after delegating it recorded the step result separately.
The unchanged owner review remained Queued. Nine actual main API requests and
both Completed application Turns were independently verified again. Both Turns
survived exit; the owned container was removed.

The first verifier required API-view length to equal `messageCount`. It failed:

| Feedback step | Native `messageCount` | API-view messages | Outgoing messages |
| --- | --- | --- | --- |
| 0 | 42 | 13 | 19 |
| 1 | 45 | 15 | 22 |
| 2 | 48 | 17 | 25 |

That original source and failure are retained. A separate payload-presence check
then verified the exact complete frozen Unicode review text in both the native
view and independent outgoing input, once each, at all three steps. It requires
the same session/Turn/index, ordered post-step results, native stop reason and
nonnull usage. It explicitly reports `full_request_view: false` and
`production_consumption_contract_qualified: false`. This is not a repaired claim
that the full-view hypothesis passed. The normal transcript and generated API
view must not be described as the exact wire request.

The check passed inside the container and on retained host evidence. Four local
evidence mutations (wrong session, missing payload, missing response usage and
wrong step index) were rejected. These are verifier sensitivity checks, not four
additional native fault-injection runs.

| Item | SHA-256 |
| --- | --- |
| Outgoing requests | `9c9ed3590593e4ffd97ec8e37c47ba3d8940d1efdb0d46fbc2d6814daf3b2057` |
| Probe events including API views | `ad62c373ca9faee8c6ccedd9101685896042dd7d03c5797c7b47c67b42276434` |
| Executed updated probe | `e6061b29fb81ad50e7edea3f536248c11d8db62da779201f8bdee9706cb2ca15` |

Retained locally in `feedback-input-candidate` beside the prior run. Reproduce
the preceding feedback scenario with this updated probe, then run Codelaya's
`verify-feedback-input.mjs` instead of just `verify-feedback.mjs`. Keep all
baseline captures; retain `input-verification.json` and native probe observations.

### Reusable contract still needed

`prompt.submit` acknowledgement and this API-view candidate are separate native
observations. Store their exact original target/generation, operation, payload
digest and stable references separately in AIKit. A successful local enqueue
cannot synthesize either. Missing/truncated views, failed steps and uncertain
handoff must retain unknown evidence and never cause an automatic resend.

The target-generated declarations explicitly permit another `turn.step` hook to
answer without sending a request, and to change the result seen by hooks above
it. Therefore API-view plus a returned result/usage does not yet qualify a
protected post-send observation. This is a declared API limitation/risk, not a
claim that native short-circuit behavior was tested in this run. Qualify competing
hooks and actual send/response provenance before promoting consumption support.

Provider enhancement request: a stable native model-request reference, with an
engine-authored observation after actual dispatch/response, identifying the
session invocation, Turn and step and inclusion of the exact submitted message
bytes (or its canonical digest). It must remain distinguishable from plugin
short-circuit/rewritten results and survive reconciliation. AIKit should normalize
that into the existing-session message journal; Codelaya should only project it.
First investigate whether an existing native primitive supplies this contract;
do not add a second transport or managed-session launcher to work around it.

Durable submit/status/reconcile, busy/admission protection, protected native input
provenance, native Windows and other providers remain open. No capability or
production readiness was promoted by this candidate test.
