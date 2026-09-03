# A judgment is one native completion, recorded whole

## Status

accepted

## Context

The Skill Evaluation context gained a Judge: a user-defined assessment of a trial by a model, rendering a Judgment with a Score. Checks answer questions the engine can read off a trace; a judge answers the one they cannot, whether the answer was any good. The full specification lives with its first consumer, `gofastskill/fastskill`, as `docs/requirements/eval-judge.md`. This record covers the decisions in it that are hard to reverse and would puzzle a reader who was not in the room.

Three ways to run a judge were on the table:

1. **Through an agent harness** — `claude`, `codex`, `pi` — the way trials already run. The backends, decoders and isolation all exist, and every subscription-only CLI would be usable.
2. **Through aikit's in-process agent loop**, which already speaks to any OpenAI-compatible endpoint.
3. **As one raw completion** through `aikit_agent::llm::LlmGateway::complete`, with the engine composing every message.

The first two share a defect that the artifact cannot see. Every harness injects a system prompt, tools, hooks or hidden behaviour, and the loop runner is a smaller harness of its own: `build_system_instructions` prepends a persona and "You are a helpful AI agent. Complete the requested task carefully and accurately.", adds filesystem skills, and leaves temperature to the provider. A judgment produced that way scores the trial *and* the harness, under sampling nobody chose, and nothing recorded says which part of the number came from where. The user raised this concern in exactly those terms — as much control as possible over the interaction with the model — and it decided the question.

A second question followed. The judgment has to be written down somewhere, and `fastskill` reads it: its scorecard folds scores into metrics and its HTML report shows the reasoning. The moment a second repository reads the bytes, the shape is a contract with the same failure mode [ADR 0020](0020-eval-artifacts-are-an-additive-only-contract.md) describes for `result.json`: a renamed or removed field does not fail downstream, it deserialises to `None` and the number quietly moves.

## Decision

**A judge is one native completion.** Exactly one `LlmGateway::complete` call per judgment attempt, with the messages the engine rendered from the user's templates, the sampling the user declared, no tools and no streaming. A judge is never an agent run: there is no `agent` key, no `--judge-agent`, and no path from a judge to the loop runner or any backend. The cost is accepted: a subscription-only agent CLI cannot judge.

**The engine injects nothing.** A system message exists only when the judge declares one; the user message is the rendered prompt; a corrective retry is the model's rejected reply and the rendered retry prompt, appended as turns. The rubric and the reply schema reach the model only because the template says `{{rubric}}` and `{{output_contract}}`. Nothing the user did not write reaches the model, and `eval validate` refuses a prompt that omits the output contract rather than adding it silently.

**Every judgment is recorded whole.** `trial-N/judgments.json` is append-only. Each element carries `schema: "aikit.judgment/1"`, the judge's definition hash, a cache key over the hash and the rendered messages, the resolved identity — model asked for, model reported, endpoint host, sampling — and every attempt with the request body as sent, the bearer value redacted, and the raw reply. A judgment can be audited without the endpoint that produced it. The API key and the endpoint path are never written.

**The engine computes the score.** The model answers each criterion; the engine normalises scale answers to `[0, 1]`, maps bools to `1.0` and `0.0`, and takes the unweighted mean as `overall`. No model-reported total is ever recorded as a score.

**A gated judge that renders no judgment excludes the trial, and never says `not_observable`.** The trial keeps a `judge:<name>` check result with `score: None` and `passed: false`, gains `judge_excluded: true`, and leaves the case's scored trials; a case with none left has verdict `error`. `not_observable` stays reserved for a decoder that cannot produce a check's evidence. An advisory judge's error is recorded and moves nothing.

**`aikit.judgment/1`, `CheckResult.score`, `judge_excluded`, `judge_excluded_count`, per-case `scores` and the judge totals are additive-only under ADR 0020.** They are added with `#[serde(default)]`, absent means not recorded, and removing any of them is the coordinated act that ADR describes. The `schema` string is the envelope's version; a shape that must change gets `aikit.judgment/2` beside it, and `/1` keeps being written for as long as any consumer reads it.

## Consequences

- `aikit-sdk` re-exports the gateway module so `aikit-evals` reaches `LlmGateway` without a new dependency edge; `aikit_sdk::Pipeline` is generalised from a single prompt to a message list and stays blocking.
- `LlmResponse` gains an optional `model` field, `EvalCase` gains `extra` for spare CSV columns, and `trace.jsonl`'s message payload gains `kind` and `phase`, so a judge can find the final answer once instead of twice. All additive.
- Every trial writes `workspace.diff` before its scratch workspace is discarded, not only the failed ones a retained workspace already covers. A judge asked for evidence a run directory predates errors by variable name rather than rendering a guess.
- The gateway's silent default to `api.openai.com` does not apply to judges. A judge with no resolvable endpoint or key fails before the first trial runs, so a paid run is never followed by an unjudgeable result.
- Judge cost is tokens. `cost_usd` on a judgment appears only when a gateway reports it, following ADR 0020's rule that an artifact never carries an estimate.
- `judgments.json` will grow: every retry attempt with its full request body is kept, and `--rejudge` appends rather than replaces. That is the intended cost of a record that can be audited without re-running it.
- Reduction across trials names what it averaged: per-case `scores` carry `judged_trials` beside each mean, and `fastskill` compares judge hashes before folding two judgments into one metric. Two identities are never one number without a flag that says so.
