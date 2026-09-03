# Context Map

## Contexts

- [Optimization Loop](./CONTEXT.md) — iterative scoring/optimization of a skill document (evals, textgrad, skillopt): epochs, splits, gates, rollouts.
- [Skill Evaluation](./aikit-evals/CONTEXT.md) — running a target agent against cases, recording every trial as artifacts, reducing them to outcomes, verdicts, and judgments.
- [SDK Agent Runner](./aikit-sdk/CONTEXT.md) — spawning external agent CLIs (Backends), decoding their per-agent Dialects into the canonical agent-event vocabulary.

## Relationships

- **Skill Evaluation → Optimization Loop**: the loop consumes evaluation through the Scorer; trial outcomes reach it already reduced to case verdicts and a split-level score.
- **Runner → Skill Evaluation**: every trial is one Agent run through the SDK runner; the trace a check or a judge reads is the runner's canonical event vocabulary, normalized. A judge is never an Agent run: it is one model completion the engine composes in full, so nothing a harness adds reaches the judge.
- **Runner → Optimization Loop**: the optimization loop runs Target and Optimizer agents through the SDK runner; a `Backend` is what executes a rollout or an optimizer stage.
- **Runner → Event Streaming Protocol**: the runner's decode step produces the canonical vocabulary defined in [ADR 0005](./docs/adr/0005-agent-events-are-the-shared-streaming-protocol.md), consumed by serve, agentrt, and the chat UI.
- **Skill Evaluation → fastskill**: a run directory's artifacts — summary, trial results, traces, judgments — are an additive-only contract ([ADR 0020](./docs/adr/0020-eval-artifacts-are-an-additive-only-contract.md)) read by `fastskill`'s eval commands, which add the Benchmark / Metric / Scorecard vocabulary on top.
