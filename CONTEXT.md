# AIKIT

Iterative optimization of a skill document by running scored agent trajectories through an optimizer agent that proposes bounded edits, accepted only when they improve a held-out validation score.

## Crate responsibilities

- **aikit-evals** — pure evaluation: run a target against a case, capture the trajectory, score it (`Scorer`), and read/write eval artifacts. Knows nothing about edits, epochs, or splits.
- **aikit-textgrad** — the artifact-agnostic text-gradient algorithm in two reusable layers: (1) the **edit substrate** — a public, model-free module (`Edit`/`Patch`/`apply_patch`, anchor resolution, dry-run/backfill, protected-region enforcement, skip reporting) usable to safely apply agent-proposed edits to any text; and (2) the **optimization loop** on top (epoch/step, textual learning rate, rejected-edit buffer, slow update, meta-skill, monotonic-best, training-run resume), which calls evals to roll out and gate. A clearly-bounded public module, not a separate crate — no non-textgrad consumer needs it yet. Generic prompt *optimization* (a non-skill `Optimizable`) is a future sibling crate, not built now.
- **aikit-skillopt** — a particular application of textgrad where the trainable artifact is a skill document (`SKILL.md` via `deploy_skill`); supplies the `Optimizable` impl and skill-flavored optimizer prompts.

All three are **library crates only** — no CLI in goaikit/aikit. The user-facing feature is exposed by `fastskill` (separate workspace: `fastskill-cli`), which consumes these via git dependency exactly as it already consumes `aikit-evals` and `aikit-sdk`.

## Language

**Skill document**:
The sole mutable text artifact in a training run, seeded from an existing `SKILL.md` (the required input — cold-start from empty is not built). Delivered to an agent by being written into the agent's workspace as a skill file (`deploy_skill` → `.../skills/<name>/SKILL.md`), which the agent reads during its run — never injected into the model's system prompt. The same file is the deployed artifact. Protected-region sentinels are injected empty at run start if absent, reused if the seed already carries them.
_Avoid_: Prompt, system prompt, context, template

