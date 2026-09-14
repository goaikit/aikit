# A session brief is one completion over a scrubbed digest

## Status

accepted

## Context

Session capture (spec 010) turns the transcripts that Claude Code, Codex and
OpenCode write to disk into normalized `ToolEvent` rows, scrubbed of secrets
by the adapter before they leave it. Session sync (spec 012) uploads the
scrubbed transcripts and interprets nothing. The history backend (spec 008,
[ADR 0018](0018-history-has-a-transcript-vocabulary-distinct-from-the-streaming-vocabulary.md))
reads a transcript back as grouped messages and can set one free-form tag per
session.

Users now want a short account of each session: what was worked on, which
parts of the repository were touched, and tags from a list they control. Three
ways to get it were on the table:

1. **Send the transcript to a model.** Simple, and wrong on three counts. A
   transcript is unbounded, so the cost of a batch is unbounded. The history
   reader returns raw text that has not passed the adapter's scrubber, so a
   credential pasted into a prompt would reach a third party. And the same
   session summarized twice would send different bytes whenever the transcript
   grew by a line, so nothing could tell an unchanged session from a changed
   one.
2. **Run an agent over the session.** An agent harness injects a system
   prompt, tools and hidden behaviour, which is exactly the objection
   [ADR 0021](0021-a-judgment-is-one-native-completion-recorded-whole.md)
   raised for judges. A summary produced that way describes the session *and*
   the harness.
3. **Build a bounded digest from the event store and ask for one completion.**

Two smaller questions came with it. The tool events already say, without a
model, that a session modified only test files or only markdown, and a model
asked to decide that anyway will sometimes get it wrong. And the tag list is
the user's: a model that invents `refactoring` beside a configured `refactor`
has made the list useless for filtering.

## Decision

**The model sees a digest, never a transcript.** The digest is built from the
scrubbed event store: prompts, files touched in order with their action
kinds, commands run, the areas table, the final assistant message. Each
section has a character budget and the whole digest is capped at a few
thousand tokens. Anything that reaches the digest from the history reader is
scrubbed with the same `SecretScrubber` the adapters use, and the rendered
digest is scrubbed once more before it leaves the process. There is no flag
that sends the transcript.

**A brief is one native completion.** Exactly one `LlmGateway::complete` call
per session per attempt, with a system message and the digest as the user
message, temperature 0, no tools, no streaming, no agent loop. A rejected
reply is corrected once by appending the reply and a corrective message as
turns, the same shape as a judge retry.

**Mechanical tags are decided in code, before the call.** A tag bound to a
rule (`only_tests`, `only_docs`, `only_config`, `read_only`) is assigned from
the events, named in the digest as already assigned, and never left to the
model. The model adds tags only from the configured list; each model tag
carries the model's one-line justification. A name outside the list is
rejected, asked about once more, and then dropped and recorded on the brief as
rejected rather than silently kept or silently lost.

**The digest hash is the identity of the request.** It is the SHA-256 of the
exact user message sent, which embeds the tag list and the mechanical tags.
A brief whose stored hash matches is not regenerated without `--force`. Two
briefs with the same hash were produced from the same bytes.

**Briefs live in the capture store, additively.** `SessionBrief` is a new
record beside `SessionSummary` in `aikit-session-capture`, persisted through
three new `EventStore` methods with default implementations, so an existing
store keeps compiling. The SQLite table is created by the existing
`IF NOT EXISTS` migration. The record follows [ADR 0020](0020-eval-artifacts-are-an-additive-only-contract.md):
fields are added with `#[serde(default)]`, never renamed or removed. The
primary tag is mirrored into the backend's own tag slot where a
`HistoryMutator` exists, so the tool's UI shows it; the brief remains the
record of truth.

## Consequences

- A summary's quality is bounded by the digest. A session whose interesting
  content is in assistant prose gets a thinner brief unless
  `--include-assistant` adds the assistant text blocks, which are scrubbed
  like everything else. That is the intended trade.
- Claude Code prompts reach the digest only through the history reader, which
  resolves the default Claude home. A Claude session under an overridden
  `--path` is summarized from its files, commands and final message alone.
  The follow-up is for the Claude adapter to emit prompts as events, as the
  Codex adapter already does; nothing in this decision changes when it does.
- The word *summary* was taken: `SessionSummary` is the captured-session row
  `sessions_for` returns. The generated record is a *brief*, and the CLI verb
  stays `summarize` because that is what a user asks for.
- `aikit session list` now lists sessions on disk; the live listing moves
  behind `--live`. Every other `session` verb and every history and capture
  route already meant on-disk sessions, and the flag keeps the old behaviour
  reachable.
- Generation stays in the CLI. `aikit serve` exposes briefs read-only; a
  server-side job that generates them needs model configuration on the
  server and is a separate decision.
