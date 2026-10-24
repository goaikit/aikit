# Claude request snapshot boundary

2026-10-24, ordinary interactive Linux Claude **2.1.308**. This follows the
earlier [later-handler comparison](../examples/claude-input-qualification/README.md)
with a predeclared reversal of diagnostic handler tiers. No SDK receipt,
capability or readiness is promoted. Windows remains deferred.

## Native result

Session `c7fff756-64ba-4c83-bf99-954a63f795de` ran the unchanged control prompt,
then the mutation prompt. The real appending handler ran at prepend tier, before
the user-tier snapshot observer. Both steps reached one matching settled
engine/core dispatch. Settings were unchanged and no hook failure was reported.

| Observation | Control | Earlier append before observer's pre-read |
| --- | --- | --- |
| Genuine main request | Yes | Yes |
| Late UTF-8 row in pre snapshot | No | **Yes** |
| Late row in post snapshot | No | Yes |
| Late row in that actual main HTTP body | No | **No** |
| Native append UUID retained after normal exit | N/A | Yes |

The appended native row UUID is `dfe60e7c-9d3f-41b6-ac34-08329edffdab`.
There were **two main requests and two separately classified title requests**.
The observer's pre-delegation read is therefore not necessarily a snapshot of
the already-frozen request, even with an unchanged step and genuine core trace.
This complements the earlier false inference from a post-dispatch API read.

The diagnostic row is not a production review operation. This does **not**
demonstrate a false receipt in the current SDK: its checkpoints remain candidates,
and the receipt stays Unknown. It rules out promoting arbitrary API-view/core
correlation to a universal exact-input proof. The same caution applies to rows
found through stored-history recovery.

## Existing API inspection and narrow enhancement candidate

The exact installed declarations have SHA-256
`3c29d70e7f113f2409db12c3db20dd8385296c46aaf26984d00e5ecacd37c2c0`.
`TurnStepInput` exports Turn/index/model/effort/messageCount/agentId, without the
request's transcript bytes or a retained input identifier. `ApiMessage` describes
normalized conversation state and omits per-send transformations. The existing
`session.messages` argument types offer main/subagent and API-view selection,
not a Turn/step-bound lookup. A `TraceEntry` reports the received step metadata
and returned response, not the missing request bytes. `session.authorize` is an
opaque credential handle for HTTP calls, not native invocation attestation;
this investigation does not call it or forward credentials.

These are inspected limits of the **tested exported surfaces**, not proof that
every Claude API or a later build lacks a solution. Existing inbox messaging,
append and history APIs remain useful and must not be described as absent.
Official references: [mods API/reference](https://code.claude.com/docs/en/plugins/mods/reference)
and [session inbox](https://code.claude.com/docs/en/cross-session-messaging#the-sessions-inbox-socket).

Prefer an upstream fix/extension of the existing `session.messages` read during
`turn.step`: return the immutable normalized input of **that exact native
session/invocation, Turn and step**, rather than later conversation state. If
live conversation reads must keep their current meaning, an explicit step-bound
selection on the same method could distinguish the two. This is a proposal,
**not an implemented Claude API**. It must define per-send transformations,
compaction, clipping, media, cancellation and model fallback, plus an independent
retained event/lookup that can resolve the same input after interruption.

Qualification should repeat both handler orderings and prove the returned
request-bound evidence agrees with real native main requests; a late append must
not enter an already-frozen input witness. Keep queue acknowledgement separate
from consumption and keep native invocation/effective-policy admission separate
from either. Do not add a new AIKit transport, proxy, daemon or message state
machine to disguise this missing fact. No issue has been sent upstream yet.

## Reproduction and retained failures

Use [the predeclared protocol](../examples/claude-input-qualification/early-protocol.md)
and `prepare-early.mjs` with immutable image
`sha256:9312badacd0cdf5496308a7292a5dc9965d25c616dbda94764ee10bbe8c30924`.
Native executable SHA-256:
`356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58`.
The accepted trial used a fresh disposable node profile, network none, no mounts,
public dummy key, loopback-only scripted endpoint and normal terminal trust.
No external model call, owner configuration or Windows provider check occurred.
Plugin validation passed with attribution/directory-submission warnings.

```sh
node /qualification/prepare-early.mjs
# Validate both plugins, start the bounded gateway, then use ordinary Claude.
# Submit AIKIT_INPUT_CONTROL and AIKIT_INPUT_MUTATION; exit normally; capture.
node /qualification/verify.mjs /home/node/evidence --early
node /qualification/verify.test.mjs /home/node/evidence --early
```

The accepted trial and all **10 altered-evidence checks pass** in Linux Node 22
and against the retained files in host Node 20. The updated verifier also accepts
the original later-handler trial and its 10 rejection checks. These are verifier
sensitivity checks, not 20 additional native fault trials. No Rust runtime changed;
the earlier 188-test Rust gate retains its own source identity.

The first container accidentally used the image's default Claude entrypoint
instead of a shell. It completed **one additional loopback main request** and
exited; fixture preparation refused the already-created profile. That failed
attempt is retained separately and excluded from the accepted control/mutation
pair. The fresh retry explicitly overrides the entrypoint. Direct host copying
then failed at a debug symlink; the retained tar excluding `latest` was extracted
into a separate directory. Those failed reads are not accepted evidence.

Private parent-workspace evidence: `.local/input-early-20261024`, accepted data
under `retained/evidence`, plus initial container/request records and manifest.
Accepted archive SHA-256:
`276c8d49cc62bcc66b6ee128bb81c0248113aa7848f35459f05c76ba421f7cd5`.
