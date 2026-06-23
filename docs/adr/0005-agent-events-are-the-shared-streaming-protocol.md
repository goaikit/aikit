# aikit's agent event model is the canonical cross-system streaming protocol

aikit's agent event frames (`text`/`reasoning`/`tool_use`/`tool_result`/`token_usage`/`step_finish`/
`subagent_*`/`error`) are published as the shared **Event Streaming Protocol**
(`specs/event-streaming-protocol.md`), consumed by agentrt, the workspace SSE relay, and the chat UI.

It is an **open tagged-frame** format: aikit owns the *agent-output* vocabulary; downstream runtimes
(agentrt) may add *runtime/meta* frames; host/UI concerns (e.g. MCP-app `ui/resourceUri`) ride in a
namespaced **`meta`** bag — **not** in aikit core.

**Why:** every surface shares one agent-agnostic vocabulary instead of an OpenAI-shaped adapter; aikit
stays a pure agent runtime while UI capabilities remain expressible via `meta`.
