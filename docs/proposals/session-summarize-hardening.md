# Proposal: hardening `aikit session summarize` for end users

**Status**: proposed, not implemented.
**Builds on**: [session-summarize.md](session-summarize.md), [ADR 0023](../adr/0023-a-session-brief-is-one-completion-over-a-scrubbed-digest.md), [ADR 0020](../adr/0020-eval-artifacts-are-an-additive-only-contract.md).

## Why

The first real runs worked end to end, and they also showed where an end user
gets hurt. Each item below was observed on real sessions and a real gateway
(`tools-advanced`, a qwen 30B thinking model), not inferred.

| Observation | Evidence | User impact |
| --- | --- | --- |
| A thinking model used the whole old budget on reasoning | 1,422 of 1,621 completion tokens were reasoning; first answer token at 23 s | "empty reply" failures (fixed in #178) |
| The gateway serves one request at a time | 4 parallel requests finished at 25, 48, 72, 95 s | `--parallel 4` adds no speed; at 5 or more, requests hit the 120 s timeout |
| No retry on transient errors | engine has none; the evals judge retries 3 times with backoff | one 429, 5xx or dropped connection fails the session |
| A wrong key fails every session one by one | 401 surfaced per session | a 50-session batch prints 50 identical failures |
| The character cap drops the most useful sections | on one session, areas and final message were cut, mid-line | brief loses how the session ended |
| Long sessions are described by their beginning | first 12 prompts, first 40 commands; 150 of 190 commands omitted | the second half of a long session is invisible |
| Edits made through the shell are not file touches | this session: 140 commands, 1 `Write`; summary said "only a proposal was written" | wrong or thin summaries for shell-driven agents |
| Claude prompts only come from the default home | history reader resolves `~/.claude` only; its errors are dropped | no prompts under `--path`, with no warning |
| Reading a stored brief needs model flags | `summarize` requires `--model` unless `--dry-run` | cannot just look at yesterday's briefs |
| Silent while the model runs | only a "scanned N files" line | a 20-session batch looks hung for minutes |
| Tag mirroring writes into the tool's transcript by default | `--no-mirror` is opt-out | surprising side effect on user files |
| CLI and `aikit serve` share one SQLite file | WAL on, no `busy_timeout` | occasional `database is locked` when both run |

## Proposal

Three phases, each shippable on its own. Every serialized change stays
additive under ADR 0020.

### Phase 1: calls that do not fail for avoidable reasons

1. **Transport retries.** Retry 429, 5xx, timeouts and connection errors up to
   3 times with exponential backoff and jitter, honouring `Retry-After`. Reuse
   the judge's policy rather than a second implementation. Reply problems
   (unparseable, unknown tags) keep their own single corrective retry.
2. **Preflight.** Before a batch, send one minimal completion to the chosen
   endpoint and model. Auth, unknown model and unreachable endpoint then fail
   once, with exit code 2, before any session is touched. `--no-preflight`
   skips it.
3. **Adaptive concurrency.** Keep `--parallel` as the ceiling, but start at 1
   and raise it only while latency stays flat; halve it on a timeout or 429.
   A one-at-a-time gateway then settles at 1 on its own, and a real cloud
   endpoint still scales. The request timeout starts when the request is
   sent, not when it was queued locally.
4. **Safer defaults.** See the table below.
5. **Say what was degraded.** Add `warnings` to the brief (additive): prompts
   unavailable and why, digest truncated, sections sampled, reply retried.
   History-reader errors become one of these instead of disappearing.
6. **Graceful stop.** Ctrl-C finishes in-flight calls, keeps every brief
   already stored, prints the tally, and exits 130. A re-run resumes for free
   because unchanged sessions are skipped by digest hash.
7. **SQLite `busy_timeout` of 5 s** on every connection, so the CLI and
   `aikit serve` can write to the same file.

### Phase 2: briefs that describe the whole session

8. **Protected sections.** Reserve budget for the header, prompts, areas and
   final assistant message first; files and commands share what remains. Never
   cut inside a line.
9. **Sample across the session, not its start.** For prompts, files and
   commands keep the first few, the last few, and evenly spaced items between,
   with explicit gap markers such as `… 42 more between 10:05 and 11:40`.
10. **File touches from shell commands.** Recognise common write and read
    shapes in `Bash` targets (`cat >`, `tee`, `>` redirection, `sed -i`,
    `git mv`, `rm`; `cat`, `head`, `sed -n`, `rg`) and count them as touches
    marked `via: shell` in the areas table. A heuristic, labelled as one.
11. **Claude prompts as events.** The Claude adapter emits scrubbed prompt
    events, as the Codex adapter already does. Prompts then work under any
    `--path`, and the history reader becomes a fallback.
12. **Opt-in long-session mode.** `--long-session segments` splits a session
    at prompt boundaries, writes a short note per segment, then writes the
    brief from the notes. Costs one extra call per segment, so it is off by
    default and `--dry-run` shows the call count.

### Phase 3: fits into how people work

13. **Read without a model.** `aikit session brief <id>` and
    `aikit session briefs [--since] [--tag]` read stored briefs. No model
    flags, no network.
14. **Config file.** `~/.config/aikit/session-summarize.toml` holds model,
    base URL, key variable name, tags file, areas file and limits, with named
    `[profile.<name>]` tables selected by `--profile`. Precedence: flag, then
    environment, then file, then default. The long gateway command line
    becomes `aikit session summarize --since 1d --profile gateway`.
15. **Output formats.** `--format text|json|ndjson|markdown`, plus
    `--summary-only` for piping the paragraph alone. `ndjson` emits one line
    per session as it completes. JSON carries
    `"schema": "aikit.session-brief/1"`.
16. **Progress.** One stderr line per session as it finishes
    (`[3/20] a27cfd7d generated 25.1s docs,research`), a final tally, and
    `--quiet` to silence it. `--dry-run` adds estimated tokens and time.
17. **Mirroring becomes opt-in.** `--mirror` writes the primary tag into the
    tool's transcript; the default writes nothing outside aikit's own
    database. Needs your decision, see below.
18. **Read-only MCP tools.** Under `mcp-tools`: `list_captured_sessions` and
    `get_session_brief`. Generation stays in the CLI, where the model is
    configured.

## Defaults

| Setting | Today | Proposed | Why |
| --- | --- | --- | --- |
| `--parallel` | 4, fixed | ceiling 4, adaptive from 1 | serial gateways time out at 5 or more today |
| `--timeout` | 120 s from local dispatch | 180 s from send | one thinking reply took 25 s; leaves headroom for slow cloud replies |
| Transport retries | 0 | 3, backoff 1 s base with jitter | matches the judge |
| `--max-tokens` | 4,096 (#178) | 4,096 | 1,621 used on a 3,000-token prompt |
| Digest cap | 12,000 chars, cut anywhere | 12,000 chars, protected sections, whole lines | keeps areas and final message |
| Commands kept | first 40 | 40 sampled head, middle, tail | covers long sessions |
| Mirror tag | on | off | no surprise writes to user files |
| SQLite busy timeout | none | 5 s | CLI and serve share the file |

## Tests that would guard this

- A fault-injecting mock gateway: 429 with `Retry-After`, 503, timeout,
  dropped connection, `finish_reason=length`, 401 at preflight.
- A serial fake backend proving adaptive concurrency settles at 1 without a
  timeout, and a parallel one proving it climbs.
- A synthetic 10,000-event session asserting protected sections survive and
  tail items appear.
- A real-shaped Claude fixture, every assistant record carrying a message id
  and usage, so the adapter regression fixed in #178 cannot return.
- A concurrent write test with two connections on one file.

## Out of scope

- Generating briefs from `aikit serve` (needs server-side model config and a
  job model).
- Any cost estimate stored on a brief (ADR 0020: never an estimate).
- Cursor and Gemini adapters.

## Decisions needed before implementation

1. **Mirroring default.** Switch to opt-in, a visible behaviour change for
   anyone already relying on the tag in Claude Code?
2. **Long-session mode.** Worth building now, or wait until sampling (item 9)
   proves insufficient on real sessions?
3. **Shell-command heuristics.** Acceptable to show inferred touches in areas,
   labelled `via: shell`, or keep areas strictly to tool events?
4. **Config file location.** A dedicated `session-summarize.toml`, or a
   `[session.summarize]` section in an existing aikit config?
