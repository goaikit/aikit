# AIKIT Evals — Skill Evaluation

Running a target agent against cases, recording every trial as artifacts, and reducing what happened to outcomes, verdicts, and judgments. This context knows nothing about edits, epochs, or splits: the Optimization Loop consumes it through the Scorer, and `fastskill` consumes its artifacts on disk.

## Language

### Running

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

**Run directory**:
The directory one evaluation of a suite writes: the summary at its root, one directory per case, one per trial beneath, holding the trace, the outcome, and any judgments. The Optimization Loop's run directory is a different thing — the training run's — told apart by what sits at the root: a summary here, training state there.
_Avoid_: Output folder, results directory, sweep, run (alone — reserved, as the Trial entry says)

### Outcomes

**Trial outcome**:
The recorded verdict for one trial. `passed` — every required check passed. `failed` — the trial produced a valid measurement and at least one required check did not pass. `error` — **no valid measurement exists**, decided on transport and terminal signal rather than inferred from the content of the output: the run timed out, the agent could not be executed, the process exited non-zero, the agent's own **Terminal event** reported failure, or the stream ended with no terminal event on a backend that declares it emits one. An agent that exits cleanly having answered with nothing is `failed`, not `error` — that is a real skill failure and scores as one, and text absence is never the discriminator. `skipped` is reserved and currently never produced. The distinction that carries weight is `failed` versus `error`: over an empty or truncated trace a negative-expectation check and a tool-call ceiling both pass *vacuously*, so a run that never produced output must never reduce to a pass — and must not be averaged into a rate as though it were a wrong answer either. One type, `CaseStatus`, currently carries both this and the **Case verdict** below at four sites (`TrialResult`, `CaseResult`, `CaseSummary`, `CaseTrialsResult.aggregated_status`); the vocabulary separates them deliberately, and the shared type is recorded debt, not ratified design.
_Avoid_: Grade, verdict (reserved for the case level), score (a scalar in [0,1], not an outcome)

**Case verdict**:
The reduction of a case's trial outcomes to one status: the pass rate — passing trials over **scored** trials, where scored excludes those with outcome `error` or whose gated judge rendered no judgment — compared against a pass threshold the caller supplies. The count of excluded trials is recorded on the case rather than dropped, because the outage rate is itself a signal. A case whose every trial errored has no scored trials and takes the verdict `error`; it is excluded from the split score and reported by name, since silently dropping it would move the vacuity hazard up one level, where a total outage scores 100% over zero cases. This is the per-item result a gate metric reduces further to a split-level score.
_Avoid_: Case status (the field name — it does not say which level it means), aggregate, rollup

**Terminal event**:
The one frame in a trial's trace that carries the agent's own verdict on the run: an outcome (success or error), a machine-readable reason, an optional message, and the vendor-reported cost when the agent states one. Every backend puts this on the wire; a backend counts as emitting one only where its decoder actually produces the frame, which a capability flag declares. The **last** status-bearing terminal event decides the run, because a backend that emits one per turn may error and then recover. Cost is recorded only as the vendor reported it and is absent otherwise, never estimated from a local price table — a stale estimate is indistinguishable from a real number once it is written to an artifact.
_Avoid_: Final event, result line, exit status (that is the process's, not the agent's), completion

**Not observable**:
A check's third result, distinct from pass and fail: the evidence it reads cannot exist on the backend the trial ran against, so no verdict about the agent is available. A skill-invocation check and a tool-call ceiling both read decoded tool frames; on a backend whose decoder emits none, the first can only fail and the second can only pass, and neither is a measurement. A not-observable check is excluded from the suite's verdict rather than counted either way. It describes the decoder, never the agent, and it is not `skipped` — that outcome stays reserved.
_Avoid_: Skipped, N/A, inconclusive, unsupported (that is a scope's isolation fidelity)

### Judging

**Judge**:
A named, user-defined assessment of a trial by a model: a prompt template that renders the Trial view, a Rubric, and the model, endpoint, and sampling settings that produce the Judgment. That identity is part of the measurement — the same rubric under a different model is a different judge, and no reduction folds two identities into one number without saying so. A judge is one completion the engine composes in full: nothing reaches the model that the template and rubric did not put there, and a judge never runs an agent, so no harness can add to what it sees. A judge selects the cases it applies to the way a check does, and it is advisory unless it declares a minimum overall Score, at which point it gates like a required check.
_Avoid_: Grader (banned repo-wide), evaluator, LLM-as-judge (the technique, not the thing), scorer (the Optimization Loop's interface), critic, agent (a judge is a completion, never an agent run)

**Rubric**:
The ordered list of Criteria a Judge scores. A rubric says what is measured; the prompt template says how the judge is asked. Two judges may share one rubric — that is how a panel is formed.
_Avoid_: Checklist, scoring guide, criteria set, schema (the fixed envelope every judgment is written in)

**Criterion**:
One named question a Rubric asks of a trial. A `scale` criterion is answered with an integer on a declared range whose levels the rubric text describes; a `bool` criterion with yes or no. A criterion's raw answer is what the judge wrote; its Score is what the engine derives from it.
_Avoid_: Dimension, aspect, metric (a scorecard reduction, downstream), check (deterministic — no model involved)

**Judgment**:
What a Judge renders for one trial: for every criterion the raw answer and the judge's reasoning, an optional free-form note that is shown but never scored, and the identity, the tokens it consumed, and the inputs that produced it. Recorded once beside the trial's other artifacts and replayed thereafter — deriving scores, verdicts, and reports never calls the model again. A judgment is never a pass or fail by itself; a gate turns an overall Score into one.
_Avoid_: Verdict (reserved for the case level here and the metric level in fastskill), assessment, rating, evaluation, review

**Score**:
A scalar in [0, 1] the engine derives from a Judgment: one per criterion, normalized from its raw answer, and one overall — the unweighted mean of the criteria. Only judgments carry scores; a check yields pass, fail, or not observable. Across trials a case's score is the mean over its scored trials, and a `bool` criterion aggregates by majority. Nothing the judge asserts about an overall figure is recorded: the overall is computed, or it is absent.
_Avoid_: Grade, points, rating, raw answer (before normalization), pass rate (a rate of check results, not of scores)

**Trial view**:
The rendering of one trial that a Judge is shown and a report drills into: the case's prompt and its extra columns, the agent's final answer, its tool calls, the transcript, the change it made to its workspace, and the skill document under test. Assembled from the trace and artifacts after the trial, never from raw agent stdout, with every part capped in size and truncation made visible in the judgment. One view feeds every judge of a trial, so judges disagree about the same evidence.
_Avoid_: Context (overloaded), evidence, transcript (one part of it), trace (the raw source), prompt (the judge's rendered prompt contains the view; it is not the view)
