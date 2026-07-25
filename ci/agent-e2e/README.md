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

The workflow **skips cleanly** (green, no-op) when any of these are unset — so
forks and secret-less environments don't fail.

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
