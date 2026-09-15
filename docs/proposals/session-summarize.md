# Proposal: session listing, summaries and tags

**Status**: accepted as the plan for the implementation in this branch.
**Related**: spec 010 (session capture), spec 012 (session sync), spec 008
(history backend), [ADR 0018](../adr/0018-history-has-a-transcript-vocabulary-distinct-from-the-streaming-vocabulary.md),
[ADR 0020](../adr/0020-eval-artifacts-are-an-additive-only-contract.md),
[ADR 0021](../adr/0021-a-judgment-is-one-native-completion-recorded-whole.md),
[ADR 0023](../adr/0023-a-session-brief-is-one-completion-over-a-scrubbed-digest.md).

## Goal

List the coding-agent sessions on disk, pick some or all, and get for each a
short summary of what was worked on, the areas of the codebase touched, and
tags from a fixed list. The result is persisted so re-running is cheap and the
primary tag shows up where the tool itself keeps tags.

## Vocabulary

| Term | Meaning |
| --- | --- |
| **Captured session** | One session an adapter has parsed into the event store: the `SessionSummary` row `EventStore::sessions_for` returns (spec 010). That name is taken, so the generated record is not called a summary. |
| **Digest** | The bounded, deterministic text built from a captured session's events that is the only thing a model sees. Never the transcript. |
| **Area** | A directory of the repository (or a user-named group of prefixes) that a session touched, with read and modified counts and time attributed from event timestamps. |
| **Mechanical tag** | A tag decided in code from the events, before any model call (`test` when only test files changed, and so on). |
| **Session brief** | The persisted result: summary paragraph, areas, tags, model, digest hash, generated-at time. One per (tool, session id). |

## Crate ownership

| Crate | Owns | Why |
| --- | --- | --- |
| `aikit-session-capture` (existing) | The `ingest` module (`parse_and_store_file`, `scan_adapter`) moved out of `aikit serve` so the CLI, serve and the summarizer share one idempotent scan path. Nothing about briefs. | The crate stays about parsed events; it neither stores nor produces briefs. |
| `aikit-session-summarize` (new) | `SessionBrief`, `AreaTouch`, `TagAssignment` and the `BriefStore` trait (`put_brief`, `brief_for`, `briefs_for`) with an `InMemoryBriefStore` for tests; locations, digest builder, area grouping, tag list + mechanical rules + validation, prompt rendering and reply parsing, the batch engine with bounded concurrency. Depends on `aikit-session-capture` and `aikit-agent` (for `LlmGateway`, `OpenAiCompatProvider`, `MockGateway`). | Mirrors `aikit-session-sync`: a sibling consumer of capture with its own responsibility, and the owner of its own record. It never spawns a tool and never reads a transcript. |
| `aikit-cli` (root) | `aikit session list` / `aikit session summarize`; the SQLite `capture_session_briefs` table, with `SqliteEventStore` implementing `BriefStore` beside `EventStore` on one connection; the history-reader prompt source and history-mutator tag mirror (the only place `aikit-sdk` is wired in); two read-only serve routes. | The root already owns the SQLite store and the serve surface. |

## CLI

`aikit session` today holds `new` and `list` (live sessions on a running
`aikit serve`) and `sync` (on-disk transcripts). The conflict is resolved by
making **`session list` mean sessions on disk**, which is what the word means
everywhere else in aikit (history, capture, sync). The live listing stays
reachable as `aikit session list --live` with the same `--serve-url` flag.

```bash
aikit session list [--tool <kind>]... [--path [<kind>=]<dir>]... [--since <when>]
                   [--db <file>] [--format default|json] [--live [--serve-url <url>]]

aikit session summarize (--session <id>... | --since <when> | --all)
                   [--tool <kind>]... [--path [<kind>=]<dir>]... [--db <file>]
                   [--model <m>] [--base-url <url>] [--api-key-env <VAR>]
                   [--tags <a,b,c> | --tags-file <toml>] [--areas-file <toml>] [--area-depth N]
                   [--include-assistant] [--parallel N] [--force] [--no-mirror]
                   [--dry-run] [--format default|json]
```

