# Claude main-input snapshot comparison

Follow-up: [earlier-handler protocol](early-protocol.md) and
[native result / request-bound API proposal](../../aikit-sdk/claude-request-snapshot-boundary.md).
Reversing handler order makes the pre snapshot include the late row even though
the same real main request excludes it. The earlier result below remains scoped
to its own ordering; do not generalize pre-read/core correlation into proof.

To reproduce the follow-up, explicitly override the image entrypoint to a shell,
run `prepare-early.mjs` in a fresh profile, then use the same two prompts. After
normal exit/capture, pass `--early` to both verifier scripts. The updated verifier
also retains the original default mode.

## Observed result, 2026-10-24

One fresh ordinary interactive Linux Claude 2.1.308 session:
`b5642e58-728d-439e-8028-06f7694e8e66`.

The [predeclared protocol](protocol.md) compares a control and a later-handler
conversation append, with real main dispatch in both cases.

| Observation | Control | Append during `turn.step`, before delegating |
| --- | --- | --- |
| Matching settled engine/core dispatch | Yes | Yes |
| Late UTF-8 row in pre-dispatch API snapshot | No | No |
| Late row genuinely stored and retained in transcript | N/A | Yes, with native UUID |
| Late row in post-dispatch API snapshot | No | Yes |
| Late row in that actual main HTTP request | No | **No** |

Claude had already frozen that main request before the later handler appended
the row. Therefore a post-dispatch API snapshot plus a genuine core trace could
misattribute this row to the completed request. The pre-dispatch snapshot agrees
with this trial's request concerning the tested row; it remains an implementation
and qualification candidate, not a demonstrated provider blocker.

The core `messageCount` remained 17 on the mutation step, while normalized API
message counts changed from three to four. The control's core count was 12 with
one normalized pre-dispatch message. Do not equate these different counts or
infer exact input from their equality.

There were two main requests and two separately classified native title requests.
Both plugins loaded in the intended prepend/user order. The session exited
normally; settings were unchanged. Eight altered-evidence checks pass. They are
not eight native trials. The first verifier did not handle HTTP message content
serialized as a string; its error is retained. Supporting that valid form fixed
the verifier without changing the submitted trial or its conclusions.

## Reproduction

Use the existing `aikit-claude-native-probe:2.1.308` image with network `none`,
no mounts and only the public dummy fixture key. Copy this folder to
`/qualification`, run `node /qualification/prepare.mjs`, validate the two plugins
under `/home/node/workspace`, and start `gateway.mjs` on container loopback.
Run ordinary `claude --debug-file /home/node/evidence/native-debug.log` in that
workspace, accept its normal workspace trust prompt, then submit the two protocol
prompts and exit. Run `capture.mjs`, then:

```sh
node /qualification/verify.mjs /home/node/evidence
node /qualification/verify.test.mjs /home/node/evidence
```

The disposable profile seeds offline onboarding explicitly. It is not an owner
profile. Plugin validation passed with attribution/directory-listing warnings;
this is not a plugin-directory submission. The endpoint accepts at most twelve
requests of 4 MiB and never proxies. No real credential, external model call,
managed agent session or Windows provider check is used.

## Identities and limits

- Native image: `sha256:9312badacd0cdf5496308a7292a5dc9965d25c616dbda94764ee10bbe8c30924`.
- Executable SHA-256: `356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58`.
- Evidence/source archive SHA-256: `8c94bf0023ce71b6661b5d1797ee34e0a60aaa9ce0d660ecd33e61a7fd1c358f`.
- Local evidence, hashes, original verifier error and final checks:
  `.local/aikit-input-20261024/`. A direct evidence-folder copy hit a native
  `latest` symlink; the complete regular-file archive excludes that link and
  retains the actual debug log, transcript and executed diagnostic source.

This probe uses existing `session.messages`, `session.append` and core trace
APIs, described in the installed version's generated declarations and the
[official mods reference](https://code.claude.com/docs/en/plugins/mods/reference).
The official [inbox documentation](https://code.claude.com/docs/en/cross-session-messaging#the-sessions-inbox-socket)
also provides an existing-session delivery path; transport is not absent.

Next, qualify pre-dispatch snapshot/core correlation with actual frozen review
framing, compaction, size bounds, competing handlers and interrupted dispatch,
and retain resolvable evidence through the existing SDK journal. Actual-session
admission and independent acknowledgement/lookup remain separate requirements.
These are unfinished integration investigations; this trial does not establish
that Claude cannot supply them. No receipt, capability or production activation
was promoted. Windows remains deferred.
