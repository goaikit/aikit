# Claude 2.1.308 native model-dispatch provenance

This is a credential-free Linux API experiment for AIKit's existing-session
review consumption contract, not a new transport or production proof adapter.
The predeclared [protocol](protocol.md) and exact source are retained here.
Windows checks remain deferred.

## Observed result, 2026-10-24

One ordinary interactive native session:
`782c52ab-b99d-4415-83f0-ae0d1a4d8eb3`.

| Observation | Genuine control | Shortcut handler |
| --- | --- | --- |
| Native API view contains its prompt | Yes | Yes |
| Returned text, stop reason and nonnull usage | Yes | Yes, synthesized by the handler |
| Matching engine/core `turn.step` entry | Yes | No |
| Actual main-loop model request | One | Zero |
| Other native calls contain shortcut text | N/A | Four session-title requests |

The exact core input contained Turn ID, index, model, effort and message count;
its result contained answer, tool uses, stop reason and usage. It did not carry
the exact outgoing messages, immutable review/input digest or a durable native
operation receipt. Both Turns ended natively as successful answers. This is no
claim of false Codelaya completion: Codelaya was not installed in this API probe.

**The original no-HTTP-send hypothesis failed.** The first verifier correctly
rejected the capture because the marker appeared in four native title requests.
The bounded gateway had refused these unexpected requests rather than proxying
them; their retry inputs are retained. They have no tools and explicitly ask for
a session title, unlike the main-loop request with its actual `Read` tool set.

A separately labeled analysis of the same immutable capture verifies that the
shortcut made no main-loop model request, while returning convincing synthetic
text/usage. It preserves `original_no_http_send_hypothesis: false`; it does not
erase/relabel the original failed premise. Native core traces can distinguish
this main-loop omission. An unscoped captured HTTP prompt, API-view presence or
returned usage alone cannot prove consumption by the intended Turn.

Twelve altered-evidence checks pass, including invented/missing core execution,
wrong Turn/session, missing API-view marker, actual main-input leakage, an
unclassified background request, settings/tier changes and foreign transcript.
These are sensitivity checks, not twelve additional native sessions. No native
contract or production readiness is promoted.

## Isolation and identities

- Linux x86-64 Claude `2.1.308 (Claude Code)`.
- Native executable SHA-256:
  `356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58`.
- Image: `sha256:9312badacd0cdf5496308a7292a5dc9965d25c616dbda94764ee10bbe8c30924`.
- Network `none`, no host mounts, no owner credentials/configuration. Public dummy
  key, loopback scripted endpoint, six processed HTTP requests maximum and 4 MiB
  body limit. Six requests were retained: one control title, one actual main
  request, four shortcut-title attempts. No external paid model call.
- Disposable profile explicitly seeds offline onboarding; normal workspace trust
  is accepted in the terminal. Actual debug confirms observer at prepend tier,
  shortcut at user tier. Both plugins pass native validation. This is a controlled
  user-profile ordering experiment, not protected mandatory handler installation.
- Source hashes, raw trace/API views, native transcript/debug, before/after
  settings, requests, original verifier failure and separate analysis are retained
  locally in `.local/native-dispatch-trace-20261024/evidence.tar`.
- Archive SHA-256:
  `22e4494144f023c83a5807aca3141fd97c93cbbe3bb43d80a5e7c5bc955d211e`.
- Request file SHA-256:
  `c6d1653f5d0a4b69003e69b20f30e64c499960469b5577f0bd53866db2907a82`.
- Native terminal exited normally; the task container was stopped after capture.

## Reproduce

Use the pinned image from the existing `claude-mod-qualification/Dockerfile.native`
with the explicit 2.1.308 version/hash. In a fresh network-isolated container copy
this directory to `/qualification`, make it and `/home/node` readable/writable by
the unprivileged `node` user, then run:

```sh
node /qualification/prepare.mjs
git -C /home/node/workspace init -q
claude --version > /home/node/evidence/native-version.txt
claude plugin validate /home/node/workspace/dispatch-observer
claude plugin validate /home/node/workspace/dispatch-shortcut
node /qualification/gateway.mjs
```

Start a separate ordinary terminal in `/home/node/workspace` with the loopback
`ANTHROPIC_BASE_URL`/public fixture key and existing nonessential-traffic flags.
Run `claude --debug-file /home/node/evidence/native-debug.log`, accept trust, send
`AIKIT_DISPATCH_CONTROL`, wait, send `AIKIT_DISPATCH_WITHOUT_ENGINE`, wait and
exit with `/exit`. Do not change a running trial's handlers/settings.

```sh
node /qualification/capture.mjs
# Original broad hypothesis intentionally fails on this retained capture:
node /qualification/verify.mjs /home/node/evidence
# Separate analysis reports the failed premise and classifies main/title input:
node /qualification/verify.mjs /home/node/evidence --main-loop-analysis
node /qualification/verify.test.mjs /home/node/evidence
```

The native title behavior may vary on a fresh run; preserve a changed/failed
premise rather than relaxing the verifier. The current analysis requires and
reports those title requests; it is not a generic all-provider request classifier.

## Consumer consequence

The SDK capability report now explains this exact Linux/x86-64/interactive
2.1.308 evidence under `MessageReconciliation`, retaining `Unsupported` and
`implemented=false` for the unfinished qualified reconciliation contract.
Existing candidate append/handoff implementations are not being removed.
Nine Linux Rust capability tests pass, including an eight-context scope matrix
and failed version probe; other versions/platforms/modes do not inherit the result.

The next required native primitive remains a generation-scoped, durable operation
receipt and lookup plus an engine observation binding the exact review payload
to an actual target main-loop input. Existing core traces are useful provenance
and must be reused; they do not establish that full contract by themselves.
