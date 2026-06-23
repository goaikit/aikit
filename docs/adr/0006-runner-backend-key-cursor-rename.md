# The runner's Cursor backend key is `cursor`, not `agent`

## Status

accepted

## Context

The SDK runner identified the Cursor Agent CLI by the key string `"agent"` (binary `agent`), while the deploy catalog called the same tool `"cursor-agent"` and token-usage attribution called it `UsageSource::Cursor`. One tool, three names. The decode function `normalize_agent` was written as a generic `event`/`message`/`result` shape, which made it read like a catch-all fallback rather than what it actually is: an under-specified Cursor decoder.

## Decision

In the runner/backend layer the canonical key for this backend is **`cursor`** (`Backend::Cursor ⇄ "cursor"`), aligned with `UsageSource::Cursor`. The binary probe takes candidates `["cursor-agent", "agent"]` — Cursor ships its CLI as `cursor-agent`, so probing only `agent` was a latent gap. There is no generic/fallback backend: the set of Backends is closed (Claude, Codex, Gemini, OpenCode, Cursor), and an unrecognised key fails to parse at the runner boundary as `RunError::AgentNotRunnable`.

## Consequences

- Breaking change to the runner key string: callers passing `"agent"` (in-repo tests, serve, CLI) must pass `"cursor"`. `AgentEvent.agent_key` now serializes `"cursor"`.
- The deploy catalog's `"cursor-agent"` key lives in a separate bounded context (skill/MCP deployment, with its own `normalize_mcp_agent_key` reconciler) and is intentionally left unchanged here. The runner↔deploy naming divergence for Cursor remains, scoped to that reconciler.
