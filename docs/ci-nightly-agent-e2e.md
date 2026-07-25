# Nightly Agent E2E — setup guide

The **Nightly Agent E2E** workflow runs *actual* agents (Claude Code, driven by
`aikit`) against our LLM gateway **over the Tailnet**, then asserts the capture +
sync pipeline end-to-end. It's the one place agentic behaviour is exercised with
a real agent instead of a mocked transport, so PR CI can stay fast and offline.

- Workflow: `.github/workflows/nightly-agent-e2e.yml`
- Harness: `ci/agent-e2e/` (`Dockerfile.agent-e2e`, `smoke.sh`, `README.md`)
- Triggers: nightly (06:00 UTC) **and** manual `workflow_dispatch`
- Runs on `goaikit/aikit` only; **not** on PRs or forks

The job **skips cleanly** (green no-op) until the secrets below are set, so it
never blocks anyone before it's configured.

## What it does

1. Joins the Tailnet as an **ephemeral, tagged (`tag:ci`) node**.
2. Builds the `ci/agent-e2e` image (the `aikit` binary + the Claude Code CLI).
3. Runs `ci/agent-e2e/smoke.sh` in the container with `--network host` (so it
   reaches the internal gateway over the Tailnet), which:
   - runs a real `aikit agent run --agent claude` turn against the gateway,
   - asserts a transcript was captured under `~/.claude/projects`,
   - asserts `aikit session sync --dry-run` detects it.

The assertions check the **pipeline, not the model's wording**, so a modest
self-hosted model behind the gateway still passes.

## One-time setup

You need repo **admin** on `goaikit/aikit` and **admin** on the Tailscale tailnet.

### 1. Create a Tailscale OAuth client

The workflow uses `tailscale/github-action@v3`, which exchanges an OAuth client
for a short-lived, tagged, ephemeral auth key on each run.

1. Open the Tailscale admin console → **Settings → OAuth clients**
   (<https://login.tailscale.com/admin/settings/oauth>).
2. Click **Generate OAuth client…**.
3. **Description:** something identifiable, e.g. `goaikit-aikit CI`.
4. **Scopes:** grant **Auth Keys → Write** (the `auth_keys` scope). That is all
   the action needs — it mints the ephemeral node key itself.
5. **Tags:** attach **`tag:ci`**. An OAuth client must be tagged; the nodes it
   creates inherit this tag. (If `tag:ci` isn't offered yet, define it in the
   ACL first — see step 2 — then come back.)
6. Click **Generate client**. Copy the **client ID** and **client secret** now —
   the secret is shown only once.

### 2. Authorize the tag and gateway access in the ACL

Open **Access Controls** (<https://login.tailscale.com/admin/acls>) and make two
edits.

**a. Declare the tag** under `tagOwners`:

```jsonc
"tagOwners": {
  "tag:ci": ["autogroup:admin"]
}
```

**b. Allow `tag:ci` to reach the gateway.** Adapt the destination to how your
gateway node is tagged/addressed and the port it serves (e.g. LiteLLM on
`:4000`):

```jsonc
"acls": [
  { "action": "accept", "src": ["tag:ci"], "dst": ["tag:llm-gateway:4000"] }
]
```

Replace `tag:llm-gateway:4000` with your gateway's tag (or host) and port.
Ephemeral `tag:ci` nodes are removed automatically when each job ends.

### 3. Add the repository secrets

`goaikit/aikit` → **Settings → Secrets and variables → Actions → Repository
secrets** (or use `gh` below).

| Secret | Value |
|--------|-------|
| `TS_OAUTH_CLIENT_ID` | OAuth **client ID** from step 1 |
| `TS_OAUTH_SECRET` | OAuth **client secret** from step 1 |
| `LLM_GATEWAY_URL` | Gateway Anthropic-compatible base URL **on the Tailnet** — a MagicDNS name or `100.x` IP, e.g. `http://gateway.<tailnet>.ts.net:4000` |
| `LLM_GATEWAY_KEY` | Gateway API key |
| `LLM_GATEWAY_MODEL` | *(optional)* model to run, e.g. `claude-sonnet-4-6`. Must be one the gateway key is allowed to serve. Falls back to `claude-sonnet-4-6` if unset. |

With `gh` (run the secret-bearing ones in your own terminal so values aren't
echoed into logs):

```bash
gh secret set TS_OAUTH_CLIENT_ID --repo goaikit/aikit --body "<client-id>"
gh secret set TS_OAUTH_SECRET    --repo goaikit/aikit          # prompts, hidden
gh secret set LLM_GATEWAY_URL    --repo goaikit/aikit --body "http://gateway.<tailnet>.ts.net:4000"
gh secret set LLM_GATEWAY_KEY    --repo goaikit/aikit          # prompts, hidden

gh secret list --repo goaikit/aikit    # confirm names (never values)
```

> Prefer sharing across several `goaikit` repos? Set them as **organization**
> secrets (Org → Settings → Secrets and variables → Actions) scoped to the repos
> that need them — the workflow reads them identically.

### 4. First run

```bash
gh workflow run nightly-agent-e2e.yml --repo goaikit/aikit
gh run watch --repo goaikit/aikit \
  "$(gh run list --repo goaikit/aikit --workflow nightly-agent-e2e.yml -L1 --json databaseId -q '.[0].databaseId')"
```

Or in the UI: **Actions → Nightly Agent E2E → Run workflow**.

## Run the smoke locally

From a machine already on the Tailnet:

```bash
docker build -f ci/agent-e2e/Dockerfile.agent-e2e -t aikit-agent-e2e .
docker run --rm --network host \
  -e ANTHROPIC_BASE_URL="http://gateway.<tailnet>.ts.net:4000" \
  -e ANTHROPIC_AUTH_TOKEN="<gateway-key>" \
  aikit-agent-e2e
```

## Troubleshooting

| Symptom | Likely cause / fix |
|---------|--------------------|
| Job skipped with "secrets not configured" | One of the four secrets is unset. `gh secret list --repo goaikit/aikit`. |
| Tailscale step fails: *requested tags are invalid or not permitted* | `tag:ci` isn't in `tagOwners`, or the OAuth client isn't tagged `tag:ci`. Fix the ACL / regenerate the client (step 1–2). |
| Turn fails to connect / times out | ACL doesn't allow `tag:ci` → the gateway node/port (step 2b), or `LLM_GATEWAY_URL` isn't the Tailnet address. Confirm the gateway is up on the Tailnet. |
| Connects but 401/403 | `LLM_GATEWAY_KEY` wrong, or the gateway expects a different auth header than `ANTHROPIC_AUTH_TOKEN`. |
| MagicDNS name doesn't resolve in the container | Use the gateway's `100.x` Tailnet IP in `LLM_GATEWAY_URL` instead of the MagicDNS name (`--network host` should carry MagicDNS, but the raw IP always works). |
| `synced=0` (no transcript detected) | The agent turn produced no session file — check the turn's own output above; usually an upstream connect/auth failure, not a sync bug. |

## Extending

This is the reusable base for real-agent testing — grow `ci/agent-e2e/`:

- **More agents:** add the CLI to `Dockerfile.agent-e2e` (`codex`, `gemini`, …)
  and a case in `smoke.sh`. Codex needs an OpenAI-compatible route from the
  gateway.
- **More behaviours:** tool-use round-trips, streaming, resume, multi-turn.
  Keep assertions on structure/pipeline, not model wording.
- **More environments:** matrix the runner/image over OSes or agent-CLI versions.
