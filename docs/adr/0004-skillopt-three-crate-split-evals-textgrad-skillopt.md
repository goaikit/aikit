# SkillOpt is split into three crates: aikit-evals, aikit-textgrad, aikit-skillopt

Rather than building SkillOpt as one feature crate, we split it along two seams:

- **aikit-evals** — pure evaluation: run a target against a case, capture the trajectory, score it, persist/read eval artifacts. The reward mechanism and nothing more.
- **aikit-textgrad** — the artifact-agnostic text-gradient optimization algorithm: edits/patches and their application, the epoch/step loop, textual learning rate, rejected-edit buffer, slow update with a protected region, meta-skill, the monotonic-best invariant, and training-run resume. It calls evals to roll out and gate, and is parameterized over the trainable artifact via an `Optimizable` trait.
- **aikit-skillopt** — a particular application of textgrad where the trainable artifact is a skill document, materialized into each rollout workspace via `aikit-sdk::deploy_skill`. It supplies the `Optimizable` impl and skill-flavored optimizer prompts.

Dependency direction: `aikit-skillopt → aikit-textgrad → aikit-evals → aikit-sdk`. CLI commands live in `aikit-cli`.

## Why

The genuinely reusable thing is the *algorithm* (text-gradient optimization over a text artifact), not its first application (optimizing a skill file). Folding everything into one crate would weld the algorithm to skill-specific mechanics (`deploy_skill`, `SKILL.md`, agent skill-loading) and make a second application — optimizing a prompt template, a config doc, an agent definition — impossible without forking. It also keeps `aikit-evals` a clean evaluation substrate that `newton` already consumes via path→git (the same extraction rationale as ADR-0001 for `aikit-magictool`), undragged by optimization dependencies. The `Optimizable` seam is the load-bearing boundary: if `aikit-textgrad` ever needs to name "skill," the split has leaked.

## Consequences

- `aikit-textgrad` is artifact-agnostic; all skill-specific behavior sits behind `Optimizable` in `aikit-skillopt`.
- Training-run resume (rejected-edit buffer, step/epoch counters, LR position, meta-skill prompts) lives in `aikit-textgrad` — it is optimization state, not eval state. Eval-artifact recovery stays in `aikit-evals`.
- The train/selection/test `split` is a textgrad concept carried as an `EvalCase` tag; `aikit-evals` stays oblivious and needs no change to support it.
- A future non-skill application reuses `aikit-textgrad` + `aikit-evals` unchanged, supplying only a new `Optimizable` impl.
