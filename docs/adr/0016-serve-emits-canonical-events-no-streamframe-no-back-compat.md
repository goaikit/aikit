# serve emits canonical agent events; StreamFrame is deleted, no back-compat shim

## Status

accepted (realizes [0005](0005-agent-events-are-the-shared-streaming-protocol.md); resolves audit ARCH-4)

## Context

`src/cli/serve/mod.rs` defines a second event vocabulary (`StreamFrame`) and lossily
re-maps the canonical `AgentEventPayload` onto it before writing SSE. The remap drops
events consumers want (token usage, reasoning, subagent activity, context compression,
step-finish — per `specs/serve-agent-fidelity-and-ndjson-parity.md`) and overloads
`ToolResult.name` with two meanings (agent-key for CLI backends, call-id for structured
ones). This contradicts ADR 0005, which established the canonical agent events as *the*
shared streaming protocol across serve, agentrt, and the optimization loop.

serve's consumers (agentrt, the optimization loop, the chat UI) parse the current
`StreamFrame` shape today, so changing the wire is a breaking change for them. The project
stance is to **advance the design rather than preserve backward compatibility** (greenfield,
break for the better shape). A dual-vocabulary/additive option was considered and rejected
as the compatibility option it is.

## Decision

serve serializes the canonical `AgentEventPayload` **directly** over SSE. `StreamFrame` and
its translation layer are **deleted** — no dual emission, no deprecation window, no
back-compat shim. Consumers migrate to the canonical shape in one step.

This lands in **Phase 2** of the remediation, **independent of and ahead of ARCH-3** (the
session-trait seam). ARCH-4 overlaps ARCH-3 in the serve file but does not depend on it:
emitting canonical events is a contained change at the serialization point, so it is not
held back to be bundled with the larger seam rewrite.

## Consequences

- ADR 0005 is realized on the serve surface: one event vocabulary end-to-end, no lossy
  translation.
- Previously-dropped events (token usage, reasoning, subagent, compression, step-finish)
  reach consumers, unblocking cost display and richer chat UI.
- The overloaded `ToolResult.name` disappears with the remap; `call_id` and agent identity
  are distinct canonical fields.
- Consumers (agentrt, optimization loop, chat UI) take a one-time breaking migration in
  Phase 2. This is accepted as the intended direction of travel, not a regression.
- The duplicated `UsageSource`→string map (`serve/mod.rs:425` vs `run_progress.rs:199`) is
  removed in the same change.
