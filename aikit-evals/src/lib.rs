//! Evaluation runner infrastructure for aikit agents.
//!
//! Provides generic eval suite loading, case execution via aikit-sdk, deterministic
//! check scoring, artifact persistence, and trace conversion.
//!
//! # Re-exported items
//! All public types are available at the crate root.

pub mod artifacts;
pub mod checks;
pub mod config;
pub mod runner;
pub mod scoring;
pub mod suite;
pub mod trace;

pub use artifacts::{
    allocate_run_dir, read_case_results, read_summary, write_case_artifacts,
    write_case_trials_summary, write_summary, write_trial_artifacts, ArtifactsError, CaseResult,
    CaseStatus, CaseSummary, CaseTrialsResult, RunArtifacts, SummaryResult, TrialResult,
};
pub use checks::{
    count_raw_json_events, load_checks, run_checks, suite_passes, CheckDefinition, CheckResult,
    ChecksError, ChecksToml,
};
pub use config::{resolve_from_input, EvalConfig, EvalConfigError, EvalConfigInput};
pub use runner::{
    run_eval_case, AikitEvalRunner, CaseRunOptions, CaseRunOutput, EvalRunner, RunnerError,
};
pub use scoring::{item_score, score_cases, split_score, ChecksScorer, GateMetric, Scorer};
pub use suite::{load_suite, EvalCase, EvalSuite, SuiteError};
pub use trace::{agent_events_to_trace, stdout_to_trace, trace_to_jsonl, TraceEvent, TracePayload};
