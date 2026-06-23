# The built-in aikit agent is a Backend with an in-process Transport

## Status

accepted (revises [0006](0006-runner-backend-key-cursor-rename.md) and [0007](0007-backend-transport-seam.md), which framed aikit as "not a Backend / the host runtime")

## Context

ADRs 0006/0007 modelled the built-in `aikit` agent as the host runtime, explicitly *not* a Backend, because it spawns no subprocess and emits canonical events directly. But aikit needs to participate in the same capability model as the external agents (it is in fact the richest — tool calls, subagents, context compression, step lifecycle), and maintaining two parallel worlds (Backends vs the built-in) duplicates the identity/capability surface. The Transport seam from ADR 0007 dissolves the original objection: "no subprocess" is just a different Transport, not a different kind of thing.

## Decision

The `Backend` enum includes `Aikit`, making six Backends. aikit's Transport is **in-process**: it produces canonical events directly with no Dialect decode step (the decode arm is identity/passthrough), alongside the subprocess-stdout-lines Transport used by the other five. Capabilities are declared per Backend including aikit, which sets the high-water mark. There is still no generic/fallback Backend; the set remains closed and exhaustive.

## Consequences

- One identity model and one capability vocabulary across all runnable agents, consistent with [ADR 0005](0005-agent-events-are-the-shared-streaming-protocol.md).
- The `agent_key: String::new()` empty-string placeholder for the built-in path (`agent_runner.rs`) becomes `"aikit"`, removing a latent smell.
- "Backend" no longer implies "external subprocess"; its definition broadens to "a runnable agent driven over a Transport." The in-process Transport is built now (aikit already exists); it is not speculative.
