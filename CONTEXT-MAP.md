# Context Map

## Contexts

- [Optimization Loop](./CONTEXT.md) — iterative scoring/optimization of a skill document (evals, textgrad, skillopt): epochs, splits, gates, rollouts.
- [SDK Agent Runner](./aikit-sdk/CONTEXT.md) — spawning external agent CLIs (Backends), decoding their per-agent Dialects into the canonical agent-event vocabulary.

## Relationships

- **Runner → Optimization Loop**: the optimization loop runs Target and Optimizer agents through the SDK runner; a `Backend` is what executes a rollout or an optimizer stage.
- **Runner → Event Streaming Protocol**: the runner's decode step produces the canonical vocabulary defined in [ADR 0005](./docs/adr/0005-agent-events-are-the-shared-streaming-protocol.md), consumed by serve, agentrt, and the chat UI.
