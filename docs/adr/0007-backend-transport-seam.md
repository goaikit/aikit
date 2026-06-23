# Backends are built on a Transport seam, not a fixed stdout-lines runner

## Status

accepted

## Context

aikit's runner was unidirectional, stateless, and single-transport: `run_agent_events(prompt, callback)` spawns a CLI, reads newline-delimited stdout, and decodes each line. The Backends aikit is moving toward break all three assumptions. `claude-agent-sdk-rust` already abstracts a `Transport` (reader/writer halves, `connect`/`split`) with `SubprocessCLITransport` as today's only impl but websocket/unix anticipated, plus a bidirectional control protocol and typed messages. `aikit-agent-codex` drives `codex app-server` as bidirectional JSON-RPC 2.0 over stdio (with `--listen` selectable transports) and answers server→client approval requests.

## Decision

The per-Backend unit is built on a **Transport seam** rather than a hardcoded stdout-lines loop. A Backend produces a Transport (reader + writer halves) and a Decoder. Only the subprocess-stdout-lines Transport is implemented in the initial refactor; JSON-RPC-over-stdio, the Claude SDK, websockets, and unix sockets are designed to slot in later as new Transport impls plus richer per-Backend decode, without reworking the Backend abstraction. Bidirectional Control (approvals, interrupts, turn/session lifecycle) is shaped for by the writer half but not built yet. Per-Backend decode may delegate to dedicated typed parsers (`claude-agent-sdk-rust`, `aikit-agent-codex`) instead of `serde_json::Value` poking.

## Consequences

- The refactor is no longer "split the parser into files"; it lays the architecture the rich Claude/Codex crates plug into, avoiding a second rewrite when bidirectional/JSON-RPC backends land.
- The closed `Backend` identity (key-string parsing) and the open runtime behaviour (transport/decode) are likely expressed differently — see the follow-on decision on enum-vs-trait.
- Scope risk: the seam must be implemented with exactly one Transport now; building speculative transports before their backends exist is explicitly out of scope.
