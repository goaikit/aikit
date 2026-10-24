# Native append competitors, 2026-10-21

Two ordinary interactive Linux sessions exercised native `session.append` with
one explicitly loaded competing plugin. Claude Code 2.1.296, executable and image
identities are unchanged from the [baseline append capture](append-acknowledgement-candidate.md).
Sources before this work: AIKit `b0436c3` and Codelaya `2bf6fc0`. This step changes
diagnostic fixtures/verifiers only, not production receipt state or capabilities.

## Isolation and failed invocation

A new public-image container completed normal onboarding with the public dummy
key and no mounts, owner profile or credentials. External networking was removed
before copying the probes or trusting the disposable workspace. Every subsequent
session used only the `none` network and a deterministic HTTP endpoint on
container loopback. Each valid scenario used a fresh ordinary native session and
fresh endpoint request journal, then exited normally before transcript retention.

The first launch mistakenly supplied `--plugin-dir A B`. This release accepts
one path per flag: B became an initial prompt and the competitor was not loaded.
That run is retained as `loading-negative` and is not competing-handler evidence.
The CLI's own `--help` confirmed the corrected repeatable spelling
`--plugin-dir A --plugin-dir B`. Both later captures confirm the competitor
actually ran through its separate observation file.

## Rewrite then return the original

The observer plugin registers the existing `/aikit-append-feedback` command.
`hooks/append-competing-probe.js`, loaded as the second plugin, replaces only the
exact synthetic payload in the provider envelope, calls `next(changed)`, records
the downstream result, then tries to return the original message/UUID.

Observed result, independently checked against the persisted transcript and
actual endpoint input:

- Native append UUID and origin stayed stable.
- The changed message was stored, including after normal session exit, and sent
  once in one main request. The original payload was absent from that request.
- Claude rejected the competing hook's false return, reporting a row other than
  what `next` answered. The observer received the actual changed stored message.
- `prompt.submit` still returned the original text. `turn.start.text` also held
  the original, pre-rewrite envelope even though the outgoing message changed.
- The original-payload envelope verifier correctly failed; no original-payload
  acknowledgement or consumption was recorded. A separate verifier
  of this specific rewrite/false-return scenario passed in Linux and on retained
  evidence. A scripted answer with correlated Turn/usage is not real-model proof.

This falsifies using successful submit return or Turn-start text as actual input
proof. The observed append result exposes this rewrite to a future adapter; it
does not stop another plugin from rewriting the message. A future adapter must
compare the frozen logical payload against qualified native framing and refuse
to acknowledge a mismatch. Do not silently accept the altered review or retry it.

## Fabricated receipt without next

In a separate session, `hooks/append-fabricated-probe.js` returned a fixed false
UUID without calling `next`. Claude reported the invalid unchained answer and
continued native storage. One real row was kept with the original complete
envelope; its native UUID matched the observer's acknowledgement and the saved
transcript. The fabricated UUID had no transcript row. One main request carried
the original payload once.

`verify-append-fabricated.mjs` reuses the independent full baseline envelope
verifier and additionally checks the competitor's executed no-next attempt,
session/input equality, fabricated/real UUID distinction and absence of the false
row. It passed in Linux and on retained evidence.

These are two user-tier plugin cases in the observed load order, not a proof of
all tiers/orderings, exceptions/timeouts, denied append APIs, native caller
authentication, generation admission, interrupted handoff/reconciliation or
protected model consumption. Windows remains deferred, and all production
readiness/capability assessments remain unchanged.

## Source and evidence identities

| Executed source | SHA-256 |
| --- | --- |
| Rewrite competitor | `78bd2865f5e393b4b8289d2f23b587a5d53438c8a59a09c31ac96d9cd7b74468` |
| Rewrite verifier | `4784bc3ed9f30378ece82eab61cbcbff10fd885e9a571f3b763d8413fd17f33c` |
| Fabricated competitor | `b595e6a82bb541d8324b714ae749394230ee0a688dc0e97a0fd58c4c98a71896` |
| Fabricated verifier | `f16eaf4a992ca7fa1e8f64105980afa7aa4da55472c56d5c779f5ede80bc81cb` |

Baseline observer, gateway and envelope verifier hashes remain those in the
baseline report. Retained raw capture root is the parent chat's
`.local/claude-append-competing-20261021`, with separate `final`, `fabricated` and
`loading-negative` directories. Verifier JSON includes every native capture hash.

| Evidence | Rewrite scenario SHA-256 | No-next scenario SHA-256 |
| --- | --- | --- |
| Observer events | `dd972a23b780ab08fdc6c9e697391bfeaf1a8dfa7c52b1b452b857f8f3c22736` | `560e8ce5bff329296045b4a167132adb77c0d936bc570552b84af5ad8d2f43e9` |
| Competitor events | `403c591c8cc718e1f837b7924d04144f1514255a887c403e84a9dd3c01207a40` | `7bbda842641d98a11893721d480349c86cff9ce1a6c5438910e4ee100678ac30` |
| Native transcript | `8427c9058797947124c3f82e18c1b59535dc50cffbe9efb5b7a94c7c261e15bb` | `c2a11afffd09320c885d90156ba8cadb40a0f200acead041463bea486bc43f94` |
| Outgoing requests | `1e8ac8dd0747690e660c75aba4899a2b13b19ae64bd9fae175b4e2f928e1a407` | `f8f593c62ca1e88f9f442aa6b505158fd01fccb3df19039e584fc286f65dbb85` |

## Reproduce

Use the empty-container onboarding, disconnection and copy sequence in the
baseline report. Select `append-native-probe.js` in the observer plugin copy.
Make a second plugin named `aikit-append-competitor` whose copied `hooks/hooks.json`
selects either `append-competing-probe.js` or `append-fabricated-probe.js`.
Start Claude with repeated `--plugin-dir` flags and only the local fixture key
and base URL. Submit the slash command once and exit normally. Copy that native
session's transcript to the evidence directory as `transcript.jsonl`.

```sh
# Rewrite scenario: unchanged baseline verifier MUST fail.
node /home/node/probe/verify-append-envelope.mjs /home/node/evidence
node /home/node/probe/verify-append-competing.mjs /home/node/evidence
# In a separate no-next scenario:
node /home/node/probe/verify-append-fabricated.mjs /home/node/evidence
```

Retain failures and raw source/network identities before removing the owned
container. Reset the owned loopback endpoint and evidence directory between
scenarios; never mix sessions or reuse the synthetic queue as production proof.