**Rollout workspace**:
A fresh, disposable working directory created per rollout: the case's starting fixtures (`workspace_subdir`) are copied in if present — otherwise it starts empty — then the current skill document is deployed into it as `SKILL.md` before the target agent is launched. Discarded after scoring. Isolation makes parallel rollouts collision-free.
_Avoid_: Project root (that's the immutable fixture source), sandbox

**Starting fixtures**:
The files a case requires to exist before the target agent runs, stored under the case's `workspace_subdir` and copied into each rollout workspace. A case with no fixtures yields an empty workspace holding only the deployed skill.
_Avoid_: Seed, setup data

**Protected region**:
A single sentinel-delimited block (`<!-- SKILLOPT:PROTECTED:BEGIN -->` / `:END`) pinned at the tail of the skill document, holding slow-update guidance. Step-level edits resolving at or after the begin sentinel are skipped and logged; `append` inserts immediately before it. HTML-comment sentinels survive arbitrary edits to the editable portion and render invisibly in the deployed file.
_Avoid_: Locked section, frozen block, header

**Slow update**:
The epoch-boundary pass that consolidates the protected region: the optimizer is given the current protected content plus this epoch's rejected-edit buffer and produces a *revised* replacement — merging, reconciling contradictions, dropping stale guidance, keeping what still holds (never blind append or overwrite). A soft size cap is passed as an instruction, not a hard truncation, so guidance is never severed mid-sentence. Acceptance is force-accept or gated (the whole revised region evaluated on the selection split) per config.
_Avoid_: Distillation, epoch merge, consolidation pass

**Meta-skill**:
The epoch-boundary second-order pass: the optimizer reflects on the full training history and *revises* the **strategy section** of its own Reflect/Aggregate prompts for the next epoch (consolidate, like slow update). It may touch only the strategy/heuristics text — never the **fixed scaffold** (JSON schema + output-format instructions that pin `Pipeline` validation). Force-accept (no per-epoch gate is possible without running the epoch); the validation gate is the backstop, so a bad meta-update can only waste an epoch, never regress `best_skill`. Versioned per epoch for audit; no auto-rollback in v1.
_Avoid_: Meta reflection, optimizer tuning

**Optimizer strategy**:
The mutable heuristics section of the optimizer's Reflect/Aggregate prompts (e.g. "prioritize edits addressing X failure mode"), revised each epoch by meta-skill and carried as per-epoch training-run state. Distinct from the immutable **scaffold** that defines the structured-output contract.
_Avoid_: Optimizer prompt (ambiguous — say strategy vs scaffold)

**Split**:
The role a case plays in a training run, declared explicitly via a `split` column: `train` (sampled for rollouts), `selection` (the held-out set the gate scores candidates against), or `test` (held out entirely; scored only for end-of-run reporting). A missing value defaults to `train`; a run with zero `selection` cases is a config-time error.
_Avoid_: Fold, partition, bucket

**Target agent**:
The runnable agent that executes tasks using the current skill document. Frozen during training — only the skill document changes.
_Avoid_: M_target, target model, inference agent

**Optimizer agent**:
A runnable agent (any key: claude, codex, gemini, etc.) that receives scored trajectories and proposes edits to the skill document. Never in the deployment path.
_Avoid_: M_opt, optimizer model, training model

**Step**:
The unit between gate decisions and the checkpoint unit for resume. One step rolls out **B×A** tasks (batch size × accumulation), Reflects each trajectory into a patch, then runs a *single* Aggregate → Select → Update → Gate cycle over the whole pooled set. The gate fires exactly once per step.
_Avoid_: Iteration, batch (a batch is one rollout group within a step)

**Accumulation**:
The count `A` of rollout batches whose patches are pooled before one update cycle — the textual analog of gradient accumulation. `A=1` means one step is one rollout batch; `A>1` widens the textual gradient (more trajectories per update) and reduces gate frequency (fewer selection-split evaluations). Drives `steps_per_epoch = ceil(train_size / (B×A))`.
_Avoid_: Grad accumulation, batching factor

**Epoch sweep**:
Each epoch covers the training split exactly once: `steps_per_epoch = ceil(train_size / (B×A))`, sampled without replacement and reshuffled per epoch. Makes the cosine learning-rate schedule's "over N epochs" well-defined against real data passes.
_Avoid_: Pass, sampling round

**Training run**:
One complete execution of the outer epoch/step loop, producing a `best_skill.md` artifact. Its entire state lives in a self-describing **run directory** so it can be resumed or moved.
_Avoid_: Job, session, experiment

**Run directory**:
The self-contained, portable directory holding everything a training run produces: versioned skill snapshots, `best_skill.md`, `runtime_state.json`, per-step records, and epoch artifacts. A run resumes from this directory alone.
_Avoid_: Output folder, workdir

**Rejected-edit buffer**:
Epoch-scoped durable state accumulating each gate-rejected candidate patch with the skill state and score delta at rejection (R10.1) — strategic signal only ("this direction didn't improve validation"). Consumed and cleared by the epoch-boundary slow update. Holds *only* gate-rejected patches, never mechanical anchor skips (those go to the skip-feedback note). Persisted at every step end (or rebuilt from on-disk trajectory digests on resume) — never in-memory-only, since losing it is a resume data-loss violation that silently weakens slow-update guidance.
_Avoid_: Step buffer, reject queue

**Skip-feedback note**:
A short, single-step record of intra-patch anchor misses ("edit `replace('foo…')` was dropped — anchor already consumed by a prior edit / not found"). Mechanical signal, threaded into the *next* step's Reflect prompt so the optimizer re-proposes with a valid anchor. Lives one step, then discarded — never accumulates, never reaches slow update. Kept separate from the rejected-edit buffer so the strategic signal stays clean.
_Avoid_: Skip log, reject buffer (different consumer and lifetime)

**Textual learning rate**:
An integer budget on the number of edits *effectively applied* per step (not merely proposed), cosine-decayed over epochs. Counting applied edits is what makes the schedule a real stability control — a patch with three stale anchors and one valid edit has a true step size of 1 regardless of how many edits were proposed.
_Avoid_: LR (spell it out in prose), edit count, step size

**Dry-run apply**:
A pure string-operation simulation of a ranked patch (zero model calls) run between Select and Update: edits are applied in declaration order; on the first skip, that edit is dropped, the next-highest-impact candidate is pulled from the ranked pool, and simulation resumes — repeating until the textual learning rate is filled with effective edits or the pool is dry. Protected-region hits are hard drops, never backfilled.
_Avoid_: Preview, validation pass

**Patch**:
An ordered list of edit operations proposed by the optimizer agent for one step. Applied sequentially; order matters.
_Avoid_: Diff, changeset, update

**Edit**:
An atomic operation on the skill document: one of append, insert-after-anchor, replace, or delete.
_Avoid_: Change, mutation, delta

**Optimizer stage**:
Any step that invokes the optimizer agent to produce schema-validated structured output (Reflect, Aggregate, Slow Update, Meta-skill). Each runs through `aikit-sdk::Pipeline` with `max_retries(2)`, validating against a JSON Schema and retrying with an augmented corrective prompt on failure. On exhaustion the stage degrades to a no-op (never aborts the run), which is safe under the monotonic-best invariant.
_Avoid_: Optimizer call, LLM call

**Optimizable**:
The seam between `aikit-textgrad` and a concrete trainable artifact: a trait exposing the artifact's current text and a way to materialize it into a rollout workspace. `aikit-skillopt` implements it for skill documents (materialize = `deploy_skill` → `SKILL.md`). textgrad never names "skill" — everything artifact-specific lives behind this trait.
_Avoid_: Target, trainable, document interface

**Scorer**:
The reward function of a benchmark environment: maps one trajectory to a result the gate can reduce to a scalar in [0,1]. An interface — the built-in `ChecksScorer` derives the scalar from the deterministic checks engine. New benchmarks supply their own scorer without touching the training loop.
_Avoid_: Grader, evaluator (that's the run harness), reward model

**Tool call**:
One structured invocation the target agent made during a rollout, decoded from its output as a `ToolUse` frame and recorded in the trace. What the `max_tool_calls` check counts, alongside `raw_json` lines for backends that still emit tool calls as raw JSON. An agent's text, reasoning and token-usage events are **not** tool calls — counting prose as activity is the specific bug this vocabulary exists to prevent (see [ADR 0019](docs/adr/0019-codex-decode-emits-typed-tool-frames.md)). The eval artifact field is still named `command_count`, deliberately: renaming a config knob does not justify breaking artifact readers.
_Avoid_: Command (it is not necessarily a shell command), action, step

**Case**:
The unit of evaluation: one prompt, together with the starting fixtures (`workspace_subdir`) and tags that travel with it, under a stable `id`. A case declares *what to ask*, and one thing about what counts as success: its `should_trigger` column, which asserts whether the skill is expected to be invoked at all. Every other assertion is a check, configured separately. `should_trigger` generates a skill-invocation check with matching polarity, so a case marked `false` asserts that the skill stayed out of it; a case whose explicit checks contradict the column is rejected rather than silently resolved one way.
_Avoid_: Test, test case, task, item, sample, prompt (that is one field of a case)

**Trial**:
One execution of one case — a fresh rollout workspace, one **Agent run** (see `docs/GLOSSARY.md`), one trace, one set of check results — and the recorded unit a scorer reads. Cases are executed over N trials to damp target-agent nondeterminism (`run_case_trials`); a single trial cannot distinguish reliable behaviour from a lucky sample.
_Avoid_: Run (reserved for the training run, the run directory, and the SDK's agent run), attempt, repetition, sample

**Check**:
One deterministic assertion evaluated against a trial's canonical trace after execution: substring presence or absence (`trigger_expectation`, `command_contains`), a structured skill invocation (`skill_invoked`), a file's existence (`file_exists`), or a ceiling on tool calls (`max_tool_calls`). Checks read the trace only, never raw agent stdout — an agent's startup capability listing must not satisfy an assertion that the skill ran. A check is `required`, meaning its failure fails the trial, or advisory, and it applies to every case in its suite unless it names the cases it targets. Because the trace echoes every file the agent read, a pattern that also occurs in the skill document matches whenever the agent merely opens it, and proves nothing about the answer.
_Avoid_: Oracle, assertion, test, validator, rule

**Suite**:
The set of cases loaded for one evaluation, carrying their split roles. A suite holds cases and nothing else; checks are configured separately and default to applying to every case in it. A check may name the cases it targets instead, so two cases needing different assertions can share a suite rather than forcing one suite per assertion.
_Avoid_: Benchmark (that is the environment: cases plus a scorer), test suite, collection

**Trial outcome**:
The recorded verdict for one trial. `passed` — every required check passed. `failed` — the trial produced a valid measurement and at least one required check did not pass. `error` — **no valid measurement exists**, decided on transport and terminal signal rather than inferred from the content of the output: the run timed out, the agent could not be executed, the process exited non-zero, the agent's own **Terminal event** reported failure, or the stream ended with no terminal event on a backend that declares it emits one. An agent that exits cleanly having answered with nothing is `failed`, not `error` — that is a real skill failure and scores as one, and text absence is never the discriminator. `skipped` is reserved and currently never produced. The distinction that carries weight is `failed` versus `error`: over an empty or truncated trace a negative-expectation check and a tool-call ceiling both pass *vacuously*, so a run that never produced output must never reduce to a pass — and must not be averaged into a rate as though it were a wrong answer either. One type, `CaseStatus`, currently carries both this and the **Case verdict** below at four sites (`TrialResult`, `CaseResult`, `CaseSummary`, `CaseTrialsResult.aggregated_status`); the vocabulary separates them deliberately, and the shared type is recorded debt, not ratified design.
_Avoid_: Grade, verdict (reserved for the case level), score (a scalar in [0,1], not an outcome)

**Case verdict**:
The reduction of a case's trial outcomes to one status: the pass rate — passing trials over **scored** trials, where scored excludes those with outcome `error` — compared against a pass threshold the caller supplies. The count of excluded trials is recorded on the case rather than dropped, because the outage rate is itself a signal. A case whose every trial errored has no scored trials and takes the verdict `error`; it is excluded from the split score and reported by name, since silently dropping it would move the vacuity hazard up one level, where a total outage scores 100% over zero cases. This is the per-item result a gate metric reduces further to a split-level score.
_Avoid_: Case status (the field name — it does not say which level it means), aggregate, rollup

**Terminal event**:
The one frame in a trial's trace that carries the agent's own verdict on the run: an outcome (success or error), a machine-readable reason, an optional message, and the vendor-reported cost when the agent states one. Every backend puts this on the wire; a backend counts as emitting one only where its decoder actually produces the frame, which a capability flag declares. The **last** status-bearing terminal event decides the run, because a backend that emits one per turn may error and then recover. Cost is recorded only as the vendor reported it and is absent otherwise, never estimated from a local price table — a stale estimate is indistinguishable from a real number once it is written to an artifact.
_Avoid_: Final event, result line, exit status (that is the process's, not the agent's), completion

**Not observable**:
A check's third result, distinct from pass and fail: the evidence it reads cannot exist on the backend the trial ran against, so no verdict about the agent is available. A skill-invocation check and a tool-call ceiling both read decoded tool frames; on a backend whose decoder emits none, the first can only fail and the second can only pass, and neither is a measurement. A not-observable check is excluded from the suite's verdict rather than counted either way. It describes the decoder, never the agent, and it is not `skipped` — that outcome stays reserved.
_Avoid_: Skipped, N/A, inconclusive, unsupported (that is a scope's isolation fidelity)

**Gate metric**:
How a scorer's per-item results are reduced to a split-level score: `hard` (per-item full-pass → 1/0, averaged = accuracy), `soft` (per-item fraction of required checks passed, averaged), or `mixed` (weighted combination). Selectable at runtime.
_Avoid_: Reward, score mode

**Validation gate**:
The accept/reject decision: the candidate skill is scored on the selection split over N trials (mean, to damp the target agent's nondeterminism — `run_case_trials`) and accepted only if its score exceeds the cached best score by a configurable epsilon margin (`score(S') > score(S) + ε`). The best score is the value recorded when best was accepted — cached, never re-measured — so the monotonic-best invariant holds by construction and cheaply; trials and epsilon attack fluke acceptance at the source. Training rollouts stay single-trial (noise matters less in Reflect); trials are spent only at the gate.
_Avoid_: Validation, acceptance test, eval gate