- `--tool` is repeatable and accepts `claude_code`, `codex`, `open_code` (with
  the aliases `session sync` accepts).
- `--path` overrides the registry's homes. `claude_code=/scratch/claude` binds
  a root to one adapter; a bare `/scratch` is offered to every compiled
  adapter and a file is claimed by name shape (UUID stem → Claude Code,
  `rollout-*`/`session-*` → Codex, `opencode.db` → OpenCode). Without `--path`
  the adapters' own resolution applies: override env (`CLAUDE_HOME`,
  `CODEX_HOME`) then `$HOME`.
- `--since` takes a duration (`24h`, `7d`) or an RFC 3339 timestamp and
  filters on the session's last event.
- `--db` (env `AIKIT_CAPTURE_DB`) is the capture SQLite file, defaulting to
  the one `aikit serve` uses (`<data dir>/aikit/capture.db`). CI runs point
  it at a scratch file.
- `--session` is repeatable; an id may be a unique prefix of at least 8
  characters. Exactly one of `--session`, `--since`, `--all` selects.
- Model: `--model` (env `AIKIT_MODEL`), `--base-url` (env `AIKIT_LLM_URL`,
  default the gateway's), `--api-key-env` (default order `OPENAI_API_KEY`,
  `AIKIT_API_KEY`), temperature 0, `--max-tokens` 4096. Exactly the judge's
  shape (ADR 0021): one `LlmGateway::complete` per session, no agent loop.
- `--dry-run` builds and prints every digest and its mechanical tags and
  makes no model call. `--format json` prints machine output on stdout;
  progress and warnings go to stderr.
- Exit codes follow `session sync`: `0` all done, `1` at least one session
  failed, `2` configuration error before any call.

`list` first scans the selected locations into the store (cursor-resumed,
idempotent, the same `ingest` path serve uses) and then queries
`sessions_for`. Output columns: session id, tool, start, end, actions, git
root, source file.

## Tags

The list comes from `--tags a,b,c` (names only), `--tags-file <toml>`, env
`AIKIT_SESSION_TAGS` (a file path), else a built-in default. A file:

```toml
[[tag]]
name = "test"
description = "Only test files were modified"
rule = "only_tests"          # optional: decided in code before the model call

[[tag]]
name = "feature"
description = "New behaviour was added"
```

Rules are a closed enum: `only_tests`, `only_docs`, `only_config`,
`read_only`. The built-in list is `feature`, `bugfix`, `refactor`, `test`,
`docs`, `config`, `research`, `chore` with the four rules bound. Mechanical
tags are computed first and named in the digest as already assigned. The
model replies with JSON `{"summary": "...", "tags": [{"name": "...", "why": "..."}]}`;
a tag outside the list is rejected, the model is asked once more with the
rejected names quoted back, and any name still unknown is dropped and recorded
in the brief's `rejected_tags`. Every tag the model assigns carries its
one-line `why`. The primary tag is the first mechanical tag, else the model's
first.

## Digest

Built from the event store only: prompts (Codex stores them as events;
Claude Code prompts come through the history reader when the session is in
the default Claude home), files touched in order with action kinds
(consecutive duplicates collapsed), commands run, the areas table, the final
assistant message, and with `--include-assistant` the assistant text blocks.
Each section has a character budget and the whole digest is capped near
12 000 characters (a few thousand tokens). The rendered digest is passed
through `SecretScrubber` once more before it leaves the process, so history
reader text is scrubbed exactly like adapter output. The digest hash is the
SHA-256 of the exact user message sent, which embeds the tag list and the
mechanical tags: same input, same hash, and `summarize` is a no-op on a
matching stored hash unless `--force`.

