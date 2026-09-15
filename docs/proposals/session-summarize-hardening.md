# Proposal: launch plan for `aikit session summarize`

**Status**: proposed. Supersedes the earlier hardening draft on this branch.
**Builds on**: [session-summarize.md](session-summarize.md), [ADR 0023](../adr/0023-a-session-brief-is-one-completion-over-a-scrubbed-digest.md), [ADR 0020](../adr/0020-eval-artifacts-are-an-additive-only-contract.md).

## Goal

Ship session briefs publicly tonight with one guarantee users can rely on:
**aikit only reads the coding tools' session files; it never writes to them.**
Everything else is split into what must land before the announcement and
what follows it.

## Where we are

- `v0.1.196` is already released and contains the feature (#177) and the
  Claude adapter fix (#178).
- That release **writes a tag line into Claude Code session files by
  default** after each Claude brief. This contradicts the guarantee above and
  is the one launch blocker.
- `main` CI is green on its second run; the failed run was a flaky macOS test
  in the SDK Pi runner (`test_run_pi_teardown_not_blocked_by_sigterm_ignoring_grandchild`),
  unrelated to sessions. Auto Release fails at "Commit version bump" because
  `main` is protected; that is pre-existing and the Release job still
  publishes.
- Measured on real sessions and a real gateway (LiteLLM 1.88.1 in front of
  Ollama, `tools-advanced`): about 25 s per brief, requests served one at a
  time even though the key admits 4, no batch support (`files_settings` not
  configured).

## Decisions taken

| Question | Decision |
| --- | --- |
| Writing into tool session files | Never. Mirroring is removed, not made optional. |
| Long sessions | One call when the session fits the digest; split into pieces only when it would be squeezed. |
| File edits made through the shell | Shown in areas, labelled `via: shell`. |
| Settings | A `[session.summarize]` section in the aikit config, global and project merged, project wins. |

## Tonight: must land before the announcement

Each item is small, independent, and covered by a test.

1. **Remove mirroring.** Delete `TagMirror`, `HistoryTagMirror`, the
   `--no-mirror` flag, the `mirrored` output field, and every doc mention.
   Amend ADR 0023 with a dated note that briefs are never written back.
   *Test:* summarize a fixture session and assert the source directory's file
   list, sizes and modification times are unchanged.
2. **Keep the end of the story.** The digest cap reserves room for the header,
   prompts, areas table and final assistant message before files and
   commands, and never cuts inside a line. Today a real session lost its areas
   table and final message. *Test:* a 10,000-event session keeps all four
   sections.
3. **Retry what is transient.** Retry 429, 5xx, timeouts and dropped
   connections three times with backoff and jitter, honouring `Retry-After`,
   using the evals judge's existing policy. *Test:* fault-injecting mock
   gateway.
4. **Fail once on bad configuration.** Before a batch, one tiny call checks
   endpoint, model and key; a 401 or unknown model exits 2 before any session
   is attempted. *Test:* mock 401 yields exit 2 and zero session outcomes.
5. **Defaults that do not time out on common gateways.** `--parallel` default
   1 (a serial backend queues parallel calls and the fourth waited 95 s),
   `--timeout` 180 s. Users on cloud endpoints raise `--parallel`.
6. **Progress.** One stderr line per finished session:
   `[3/20] a27cfd7d generated 25.1s docs,research`, then a tally.
7. **Say what was degraded.** An additive `warnings` list on the brief:
   prompts unavailable and why, digest truncated, reply retried. History
   reader errors stop disappearing.
8. **Shared database.** SQLite `busy_timeout` of 5 s so the CLI and
   `aikit serve` can write the same file.
9. **Read without a model.** `aikit session briefs [--session] [--since]
   [--format]` prints stored briefs with no model flags and no network.
10. **Docs and release note.** README and command reference state the
    read-only guarantee, the known limits below, and the thinking-model
    budget. The release note tells `v0.1.196` users that tag lines may have
    been appended to Claude session files and that the new version never
    does so.

### Known limits to publish with the launch

- A session edited mostly through shell commands has thin areas until the
  `via: shell` heuristic lands.
- Very long sessions are represented by the start of each section until
  sampling and splitting land.
- Claude Code prompts are read only from the default Claude home.
- Speed is the gateway's: about 25 s per brief on a one-at-a-time thinking
  model.

## Go / no-go checklist

- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean.
- [ ] `cargo test --workspace --all-features --all-targets` green on Linux; macOS rerun if the Pi flake recurs.
- [ ] Smoke run against the real gateway on three real sessions: `list`, `summarize --dry-run`, `summarize`, `briefs`.
- [ ] Read-only check on that smoke run: no tag lines in `~/.claude/projects`, no mtime changes.
- [ ] Version bumped by hand (Auto Release cannot push to protected `main`), release note written.

## After launch

In order of user value:

1. **Sampling across the session**: first, middle and last items with gap markers, instead of the first N.
2. **Split only when it does not fit**: pieces summarized with adaptive parallelism, then combined; one call for sessions that fit.
3. **Shell edits in areas**, labelled `via: shell`.
4. **Adaptive concurrency**: start at 1, climb while latency stays flat, halve on timeout or 429.
5. **`[session.summarize]` config section** with named profiles, merged global then project.
6. **Claude prompt events** from the adapter, so prompts work under any `--path`.
7. **Output formats** `ndjson`, `markdown`, `--summary-only`; versioned JSON schema.
8. **Graceful Ctrl-C** and **read-only MCP tools** (`list_captured_sessions`, `get_session_brief`).

## Out of scope

Generating briefs from `aikit serve`, cost estimates on briefs (ADR 0020),
and Cursor or Gemini adapters.
