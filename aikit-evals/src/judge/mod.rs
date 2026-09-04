//! The LLM judge tier (spec `eval-judge.md`, ADR 0021).
//!
//! A **judge** is a `[[judge]]` table in checks.toml: a rubric of criteria, a
//! prompt template and a model identity. Judging a trial renders the prompt
//! from the trial's artifacts, makes exactly one native chat completion (no
//! agent, no tools, nothing injected the author did not write), validates the
//! reply against a schema the engine derives from the rubric, and records the
//! whole exchange as a **judgment** in `trial-N/judgments.json`. The engine —
//! never the model — computes every score.
//!
//! The judgment then flattens into the trial's `result.json` as a
//! `judge:<name>` check row (gated or advisory), reduces into the case's
//! `aggregated.json`, and rolls up into `summary.json`. Everything is
//! additive (ADR 0020): a reader that predates the judge tier sees the same
//! fields it always did.
//!
//! Modules:
//! - [`config`]: the TOML shape, identity resolution (R3) and file-only
//!   validation (R14);
//! - [`template`]: `{{variable}}` scanning and rendering with the byte cap (R2);
//! - [`view`]: the trial view — final answer, tool calls, transcript,
//!   workspace diff, skill body — read from the run dir, erroring when the
//!   run dir cannot supply a variable;
//! - [`schema`]: the reply envelope schema, the rubric text and scoring (R5, R6);
//! - [`record`]: the `aikit.judgment/1` record and its hashes (R11);
//! - [`run`]: judging a run dir end to end, flattening and reduction (R8–R13).

pub mod config;
pub mod record;
pub mod run;
pub mod schema;
pub mod template;
pub mod view;

pub use config::{
    resolve_judges, validate_judges, Criterion, CriterionDefinition, CriterionKind, IssueLevel,
    JudgeDefaults, JudgeDefinition, JudgeIdentity, ResolvedJudge, ValidationIssue,
    DEFAULT_MAX_RETRIES, DEFAULT_MAX_TOKENS, DEFAULT_MAX_VAR_BYTES, DEFAULT_SCALE,
    DEFAULT_TEMPERATURE, DEFAULT_TIMEOUT_SECS,
};
pub use record::{
    append_judgment, cache_key, endpoint_host, judge_hash, latest_for, read_judgments,
    AttemptRecord, Judgment, JudgmentIdentity, JudgmentUsage, RecordError, JUDGMENT_SCHEMA,
};
pub use run::{
    judge_run_dir, JudgeError, JudgeOutcome, JudgeRunOptions, JudgeRunReport, SuitePassRule,
    TrialJudgeOutcome,
};
pub use schema::{output_contract, reply_schema, rubric_text, score_reply};
pub use template::{placeholders, render, Rendered, Scope, TemplateError};
pub use view::{TrialView, VarError, ViewError};
