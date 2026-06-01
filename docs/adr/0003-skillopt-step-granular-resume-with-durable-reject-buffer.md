# SkillOpt resumes at step granularity, but the rejected-edit buffer is durable state

A SkillOpt training run checkpoints at the granularity of one completed step: `runtime_state.json` is written atomically after a step's gate resolves, and a crash anywhere mid-step causes the entire step to re-run. We chose this over stage-granular checkpointing for simplicity, and it is safe against score regression because the skill changes only on gate-accept, which is the last action in a step.

The one exception: the **rejected-edit buffer** is persisted as durable epoch-scoped state, not held in memory. This deliberately deviates from the otherwise-equivalent step-granular design (and from a known competitor implementation that keeps the buffer in memory and resets it per epoch).

## Why the exception

The resume contract requires no *data loss* and no *score regression* — two separate guarantees. Re-running a lost mid-step is not data loss (the work is recomputable and the skill is unchanged). But the rejected-edit buffer accumulates *across* steps within an epoch and is consumed by the epoch-boundary slow update. If a crash or a resume into a mid-epoch step wiped it, the slow update would synthesize guidance from a partial buffer and silently produce weaker guidance than an uninterrupted run — a real correctness regression that leaves no error and no obvious symptom. The trajectory data needed to rebuild the buffer is already written to disk per step; persisting (or reconstructing) it closes the gap at near-zero cost.

## Consequences

- `runtime_state.json` carries the rejected-edit buffer (or it is rebuilt from per-step trajectory digests on startup).
- Mid-step rollout results and intermediate patches are intentionally *not* cached for v1 — re-running them on resume wastes tokens but is correctness-neutral. A rollout cache can be added later without changing the resume contract.
- The run directory is fully self-describing: a run resumes from that directory alone.
