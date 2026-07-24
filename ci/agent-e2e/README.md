# Real-agent E2E harness

Runs **actual agents** (Claude Code, driven by `aikit`) against a custom
LLM gateway and asserts the capture + sync pipeline end-to-end. This is where
agentic behavior gets exercised with a real agent instead of mocks, so PR CI
can stay fast and offline.

## Why

Unit/integration tests mock the transport. That can't catch "does a real agent
turn actually complete, get captured to `~/.claude/projects`, and get detected
by `aikit session sync`?". This harness does, using your own gateway
(Anthropic-compatible, backed by a self-hosted model) so there's **no real
Anthropic spend**.

## Pieces

| File | Role |
|------|------|
| `Dockerfile.agent-e2e` | Image: the `aikit` binary + the Claude Code CLI. |
| `smoke.sh` | The suite: one real turn → assert capture → assert sync detection. |
| `../../.github/workflows/nightly-agent-e2e.yml` | Nightly (06:00 UTC) + manual `workflow_dispatch`. |

The smoke asserts the **pipeline, not the model's wording** — a modest
self-hosted model must still pass. It checks: `aikit agent run --agent claude`
exits 0, a `*.jsonl` transcript appears under `~/.claude/projects`, and
`aikit session sync --dry-run` reports `synced >= 1`.

## Required repo secrets

| Secret | Meaning |
|--------|---------|
| `LLM_GATEWAY_URL` | The gateway's Anthropic-compatible base URL (public HTTPS). |
| `LLM_GATEWAY_KEY` | The gateway API key. |

The workflow **skips cleanly** (green, no-op) when these are unset — so forks
and secret-less environments don't fail.

## Run it

- **CI:** Actions → *Nightly Agent E2E* → *Run workflow* (or wait for the nightly).
- **Locally:**
  ```bash
  docker build -f ci/agent-e2e/Dockerfile.agent-e2e -t aikit-agent-e2e .
  docker run --rm \
    -e ANTHROPIC_BASE_URL="https://your-gateway.example.com" \
    -e ANTHROPIC_AUTH_TOKEN="your-gateway-key" \
    aikit-agent-e2e
  ```

## Extending

This is the reusable base for real-agent testing. To grow it:

- **More agents** — add the CLI to the Dockerfile (`codex`, `gemini`, …) and a
  case in `smoke.sh` (or a sibling script). Codex needs an OpenAI-compatible
  route from the gateway.
- **More behaviors** — add cases for tool-use round-trips, streaming, resume,
  multi-turn. Keep assertions on structure/pipeline, not model wording, so they
  stay robust against the backing model.
- **More environments** — matrix the runner/image to test different OSes or
  agent-CLI versions.
