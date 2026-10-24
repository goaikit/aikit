# Stored-message acknowledgement candidate

## Executed evidence, 2026-10-18

Claude Code 2.1.296 Linux x86_64 ran its native plugin test engine in a disposable
container with `--network none`, no owner profile and no credentials. Plugin
validation passed; fourteen tests passed, zero failed. Four existing event tests
remain distinct from the ten `tests/append.test.ts` checks:

1. The returned append UUID and message match the row held by mock storage.
2. A downstream content rewrite is visible in the returned stored message.
3. Returning a fabricated result without chaining cannot invent a stored receipt.
4. A wrapper cannot conceal a downstream content rewrite by returning the original.
5. Six independent competing handlers cannot replace UUID, door, origin, agent ID,
   message type or role. Each test changes only one field, so rejecting one field
   cannot mask acceptance of another.

An initial per-field run had thirteen passes and one test-registration error:
the diagnostic plugin name contained uppercase `agentId`. The corrected lowercase
plugin name ran the previously unexecuted agent-ID assertion successfully. The
final log is `.local/claude-native-append-fields-final.log`; the failed attempt is
retained separately as `.local/claude-native-append-fields.log`.

Image: `aikit-claude-native-probe:2.1.296`, digest
`sha256:fe24c3f19fe12410b4d57bb1101def27a05a92fa7083611623157a0a2b433e65`.
Executable SHA-256:
`24972e3bc859fab2b46ed4c1e51f7d6130f06d3bd550811a114640de3370d0de`.
The earlier eight-test log is `.local/claude-native-append-engine-first.log` in
the qualification workspace. Raw local logs are not published acceptance artifacts.

