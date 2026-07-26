# Nightly Agent E2E — setup guide

The **Nightly Agent E2E** workflow runs *actual* agents (Claude Code always;
Codex when configured), driven by `aikit`, against our LLM gateway(s) **over
the Tailnet**, then asserts the capture + sync pipeline end-to-end. It's the
one place agentic behaviour is exercised with a real agent instead of a mocked
transport, so PR CI can stay fast and offline.

- Workflow: `.github/workflows/nightly-agent-e2e.yml`
- Harness: `ci/agent-e2e/` (`Dockerfile.agent-e2e`, `smoke.sh`, `README.md`)
- Triggers: nightly (06:00 UTC) **and** manual `workflow_dispatch`
- Runs on `goaikit/aikit` only; **not** on PRs or forks

The job **skips cleanly** (green no-op) until the four required secrets below
are set, so it never blocks anyone before it's configured. The Codex case has
its own optional trio of secrets (see below) and skips cleanly on its own if
they're absent, independent of the job-level guard.

## What it does

1. Joins the Tailnet as an **ephemeral, tagged (`tag:ci`) node**.
2. Builds the `ci/agent-e2e` image (the `aikit` binary + the Claude Code CLI +
   the Codex CLI).
3. Runs `ci/agent-e2e/smoke.sh` in the container with `--network host` (so it
   reaches the internal gateway(s) over the Tailnet). The suite runs one case
   per agentic behaviour and aggregates a single pass/fail verdict:
   - `case_claude_turn` — a real `aikit agent run --agent claude` turn
     completes, a transcript is captured under `~/.claude/projects`, and
     `aikit session sync --dry-run` detects it (`synced >= 1`).
   - `case_claude_tool_use` — Claude is prompted to actually **call** its
     file-writing tool (not just describe doing so); asserted by finding a
     `tool_use` content block in the captured transcript (authoritative),
     with the written file logged as a secondary, non-gating signal.
   - `case_streaming` — `--events` emits parseable JSON `AgentEvent` lines,
     including at least one carrying `.payload.stream_message` or
     `.payload.result`.
   - `case_resume_claude` — a second turn with `--resume <session-id>`
     appends to the **same** transcript rather than starting a new one.
     `--resume-last` doesn't work for external agent CLIs (only the built-in
     backend updates the session index), so the session id is obtained by
     globbing the newest `~/.claude/projects/*/*.jsonl` after the first turn
     — the filename stem **is** the session id.
   - `case_codex` — the same turn/capture/sync shape via the Codex backend.
     **Skips cleanly** unless `CODEX_BASE_URL`, `CODEX_API_KEY`, and
     `CODEX_MODEL` are all set.

The assertions check the **pipeline/structure, not the model's wording**, so a
modest self-hosted model behind the gateway still passes every case.

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

### Optional: enabling the Codex case

Codex requires an **OpenAI-compatible** route from the gateway (a LiteLLM
gateway, for instance, can serve both the Anthropic-compatible route used by
Claude and an OpenAI-compatible route used by Codex from the same deployment).
`smoke.sh` seeds `~/.codex/config.toml` at runtime from these values — nothing
Codex-specific is baked into the image — and `--resume-last`-style session
detection uses the session id embedded in the rollout file itself (see the
harness README), not a filename-stem convention like Claude's.

| Secret | Value |
|--------|-------|
| `CODEX_BASE_URL` | Gateway **OpenAI-compatible** base URL, including any `/v1` suffix, e.g. `http://gateway.<tailnet>.ts.net:4000/v1`. |
| `CODEX_API_KEY` | Gateway API key for that route. |
| `CODEX_MODEL` | Model to run through Codex. |

```bash
gh secret set CODEX_BASE_URL --repo goaikit/aikit --body "http://gateway.<tailnet>.ts.net:4000/v1"
gh secret set CODEX_API_KEY  --repo goaikit/aikit          # prompts, hidden
gh secret set CODEX_MODEL    --repo goaikit/aikit --body "<codex-model>"
```

These are deliberately **not** part of the job-level guard: leaving any of
them unset does not skip the whole nightly run, it just skips `case_codex` in
`smoke.sh` (counted as a pass) while the Claude cases still run and gate the
job.

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

Add the three `CODEX_*` env vars to also exercise `case_codex`:

```bash
docker run --rm --network host \
  -e ANTHROPIC_BASE_URL="http://gateway.<tailnet>.ts.net:4000" \
  -e ANTHROPIC_AUTH_TOKEN="<gateway-key>" \
  -e CODEX_BASE_URL="http://gateway.<tailnet>.ts.net:4000/v1" \
  -e CODEX_API_KEY="<gateway-key>" \
  -e CODEX_MODEL="<codex-model>" \
  aikit-agent-e2e
```

## Troubleshooting

| Symptom | Likely cause / fix |
|---------|--------------------|
| Job skipped with "secrets not configured" | One of the four **required** secrets is unset. `gh secret list --repo goaikit/aikit`. (The three `CODEX_*` secrets are optional and don't affect this guard.) |
| Tailscale step fails: *requested tags are invalid or not permitted* | `tag:ci` isn't in `tagOwners`, or the OAuth client isn't tagged `tag:ci`. Fix the ACL / regenerate the client (step 1–2). |
| Turn fails to connect / times out | ACL doesn't allow `tag:ci` → the gateway node/port (step 2b), or `LLM_GATEWAY_URL` isn't the Tailnet address. Confirm the gateway is up on the Tailnet. |
| Connects but 401/403 | `LLM_GATEWAY_KEY` wrong, or the gateway expects a different auth header than `ANTHROPIC_AUTH_TOKEN`. |
| MagicDNS name doesn't resolve in the container | Use the gateway's `100.x` Tailnet IP in `LLM_GATEWAY_URL` instead of the MagicDNS name (`--network host` should carry MagicDNS, but the raw IP always works). |
| `synced=0` (no transcript detected) | The agent turn produced no session file — check the turn's own output above; usually an upstream connect/auth failure, not a sync bug. |
| `case_codex` always shows `[SKIP]` | One of `CODEX_BASE_URL`/`CODEX_API_KEY`/`CODEX_MODEL` is unset — this is by design, not a failure. Set all three to enable it. |
| `case_codex` fails to connect / 401 | `CODEX_BASE_URL` isn't a valid OpenAI-compatible route (must include `/v1` if the gateway expects it), or `CODEX_API_KEY` is wrong for that route. Check the `~/.codex/config.toml` the case wrote (logged at the start of the case). |
| `case_resume_claude` fails ("did not append to the same transcript") | Something started a fresh session instead of resuming — check the resumed turn's own output for a `--resume` rejection from the Claude Code CLI (e.g. unknown session id), which usually means the seed turn's transcript wasn't flushed yet or `--cd`/workdir mismatched between the two turns. |

## Extending

This is the reusable base for real-agent testing — grow `ci/agent-e2e/`:

- **More agents:** add the CLI to `Dockerfile.agent-e2e` (`gemini`, …) and a
  case in `smoke.sh`, following the `case_codex` pattern for agents whose
  gateway config isn't handled by aikit directly (custom config file +
  runtime-injected secrets, session files discovered by globbing rather than
  a filename-stem convention).
- **More behaviours:** multi-turn conversations, error handling, cancellation.
  Keep assertions on structure/pipeline, not model wording.
- **More environments:** matrix the runner/image over OSes or agent-CLI versions.
