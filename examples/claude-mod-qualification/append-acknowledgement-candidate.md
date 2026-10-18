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

## Interactive probe: implemented, not qualified

`hooks/append-native-probe.js` serializes evidence-file writes so overlapping
callbacks cannot overwrite a newer event list with an older one. It is an
explicitly selected diagnostic module; the
default `hooks/hooks.json` does not load it. It registers one diagnostic slash
command and tool, writes evidence under `/home/node/evidence`, and schedules one
`$.prompt.submit` with fixed synthetic Unicode/newline content. It does not retry
an uncertain submission or modify an SDK/Codelaya receipt. Use only inside a
disposable qualification container with a scripted loopback model endpoint.

`verify-append.mjs EVIDENCE_DIRECTORY` is a candidate independent verifier of the
native append return, transcript UUID/content, session correlation and actual
scripted endpoint input. It has not passed against an interactive capture yet;
its assertions are hypotheses to check, not evidence of native guarantees.

The attempted ordinary interactive launch exited before onboarding with
`EAI_AGAIN` connecting to Anthropic services. No slash command or review prompt
was submitted. Automatic approval review rejected temporarily attaching Docker's
bridge network because repository content was mounted/copied into the container.
Inspection confirmed that the container retained only the `none` network. No
owner configuration, Windows policy helper, profile or credential was changed.

Next qualification requires an approved isolated onboarding arrangement, then
actual stored-row and request evidence. Resolve differences against retained
raw evidence; do not loosen the verifier merely to obtain a passing result.
Native Windows validation separately still requires the owner's policy-service
sign-in. Linux success will not satisfy that gate.

Production acknowledgement and consumption remain unqualified. This candidate
changes no public SDK capability, readiness decision or delivery state.