The target-generated declarations and the official
[event reference](https://code.claude.com/docs/en/plugins/mods/reference) suggest
`session.append` as a storage acknowledgement primitive. These tests exercise
mock storage inside the provider's test engine. They do not establish actual
transcript persistence, protected model consumption, caller identity, deadline
admission, competing-handler safety in an interactive session, or Windows support.

## Interactive probe: bounded Linux capture, 2026-10-21

The owner approved continuing Linux Docker qualification and deferring Windows
checks. A fresh container from the image above completed ordinary onboarding
with only the public dummy key, no repository mounts, owner profile or real
credentials. Before copying the diagnostic source or trusting its workspace,
the bridge network was disconnected and only Docker's `none` network attached.
One ordinary interactive session then ran `/aikit-append-feedback` against the
container-loopback scripted endpoint and exited normally. No Windows policy or
Claude configuration was changed.

The original `verify-append.mjs` **failed unchanged**: the stored user message is
not the original payload alone. Claude frames it with the plugin name and an
instruction explaining that a plugin submitted it between turns. The returned
`prompt.submit` text remains the unframed payload. Treating those two strings as
the same native content would produce incorrect acknowledgement validation.

A separate `verify-append-envelope.mjs` checks the observed version-specific
envelope exactly. It passed inside Linux Docker and against the retained capture:

- One submit attempt/return and one native append UUID.
- Matching plugin origin, original session and stored user row after exit.
- Exact complete envelope in the append event/result, transcript and Turn start.
- One main HTTP request containing that complete envelope as a text block and
  the exact original Unicode/newline payload once.
- Correlated successful Turn, nonzero scripted response usage and normal exit.

The initial envelope verifier also rejected the outgoing request because it
assumed the entire user-message content was the envelope. The wire message has
an additional system-reminder text block. That failed whole-message hypothesis
is retained separately; the final verifier matches the complete envelope block
and does not claim the full request equals the stored row.

Seven verifier checks passed on the retained capture: the unchanged capture
passes; foreign transcript session, forged acknowledgement UUID, changed stored
payload, duplicate wire payload, missing wire payload and aborted Turn fail.
These six mutations are verifier sensitivity checks, not six native fault runs.
An initial mutation changed an earlier queue row rather than the stored message;
that test failure is retained and the mutation now targets the actual user row.

Retained evidence: parent chat `.local/claude-append-20261021`, including
`envelope-verification.json`, both failed hypotheses and verifier logs. The
original verifier failure is `.local/claude-append-exact-20261021.log`.

| Evidence/source | SHA-256 |
| --- | --- |
| Append events | `07091cdc05e3388cb18fa7c0eace38949c6881f6efa504e756a077f49a9a714e` |
| Native transcript | `0d0dae57738eaa6d5e99b62beb6aac21e5905a2cd7e2e3b98cd613e3a4408986` |
| Outgoing requests | `119cdb51e6354cdbb45f4552b1fadc3d2ff3b7d57001e4486b25735659867046` |
| Executed opt-in probe | `3d5d7d67e28538483e58d396b5ac330492526b5055a1a70eca114e4637d723d6` |
| Executed loopback endpoint | `cb145e1a5c5615a0baba74b1c100424d331b3a88e20dce87f9284c423aadad31` |
| Original exact-message verifier | `139adf22332bf1998489150c3849d31ce32ae514bef66f56a7621f30cc8b3d06` |
| Separate envelope verifier | `076bf4996e6d596e9b71a21de69664d03655cb58c58f238c2b014befd3b51d87` |

Reproduce the [ordinary isolated startup](native-gateway-probe.md#reproduce),
using a **fresh empty container without mounts for onboarding**, disconnecting
external networking before copying the fixture, and selecting
`hooks/append-native-probe.js` in its copied `hooks/hooks.json`. Submit the slash
command once, exit, and copy that session's transcript to
`evidence/transcript.jsonl`. Then run:

```sh
node /home/node/probe/verify-append.mjs /home/node/evidence # expected exact-message failure
node /home/node/probe/verify-append-envelope.mjs /home/node/evidence
node /home/node/probe/tests/verify-append-envelope.test.mjs /home/node/evidence
```

### Consequences for the existing SDK contract

Later that day, [two real native append competitors](native-append-adversarial.md)
showed that the observed append result exposes a downstream rewrite even when
the competitor returns the original message, and that an unchained fabricated
UUID is rejected. Submit return and Turn-start text still held the original text
in the rewritten run and therefore cannot prove the actual stored/sent payload.
These are bounded Linux cases, not general production capability qualification.

Reuse `MessageRequest.content` / `MessageReceipt.payload_sha256` for the original
logical payload. The existing contract explicitly assigns native framing to the
adapter. A future qualified adapter must retain both original payload digest
and exact native envelope digest, native row UUID, plugin origin, provider
version and original session/generation in the evidence resolved by the existing
`MessageProof::Acknowledgement.receipt_reference`. This finding alone requires
no new public message state, duplicate delivery API or storage schema.

Do not strip arbitrary text, search for a payload substring and call it an
acknowledgement, or replace the frozen payload hash with the envelope hash.
Unknown framing/version, rewritten content or uncertain identity must remain
unqualified. Protect handoff, reconcile after interruption, and qualify competing
handlers before emitting production proof. Independent fixture request capture
does not create a protected production consumption signal or prove real-model
understanding. No SDK capability, Codelaya receipt or readiness is promoted.

## Probe implementation and prior attempt, 2026-10-18

`hooks/append-native-probe.js` serializes evidence-file writes so overlapping
callbacks cannot overwrite a newer event list with an older one. It is an
explicitly selected diagnostic module; the
default `hooks/hooks.json` does not load it. It registers one diagnostic slash
command and tool, writes evidence under `/home/node/evidence`, and schedules one
`$.prompt.submit` with fixed synthetic Unicode/newline content. It does not retry
an uncertain submission or modify an SDK/Codelaya receipt. Use only inside a
disposable qualification container with a scripted loopback model endpoint.

`verify-append.mjs EVIDENCE_DIRECTORY` was a candidate independent verifier of the
native append return, transcript UUID/content, session correlation and actual
scripted endpoint input. Its whole-message hypothesis failed in the later
capture above and remains unchanged as a retained negative check.

The attempted ordinary interactive launch exited before onboarding with
`EAI_AGAIN` connecting to Anthropic services. No slash command or review prompt
was submitted. Automatic approval review rejected temporarily attaching Docker's
bridge network because repository content was mounted/copied into the container.
Inspection confirmed that the container retained only the `none` network. No
owner configuration, Windows policy helper, profile or credential was changed.

The later approved empty-container arrangement resolved this onboarding obstacle
for Linux only. Native Windows validation is deferred by the owner as of
2026-10-21; Linux success will not satisfy that gate.

Production acknowledgement and consumption remain unqualified. This candidate
changes no public SDK capability, readiness decision or delivery state.