## Areas

Targets of `Read` count as reads; `Write`, `Edit`, `Delete` count as
modifications. A path is made relative to the session's git root when it
lies under it. The area is the first `--area-depth` components of the parent
directory (default 2, so `src/cli/serve/capture.rs` is `src/cli`), or the
longest matching prefix from `--areas-file`:

```toml
[[area]]
name = "capture"
prefix = "aikit-session-capture/"
```

Time per area is the gap from an event to the next one in the session, capped
at five minutes, attributed to the earlier event's area; the last event gets
its own `duration_ms` if any. Deterministic, and honest about idle gaps.

## Data model

```rust
pub struct SessionBrief {
    pub tool: ToolKind,
    pub session_id: String,
    pub summary: String,
    pub areas: Vec<AreaTouch>,
    pub tags: Vec<TagAssignment>,
    pub model: String,            // the model asked for
    pub digest_hash: String,      // sha256 hex of the user message sent
    pub generated_at_ms: i64,
    #[serde(default)] pub model_reported: Option<String>,
    #[serde(default)] pub rejected_tags: Vec<String>,
    #[serde(default)] pub prompt_source: Option<String>,   // "history" | "events" | none
}
pub struct AreaTouch { pub area: String, pub reads: u64, pub modifications: u64,
                       pub time_ms: u64, pub files: Vec<String> }
pub struct TagAssignment { pub name: String, pub source: TagSource /* mechanical | model */,
                           pub justification: String }
```

SQLite: `capture_session_briefs(tool, session_id, summary, areas TEXT, tags
TEXT, model, digest_hash, generated_at_ms, extra TEXT DEFAULT '{}', PRIMARY
KEY (tool, session_id))`, created by the existing `IF NOT EXISTS` migration.
`areas`, `tags` and `extra` are JSON so new fields never need a column
migration. The struct follows ADR 0020: fields are only ever added, with
`#[serde(default)]`.

## Persistence and mirroring

`BriefStore::put_brief` replaces the row for (tool, session id). After a successful
write, when the tool's Backend has a `HistoryMutator` (Claude today) the
primary tag is written to the backend's tag slot via
`Backend::history_mutator().tag(...)`; a failure there is a warning, never a
failed session. `--no-mirror` skips it.

## Serve

Two read-only routes join the capture router; `CaptureState` gains a
`BriefStore` handle (the same SQLite store) beside its event store:

- `GET /api/v1/capture/{backend}/briefs?limit=&offset=`
- `GET /api/v1/capture/{backend}/sessions/{session_id}/brief` (404 `not_found`
  when none)

Listing sessions over HTTP already exists as `GET /capture/{backend}/sessions`.
Generating briefs over HTTP is left out: it needs model configuration on the
server and a job model like `POST /capture/scan`, which is a follow-up.

## Out of scope

- Generating briefs from `aikit serve` or MCP (read routes only).
- Claude Code prompts for sessions outside the default Claude home: the
  history reader resolves only `CLAUDE_CONFIG_DIR`/`~/.claude`; the digest
  still carries files, commands and the final message. Emitting prompts from
  the Claude adapter is the follow-up that closes this.
- A `--watch` mode; briefs are generated on demand.
- Any cost estimate on a brief (ADR 0020: never an estimate).
- Cursor and Gemini adapters (reserved `ToolKind`s with no adapter).

## Tests

Unit: digest sections and budgets, area grouping with and without a mapping,
time attribution, each mechanical rule, tag validation and the retry path,
`--since` parsing, path claiming by name shape. Store: SQLite brief
round-trip and replace. Flow: the Claude Code and Codex fixtures under
`aikit-session-capture/tests/fixtures` are scanned into `InMemoryEventStore`
and summarized through `MockGateway`, asserting the brief, the no-op on the
second run and the `--force` path. CLI: `session summarize --dry-run` against
a scratch `--path` and `--db`.
