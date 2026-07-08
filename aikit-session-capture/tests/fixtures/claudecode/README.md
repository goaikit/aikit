# Claude Code test fixtures

Vendored from `superbased-observer/testdata/claudecode/` at
https://github.com/marmutapp/superbased-observer (Apache-2.0 licensed —
compatible with aikit's Apache-2.0).

## Files

| File | What it exercises |
|---|---|
| `simple-session.jsonl` | Basic user→assistant→tool_use→tool_result flow. 5 lines, 1 Read + 1 Bash. |
| `multi-tool-turn.jsonl` | One assistant turn emitting 3 parallel tool_use blocks (Grep + Glob + WebSearch) followed by 3 paired tool_result blocks (one errored). |
| `multi-block-dedup.jsonl` | Multiple JSONL records sharing one Anthropic `msg_*` id with progressing usage envelopes — must dedup to a single TokenEvent (highest output_tokens wins). |
| `api-error.jsonl` | `type:"system", subtype:"api_error"` records (400 content-policy, 429 rate-limit, 529 overloaded with nested envelope). |
| `malformed-line.jsonl` | One malformed JSON line in the middle; parser must skip it, advance the cursor, and parse the lines around it. |
| `concatenated-records.jsonl` | Multiple JSON objects on one physical line (writer corruption pattern). The reference observer impl has a recovery path; the aikit Phase 2 MVP treats this as one malformed line and emits a warning. |

## `with-secrets.jsonl` (synthetic, NOT from observer)

Hand-authored fixture containing fake credentials for the scrub invariant
test. Documented as synthetic in a header comment in the file.
