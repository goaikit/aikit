# Real-agent E2E harness

Runs **actual agents** (Claude Code always; Codex when configured), driven by
`aikit`, against custom LLM gateways and asserts the capture + sync pipeline
end-to-end. This is where agentic behavior gets exercised with a real agent
instead of mocks, so PR CI can stay fast and offline.

## Why

Unit/integration tests mock the transport. That can't catch "does a real agent
turn actually complete, get captured to `~/.claude/projects`, and get detected
by `aikit session sync`?" — or "does the agent actually call a tool, not just
describe doing so?", or "does `--resume` really continue the same session?".
This harness does, using your own gateway(s) (Anthropic-compatible for Claude,
OpenAI-compatible for Codex, both backed by a self-hosted model) so there's
**no real Anthropic/OpenAI spend**.

## Pieces

| File | Role |
|------|------|
| `Dockerfile.agent-e2e` | Image: the `aikit` binary + the Claude Code CLI + the Codex CLI. |
| `smoke.sh` | The suite: a case per agentic behavior, aggregated into one pass/fail verdict. |
| `../../.github/workflows/nightly-agent-e2e.yml` | Nightly (06:00 UTC) + manual `workflow_dispatch`. |

The smoke asserts **pipeline/structure, not the model's wording** — a modest
self-hosted model must still pass every case.

## Cases

`smoke.sh` runs each case in order via a small `run_case` harness (prints a
`[name] PASS`/`FAIL` line, aggregates a `FAILURES` counter, and a case can
`skip` cleanly — still counted as a pass — when its prerequisites aren't met).
At the end it prints a pass/fail/skip summary and exits 1 iff any case failed.

| Case | Proves |
|------|--------|
| `case_claude_turn` | `aikit agent run --agent claude` completes a real turn, a transcript lands under `~/.claude/projects`, and `aikit session sync --dry-run` detects it (`synced >= 1`). |
| `case_claude_tool_use` | Claude actually **invokes** a tool (writes `e2e-proof.txt`), not merely describes doing so. Asserted authoritatively via the captured transcript's `tool_use` content blocks (jq over the `.jsonl`); the file's existence is logged as a secondary, non-gating signal. |
| `case_streaming` | `--events` emits parseable, one-JSON-object-per-line `AgentEvent`s, including at least one carrying `.payload.stream_message` or `.payload.result`. |
| `case_resume_claude` | `--resume <session-id>` genuinely continues the same session rather than starting a new one. Since `--resume-last` only works for the built-in backend's own session index (external CLIs like Claude Code don't update it), the session id is obtained by globbing the newest `~/.claude/projects/*/*.jsonl` after the seed turn — the filename stem **is** the session id — and the test asserts the newest transcript's stem is unchanged after the resumed turn. |
| `case_codex` | Same turn/capture/sync shape via the Codex backend. **Skips cleanly** unless `CODEX_BASE_URL`, `CODEX_API_KEY`, and `CODEX_MODEL` are all set (see below). |

## Required repo secrets

> Full setup guide (Tailscale OAuth client, ACL, secrets, first run,
> troubleshooting): **[`docs/ci-nightly-agent-e2e.md`](../../docs/ci-nightly-agent-e2e.md)**.
> This file is the quick reference.

The gateway is reached **over the Tailnet**, not a public endpoint. The nightly
workflow joins the Tailnet as an ephemeral, tagged node, then runs the smoke
container with `--network host` so it can resolve/reach the internal gateway.

| Secret | Meaning |
|--------|---------|
| `TS_OAUTH_CLIENT_ID` | Tailscale OAuth client id, authorized for `tag:ci`. |
| `TS_OAUTH_SECRET` | Tailscale OAuth client secret. |
| `LLM_GATEWAY_URL` | Gateway Anthropic-compatible base URL **on the Tailnet** — a MagicDNS name or `100.x` tailnet IP, e.g. `http://gateway.<tailnet>.ts.net:4000`. |
| `LLM_GATEWAY_KEY` | The gateway API key. |

The workflow **skips cleanly** (green, no-op) when any of these four are unset
— so forks and secret-less environments don't fail.

### Optional: Codex secrets

| Secret | Meaning |
|--------|---------|
| `CODEX_BASE_URL` | Gateway **OpenAI-compatible** base URL, including any `/v1` suffix (e.g. LiteLLM can serve this alongside the Anthropic-compatible route). |
| `CODEX_API_KEY` | The gateway API key for that route. |
| `CODEX_MODEL` | The model to run through Codex. |

These are **not** part of the job guard — the four secrets above are enough to
run the job, and `case_codex` in `smoke.sh` skips cleanly (still counts as a
pass) if any of the three codex secrets is unset. `smoke.sh` seeds
`~/.codex/config.toml` at runtime from these values (a custom
`model_providers.e2e` provider); nothing is baked into the image.

### Tailscale setup (one-time)

1. In the Tailscale admin console → **Settings → OAuth clients**, create a client
   with the **`auth_keys`** scope and attach the tag **`tag:ci`**.
2. In the **ACL policy**, define `tag:ci` under `tagOwners` (e.g.
   `"tag:ci": ["autogroup:admin"]`) and grant it access to the gateway node/port
   (an ACL rule allowing `tag:ci` → the gateway's `:4000`).
3. Add the client id/secret as the `TS_OAUTH_*` repo secrets above.

Ephemeral nodes created by the action auto-remove when the job ends.

## Run it

- **CI:** Actions → *Nightly Agent E2E* → *Run workflow* (or wait for the nightly).
- **Locally** (on a machine already on the Tailnet):
  ```bash
  docker build -f ci/agent-e2e/Dockerfile.agent-e2e -t aikit-agent-e2e .
  docker run --rm --network host \
    -e ANTHROPIC_BASE_URL="http://gateway.<tailnet>.ts.net:4000" \
    -e ANTHROPIC_AUTH_TOKEN="your-gateway-key" \
    aikit-agent-e2e
  ```
  `--network host` lets the container use the host's Tailnet connectivity and
  MagicDNS. If MagicDNS isn't available, use the gateway's `100.x` tailnet IP in
  `ANTHROPIC_BASE_URL` instead of the MagicDNS name.

  To also exercise `case_codex`, add its three env vars:
  ```bash
  docker run --rm --network host \
    -e ANTHROPIC_BASE_URL="http://gateway.<tailnet>.ts.net:4000" \
    -e ANTHROPIC_AUTH_TOKEN="your-gateway-key" \
    -e CODEX_BASE_URL="http://gateway.<tailnet>.ts.net:4000/v1" \
    -e CODEX_API_KEY="your-gateway-key" \
    -e CODEX_MODEL="your-codex-model" \
    aikit-agent-e2e
  ```

## Extending

This is the reusable base for real-agent testing. To grow it:

- **More agents** — add the CLI to the Dockerfile (`gemini`, …) and a case in
  `smoke.sh` (or a sibling script), following the `case_codex` pattern for
  agents whose gateway config isn't handled by aikit directly.
- **More behaviors** — add cases beyond tool-use/streaming/resume: multi-turn
  conversations, error handling, cancellation. Keep assertions on
  structure/pipeline, not model wording, so they stay robust against the
  backing model.
- **More environments** — matrix the runner/image to test different OSes or
  agent-CLI versions.
