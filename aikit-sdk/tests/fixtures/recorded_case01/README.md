# recorded_case01

Real newline-delimited JSON (JSONL) captures from each supported agent, taken under the same prompt and stream settings. They are intended for **schema reference**, **extractor unit tests** (token usage, duration, turns), and **regression checks** when upstream CLIs change output shape.

## Prompt

All recordings used the same user message:

```text
Reply with one word only: ok.
```

## How they were generated

1. **Script:** Newton **`coder.sh`** (e.g. `~/.newton/scripts/coder.sh`, or `ws001/.newton/scripts/coder.sh` beside the `goaikit` checkout), or any wrapper that passes the same flags. The **`-s` / `--stream`** flag turns on each agent’s machine-readable stream (JSONL on stdout where supported).

2. **Working directory:** Captures were originally produced from `~/.newton/scripts` with `tee` into `captures/*.jsonl`; files were then copied here unchanged (aside from path in this repo).

3. **Per-agent commands** (stdin carries the prompt; adjust `-m` if your account requires different models):

   ```bash
   P='Reply with one word only: ok.'
   SH=/path/to/coder.sh   # e.g. ~/.newton/scripts/coder.sh

   printf '%s\n' "$P" | "$SH" -a codex       -m gpt-5-codex           -y -s | tee codex.jsonl
   printf '%s\n' "$P" | "$SH" -a claude      -m sonnet                -y -s | tee claude.jsonl
   printf '%s\n' "$P" | "$SH" -a gemini      -m gemini-2.5-flash      -y -s | tee gemini.jsonl
   printf '%s\n' "$P" | "$SH" -a agent        -m sonnet-4              -y -s | tee cursor-agent.jsonl
   printf '%s\n' "$P" | "$SH" -a opencode     -m zai-coding-plan/glm-4.7 -y -s | tee opencode.jsonl
   ```

   **Claude Code** expects model **aliases** (`sonnet`, `opus`, `haiku`) or full model ids, not Cursor-style names. **Cursor `agent`** uses its own model ids (e.g. `sonnet-4`). **OpenCode** needs a real `provider/model` known to your install.

4. **Do not pass `--verbose` to Cursor `agent`:** Current `agent` binaries reject that flag and exit before emitting JSON.

## Files

| File | Agent CLI | Notes |
|------|-----------|--------|
| `codex.jsonl` | OpenAI Codex (`codex exec --json`) | Look for `turn.completed` and `usage`. |
| `claude.jsonl` | Anthropic Claude Code | `stream-json` + `--verbose` on the Claude side; rich `stream_event` and final `result`. |
| `gemini.jsonl` | Google Gemini CLI | `stream-json`; final `result.stats` for tokens and timing. |
| `cursor-agent.jsonl` | Cursor Agent (`agent --print`) | `stream-json`; `result.usage` uses **camelCase** field names. |
| `opencode.jsonl` | OpenCode (`opencode run --format json`) | Token blocks under `step_finish.part.tokens`. |

## Refreshing these fixtures

When an agent’s JSONL shape changes:

1. Re-run the commands above with the same prompt (or update this README if the scenario changes).
2. Replace the JSONL files in this directory.
3. Note the **CLI versions** you used (e.g. `codex --version`, `claude --version`, `agent --version`, `gemini --version`, `opencode --version`) in your commit message or a short note below.

### Recorded with (fill in when updating)

- Date:
- `codex`:
- `claude`:
- `gemini`:
- `agent`:
- `opencode`:

## Privacy / hygiene

These files may contain **session IDs**, **cwd**, and **request IDs**. For public branches, consider redacting or regenerating from a neutral workspace if anything sensitive appears.
