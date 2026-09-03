//! Artifact layout and persistence for eval runs

use crate::checks::CheckResult;
use aikit_sdk::TerminalOutcome;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Status of a single eval case
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CaseStatus {
    Passed,
    Failed,
    Error,
    Skipped,
}

impl std::fmt::Display for CaseStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaseStatus::Passed => write!(f, "passed"),
            CaseStatus::Failed => write!(f, "failed"),
            CaseStatus::Error => write!(f, "error"),
            CaseStatus::Skipped => write!(f, "skipped"),
        }
    }
}

/// The agent's own report of how the run ended, as recorded in the artifact.
///
/// Present only when the backend's decoder emits a terminal frame. Absent means
/// "not recorded", never "succeeded" — see [`aikit_sdk::BackendCapabilities`]'s
/// `terminal_event` flag for which backends can supply it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerminalRecord {
    pub outcome: TerminalOutcome,
    /// Machine-readable reason from the agent (`stop_reason`, `subtype`,
    /// `stopReason`, or the event name for codex).
    #[serde(default)]
    pub reason: Option<String>,
    /// Human-readable failure text, when the agent supplied one.
    #[serde(default)]
    pub message: Option<String>,
}

/// Numbers the runner already receives and previously narrowed away.
///
/// Every field is `Option` and `#[serde(default)]`: absent means this version
/// or this backend did not record it, never zero (ADR 0020).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TokenBreakdown {
    #[serde(default)]
    pub total_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
}

impl TokenBreakdown {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// Per-case result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseResult {
    pub id: String,
    pub status: CaseStatus,
    pub command_count: Option<usize>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub check_results: Vec<CheckResult>,
    pub error_message: Option<String>,
    /// Process exit code, captured by the runner since Phase 0 and never
    /// persisted until now. `None` when the run never reached a process exit.
    #[serde(default)]
    pub exit_code: Option<i32>,
    /// The agent's own terminal report, when its decoder emits one.
    #[serde(default)]
    pub terminal: Option<TerminalRecord>,
    /// Vendor-reported cost, summed over terminal frames. **Never estimated**
    /// from a local price table: `None` means the backend reported nothing.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// Cache and reasoning token counts the runner receives alongside the
    /// input/output pair above.
    #[serde(default)]
    pub tokens: TokenBreakdown,
    /// Where the skill document was staged for **this trial**, so an offline
    /// re-score can rebuild the same [`CheckContext`](crate::checks::CheckContext)
    /// the run used.
    ///
    /// Per trial, not per run: under isolation every trial stages into its own
    /// scratch directory, so a single run-level path would name one arbitrary
    /// trial's temp dir and fail to match every other trial's trace.
    ///
    /// Deliberately its own field and not read off [`IsolationReport`], which
    /// is report-only: nothing in that struct may reach a `CheckResult`. This
    /// path is a fact the runner recorded about what it staged, not something
    /// the agent reported about itself.
    ///
    /// `None` means "this version did not record it", never "no skill was
    /// staged".
    #[serde(default)]
    pub skill_path: Option<PathBuf>,
}

/// Per-trial result for a case
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialResult {
    pub trial_id: u32,
    pub status: CaseStatus,
    pub command_count: Option<usize>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub check_results: Vec<CheckResult>,
    pub error_message: Option<String>,
    /// Process exit code, captured by the runner since Phase 0 and never
    /// persisted until now. `None` when the run never reached a process exit.
    #[serde(default)]
    pub exit_code: Option<i32>,
    /// The agent's own terminal report, when its decoder emits one.
    #[serde(default)]
    pub terminal: Option<TerminalRecord>,
    /// Vendor-reported cost, summed over terminal frames. **Never estimated**
    /// from a local price table: `None` means the backend reported nothing.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// Cache and reasoning token counts the runner receives alongside the
    /// input/output pair above.
    #[serde(default)]
    pub tokens: TokenBreakdown,
    /// Where the skill document was staged for **this trial**, so an offline
    /// re-score can rebuild the same [`CheckContext`](crate::checks::CheckContext)
    /// the run used.
    ///
    /// Per trial, not per run: under isolation every trial stages into its own
    /// scratch directory, so a single run-level path would name one arbitrary
    /// trial's temp dir and fail to match every other trial's trace.
    ///
    /// Deliberately its own field and not read off [`IsolationReport`], which
    /// is report-only: nothing in that struct may reach a `CheckResult`. This
    /// path is a fact the runner recorded about what it staged, not something
    /// the agent reported about itself.
    ///
    /// `None` means "this version did not record it", never "no skill was
    /// staged".
    #[serde(default)]
    pub skill_path: Option<PathBuf>,
}

/// Aggregated results for a case across multiple trials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseTrialsResult {
    pub id: String,
    pub trials: Vec<TrialResult>,
    pub aggregated_status: CaseStatus,
    pub pass_count: u32,
    pub total_trials: u32,
    /// Passing trials over **scored** trials. Trials with outcome `error`
    /// produced no measurement and are excluded from both sides of the ratio.
    pub pass_rate: f64,
    /// Trials excluded from `pass_rate` because they errored. Recorded, never
    /// silently dropped: the outage rate is itself a result.
    #[serde(default)]
    pub error_count: u32,
    /// `total_trials - error_count`, i.e. the denominator of `pass_rate`.
    /// Zero means the case produced no measurement at all and takes the
    /// verdict `CaseStatus::Error`.
    #[serde(default)]
    pub scored_trials: u32,
}

/// Fold a case's trials into its verdict.
///
/// One implementation, called by the real runner and by every test double, so
/// the rate cannot drift between them.
///
/// - `error` trials are excluded from both sides of `pass_rate`.
/// - A case with no scored trials left is `CaseStatus::Error`, not a 0% fail:
///   a total outage must not read as a case the agent got wrong, and must not
///   quietly vanish from the denominator one level up either.
pub fn aggregate_trials(
    case_id: &str,
    mut trials: Vec<TrialResult>,
    total_trials: u32,
    pass_threshold: f64,
) -> CaseTrialsResult {
    trials.sort_by_key(|t| t.trial_id);
    let error_count = trials
        .iter()
        .filter(|t| t.status == CaseStatus::Error)
        .count() as u32;
    let pass_count = trials
        .iter()
        .filter(|t| t.status == CaseStatus::Passed)
        .count() as u32;
    let total_trials = total_trials.max(1);
    let scored_trials = total_trials.saturating_sub(error_count);
    let pass_rate = if scored_trials == 0 {
        0.0
    } else {
        pass_count as f64 / scored_trials as f64
    };
    let aggregated_status = if scored_trials == 0 {
        CaseStatus::Error
    } else if pass_rate >= pass_threshold {
        CaseStatus::Passed
    } else {
        CaseStatus::Failed
    };

    CaseTrialsResult {
        id: case_id.to_string(),
        trials,
        aggregated_status,
        pass_count,
        total_trials,
        pass_rate,
        error_count,
        scored_trials,
    }
}

/// Achieved isolation fidelity for one scope (spec 016 D6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeFidelity {
    /// The scope was actually isolated for this run.
    Isolated,
    /// The scope ran against the ambient environment (legacy behaviour, or a
    /// recorded degradation — see [`IsolationReport::degrade_reason`]).
    Inherited,
    /// The backend has no mechanism for this scope; the run proceeded anyway
    /// and says so honestly (spec 016 D4).
    Unsupported,
}

/// What environment a run *actually* got, per scope (spec 016 D6).
///
/// **Report-only.** Every field here — `ambient_skills` in particular — is
/// evidence of *environment*, never evidence of *invocation*: the agent's
/// capability listing produced false passes when it fed scoring (spec 015),
/// so nothing in this struct may ever feed a `CheckResult` or otherwise
/// influence pass/fail. Do not "fix" this back into the spec-015 bug.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IsolationReport {
    /// Was `IsolationMode::Isolated` asked for?
    pub requested: bool,
    /// Project scope (`<cwd>/.claude/skills`, `CLAUDE.md`, `.mcp.json`, …).
    pub project_scope: ScopeFidelity,
    /// User scope (`~/.claude/skills`, plugins, user settings, …).
    pub user_scope: ScopeFidelity,
    /// The per-backend mechanism used (e.g. `--setting-sources project`).
    pub mechanism: Option<String>,
    /// Agent version, when observable from the run output.
    pub agent_version: Option<String>,
    /// Skills the agent reported loading (best-effort, parsed from claude's
    /// `system`/`init` event). An empty vec means "not observable on this
    /// backend" and must be rendered as such — never as "nothing was loaded".
    #[serde(default)]
    pub ambient_skills: Vec<String>,
    /// Root of the scratch workspace the case ran in, when isolated.
    pub workspace_root: Option<PathBuf>,
    /// Why isolation degraded below what was requested (e.g. opencode has no
    /// skills path in the deploy catalog). `None` when nothing degraded.
    /// Additive to the spec-016 D6 shape so a degraded run never *silently*
    /// claims isolation (D4).
    #[serde(default)]
    pub degrade_reason: Option<String>,
}

/// Aggregated run summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryResult {
    pub suite_pass: bool,
    #[serde(default)]
    pub suite_pass_rate: Option<f64>,
    pub agent: String,
    pub model: Option<String>,
    pub total_cases: usize,
    pub passed: usize,
    pub failed: usize,
    #[serde(default)]
    pub trials_per_case: Option<u32>,
    #[serde(default)]
    pub parallel: Option<u32>,
    #[serde(default)]
    pub pass_threshold: Option<f64>,
    pub run_dir: PathBuf,
    pub checks_path: Option<PathBuf>,
    pub skill_project_root: PathBuf,
    /// Environment contract for the run (spec 016 D6). Additive: pre-016
    /// summaries deserialize as `None`, which renders as "unknown" — never as
    /// "not isolated".
    #[serde(default)]
    pub isolation: Option<IsolationReport>,
    pub cases: Vec<CaseSummary>,
}

/// Per-case summary entry in summary.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseSummary {
    pub id: String,
    pub status: CaseStatus,
    pub command_count: Option<usize>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub pass_count: Option<u32>,
    #[serde(default)]
    pub total_trials: Option<u32>,
    #[serde(default)]
    pub pass_rate: Option<f64>,
    /// Trials excluded from `pass_rate` because they errored.
    #[serde(default)]
    pub error_count: Option<u32>,
    /// Denominator of `pass_rate`. `Some(0)` means the case has no measurement.
    #[serde(default)]
    pub scored_trials: Option<u32>,
    /// The case's `should_trigger` column, recorded so an offline re-score can
    /// rebuild the same effective check list the run scored against.
    ///
    /// Under R7 this column generates an implicit skill-invocation check, so a
    /// scorer that cannot see it silently drops that check and reports a
    /// different verdict than the run did.
    ///
    /// `None` means "this version did not record it", never `false`. A scorer
    /// reading `None` must fall back to the explicit checks alone, which is
    /// what pre-R7 artifacts were scored with.
    #[serde(default)]
    pub should_trigger: Option<bool>,
    #[serde(default)]
    pub trials: Vec<TrialResult>,
}

/// All artifacts from a completed run
#[derive(Debug)]
pub struct RunArtifacts {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub summary: SummaryResult,
    pub case_results: Vec<CaseResult>,
}

/// Errors during artifact writing/reading
#[derive(Debug, Error)]
pub enum ArtifactsError {
    #[error("EVAL_ARTIFACTS_CORRUPT: IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("EVAL_ARTIFACTS_CORRUPT: JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("EVAL_ARTIFACTS_CORRUPT: Missing required field: {0}")]
    MissingField(String),
    #[error("EVAL_RUN_DIR_EXHAUSTED: no free run directory for '{0}' after 999 suffix attempts")]
    RunDirExhausted(String),
}

/// Allocate a run directory under output_dir using ISO 8601 timestamp format
/// Appends numeric suffix if directory already exists
pub fn allocate_run_dir(output_dir: &Path, run_id: &str) -> Result<PathBuf, ArtifactsError> {
    let base = output_dir.join(run_id);
    if !base.exists() {
        std::fs::create_dir_all(&base)?;
        return Ok(base);
    }

    // Append numeric suffix
    for i in 2..=999 {
        let candidate = output_dir.join(format!("{}-{}", run_id, i));
        if !candidate.exists() {
            std::fs::create_dir_all(&candidate)?;
            return Ok(candidate);
        }
    }

    // All suffixes taken: error out rather than silently reusing (and
    // overwriting) the existing base directory.
    Err(ArtifactsError::RunDirExhausted(run_id.to_string()))
}

/// Everything one trial leaves on disk besides its directory name.
///
/// `workspace_diff` is `Some` for every trial that ran in a seeded scratch
/// workspace, an empty string when the agent changed nothing, and `None` when
/// there was no seeded state to diff against (inherited or degraded
/// environment). In the `None` case `workspace.diff` is deliberately not
/// written (spec eval-judge R10): a reader then sees "no evidence" rather
/// than "no change".
#[derive(Debug, Clone, Copy)]
pub struct TrialArtifacts<'a> {
    pub stdout: &'a [u8],
    pub stderr: &'a [u8],
    pub trace_jsonl: &'a str,
    pub workspace_diff: Option<&'a str>,
    pub result: &'a TrialResult,
}

/// Write per-trial artifacts (stdout.txt, stderr.txt, trace.jsonl,
/// workspace.diff, result.json) under `{run_dir}/{case_id}/trial-{trial_id}/`.
pub fn write_trial_artifacts(
    run_dir: &Path,
    case_id: &str,
    trial_id: u32,
    artifacts: &TrialArtifacts<'_>,
) -> Result<PathBuf, ArtifactsError> {
    let trial_dir = run_dir.join(case_id).join(format!("trial-{}", trial_id));
    std::fs::create_dir_all(&trial_dir)?;

    std::fs::write(trial_dir.join("stdout.txt"), artifacts.stdout)?;
    std::fs::write(trial_dir.join("stderr.txt"), artifacts.stderr)?;
    std::fs::write(trial_dir.join("trace.jsonl"), artifacts.trace_jsonl)?;
    if let Some(diff) = artifacts.workspace_diff {
        std::fs::write(trial_dir.join("workspace.diff"), diff)?;
    }

    let result_json = serde_json::to_string_pretty(artifacts.result)?;
    std::fs::write(trial_dir.join("result.json"), result_json)?;

    Ok(trial_dir)
}

/// Write `{run_dir}/{case_id}/aggregated.json`
pub fn write_case_trials_summary(
    run_dir: &Path,
    case_id: &str,
    trials_result: &CaseTrialsResult,
) -> Result<(), ArtifactsError> {
    let case_dir = run_dir.join(case_id);
    std::fs::create_dir_all(&case_dir)?;
    let aggregated_json = serde_json::to_string_pretty(trials_result)?;
    std::fs::write(case_dir.join("aggregated.json"), aggregated_json)?;
    Ok(())
}

fn case_result_to_trial(case: &CaseResult, trial_id: u32) -> TrialResult {
    TrialResult {
        trial_id,
        status: case.status.clone(),
        command_count: case.command_count,
        input_tokens: case.input_tokens,
        output_tokens: case.output_tokens,
        check_results: case.check_results.clone(),
        error_message: case.error_message.clone(),
        exit_code: case.exit_code,
        terminal: case.terminal.clone(),
        cost_usd: case.cost_usd,
        tokens: case.tokens.clone(),
        skill_path: case.skill_path.clone(),
    }
}

/// Write per-case artifacts for backwards-compatible callers.
///
/// Artifacts are written as trial 1 under `{run_dir}/{case_id}/trial-1/`, and an
/// aggregated `{run_dir}/{case_id}/aggregated.json` is also created.
pub fn write_case_artifacts(
    run_dir: &Path,
    case_id: &str,
    stdout: &[u8],
    stderr: &[u8],
    trace_jsonl: &str,
    workspace_diff: Option<&str>,
    result: &CaseResult,
) -> Result<PathBuf, ArtifactsError> {
    let trial = case_result_to_trial(result, 1);
    let trial_dir = write_trial_artifacts(
        run_dir,
        case_id,
        1,
        &TrialArtifacts {
            stdout,
            stderr,
            trace_jsonl,
            workspace_diff,
            result: &trial,
        },
    )?;

    // One trial, folded by the same rule as any other case (`aggregate_trials`):
    // a single errored trial leaves zero scored trials, so the case is `error`,
    // not a 0% failure.
    let aggregated = aggregate_trials(&result.id, vec![trial], 1, 1.0);
    write_case_trials_summary(run_dir, case_id, &aggregated)?;

    Ok(trial_dir)
}

/// Write summary.json
pub fn write_summary(run_dir: &Path, summary: &SummaryResult) -> Result<(), ArtifactsError> {
    let summary_json = serde_json::to_string_pretty(summary)?;
    std::fs::write(run_dir.join("summary.json"), summary_json)?;
    Ok(())
}

/// Read summary.json from a run directory
pub fn read_summary(run_dir: &Path) -> Result<SummaryResult, ArtifactsError> {
    let summary_path = run_dir.join("summary.json");
    let content = std::fs::read_to_string(&summary_path)?;
    let summary: SummaryResult = serde_json::from_str(&content)?;
    Ok(summary)
}

/// Read case result.json files from a run directory
pub fn read_case_results(run_dir: &Path) -> Result<Vec<CaseResult>, ArtifactsError> {
    let mut results = Vec::new();

    let entries = std::fs::read_dir(run_dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let aggregated_path = path.join("aggregated.json");
            if aggregated_path.exists() {
                let content = std::fs::read_to_string(&aggregated_path)?;
                let aggregated: CaseTrialsResult = serde_json::from_str(&content)?;
                let total_input = aggregated
                    .trials
                    .iter()
                    .filter_map(|t| t.input_tokens)
                    .fold(None::<u64>, |acc, v| {
                        Some(acc.unwrap_or(0).saturating_add(v))
                    });
                let total_output = aggregated
                    .trials
                    .iter()
                    .filter_map(|t| t.output_tokens)
                    .fold(None::<u64>, |acc, v| {
                        Some(acc.unwrap_or(0).saturating_add(v))
                    });
                // Representative trial for per-case detail fields: the first
                // failing trial if any (its check_results/error_message explain
                // the aggregated failure), else the first trial.
                let representative = aggregated
                    .trials
                    .iter()
                    .find(|t| t.status != CaseStatus::Passed)
                    .or_else(|| aggregated.trials.first());
                results.push(CaseResult {
                    id: aggregated.id.clone(),
                    status: aggregated.aggregated_status.clone(),
                    command_count: representative.and_then(|t| t.command_count),
                    input_tokens: total_input,
                    output_tokens: total_output,
                    check_results: representative
                        .map(|t| t.check_results.clone())
                        .unwrap_or_default(),
                    error_message: representative.and_then(|t| t.error_message.clone()),
                    exit_code: representative.and_then(|t| t.exit_code),
                    terminal: representative.and_then(|t| t.terminal.clone()),
                    // Cost is summed across trials, not taken from the
                    // representative one: the case cost what every trial cost.
                    cost_usd: aggregated
                        .trials
                        .iter()
                        .filter_map(|t| t.cost_usd)
                        .fold(None::<f64>, |acc, v| Some(acc.unwrap_or(0.0) + v)),
                    tokens: representative.map(|t| t.tokens.clone()).unwrap_or_default(),
                    skill_path: representative.and_then(|t| t.skill_path.clone()),
                });
                continue;
            }

            // Legacy layout fallback: `{case_id}/result.json`
            let result_path = path.join("result.json");
            if result_path.exists() {
                let content = std::fs::read_to_string(&result_path)?;
                let result: CaseResult = serde_json::from_str(&content)?;
                results.push(result);
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_allocate_run_dir_creates_new() {
        let dir = TempDir::new().unwrap();
        let run_dir = allocate_run_dir(dir.path(), "2026-04-01T14-00-00Z").unwrap();
        assert!(run_dir.exists());
        assert!(run_dir.ends_with("2026-04-01T14-00-00Z"));
    }

    #[test]
    fn test_allocate_run_dir_suffix_on_conflict() {
        let dir = TempDir::new().unwrap();
        let run_dir1 = allocate_run_dir(dir.path(), "2026-04-01T14-00-00Z").unwrap();
        let run_dir2 = allocate_run_dir(dir.path(), "2026-04-01T14-00-00Z").unwrap();
        assert_ne!(run_dir1, run_dir2);
        assert!(run_dir2.to_string_lossy().contains("-2"));
    }

    #[test]
    fn test_allocate_run_dir_errors_after_suffix_exhaustion() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("run")).unwrap();
        for i in 2..=999 {
            std::fs::create_dir(dir.path().join(format!("run-{}", i))).unwrap();
        }

        let result = allocate_run_dir(dir.path(), "run");

        assert!(
            result.is_err(),
            "exhausting all 999 suffixes must error, not silently reuse the base dir"
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("EVAL_RUN_DIR_EXHAUSTED"));
    }

    #[test]
    fn test_read_case_results_sums_trial_tokens() {
        let dir = TempDir::new().unwrap();
        let case_dir = dir.path().join("case-1");
        std::fs::create_dir_all(&case_dir).unwrap();

        let trials_result = CaseTrialsResult {
            id: "case-1".to_string(),
            trials: vec![
                TrialResult {
                    trial_id: 1,
                    status: CaseStatus::Passed,
                    command_count: Some(1),
                    input_tokens: Some(100),
                    output_tokens: Some(50),
                    check_results: vec![],
                    error_message: None,
                    cost_usd: None,
                    exit_code: None,
                    terminal: None,
                    tokens: Default::default(),
                    skill_path: None,
                },
                TrialResult {
                    trial_id: 2,
                    status: CaseStatus::Passed,
                    command_count: Some(1),
                    input_tokens: Some(200),
                    output_tokens: Some(80),
                    check_results: vec![],
                    error_message: None,
                    cost_usd: None,
                    exit_code: None,
                    terminal: None,
                    tokens: Default::default(),
                    skill_path: None,
                },
            ],
            aggregated_status: CaseStatus::Passed,
            pass_count: 2,
            total_trials: 2,
            pass_rate: 1.0,
            error_count: 0,
            scored_trials: 0,
        };
        let json = serde_json::to_string_pretty(&trials_result).unwrap();
        std::fs::write(case_dir.join("aggregated.json"), json).unwrap();

        let results = read_case_results(dir.path()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].input_tokens,
            Some(300),
            "must sum input_tokens across trials"
        );
        assert_eq!(
            results[0].output_tokens,
            Some(130),
            "must sum output_tokens across trials"
        );
    }

    #[test]
    fn test_read_case_results_none_tokens_when_all_trials_none() {
        let dir = TempDir::new().unwrap();
        let case_dir = dir.path().join("case-null");
        std::fs::create_dir_all(&case_dir).unwrap();

        let trials_result = CaseTrialsResult {
            id: "case-null".to_string(),
            trials: vec![TrialResult {
                trial_id: 1,
                status: CaseStatus::Error,
                command_count: None,
                input_tokens: None,
                output_tokens: None,
                check_results: vec![],
                error_message: Some("timeout".to_string()),
                cost_usd: None,
                exit_code: None,
                terminal: None,
                tokens: Default::default(),
                skill_path: None,
            }],
            aggregated_status: CaseStatus::Error,
            pass_count: 0,
            total_trials: 1,
            pass_rate: 0.0,
            error_count: 0,
            scored_trials: 0,
        };
        let json = serde_json::to_string_pretty(&trials_result).unwrap();
        std::fs::write(case_dir.join("aggregated.json"), json).unwrap();

        let results = read_case_results(dir.path()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].input_tokens, None,
            "must remain None when all trial tokens are None"
        );
        assert_eq!(
            results[0].output_tokens, None,
            "must remain None when all trial tokens are None"
        );
    }

    #[test]
    fn test_read_case_results_populates_from_representative_failing_trial() {
        use crate::checks::CheckResult;
        let dir = TempDir::new().unwrap();

        let passing_trial = TrialResult {
            trial_id: 1,
            status: CaseStatus::Passed,
            command_count: Some(3),
            input_tokens: Some(10),
            output_tokens: Some(5),
            check_results: vec![CheckResult {
                check_name: "file_exists".to_string(),
                passed: true,
                required: true,
                message: None,
                not_observable: None,
            }],
            error_message: None,
            cost_usd: None,
            exit_code: None,
            terminal: None,
            tokens: Default::default(),
            skill_path: None,
        };
        let failing_trial = TrialResult {
            trial_id: 2,
            status: CaseStatus::Failed,
            command_count: Some(7),
            input_tokens: Some(20),
            output_tokens: Some(8),
            check_results: vec![CheckResult {
                check_name: "file_exists".to_string(),
                passed: false,
                required: true,
                message: Some("File 'out.txt' does not exist".to_string()),
                not_observable: None,
            }],
            error_message: Some("something went wrong".to_string()),
            cost_usd: None,
            exit_code: None,
            terminal: None,
            tokens: Default::default(),
            skill_path: None,
        };
        let trials_result = CaseTrialsResult {
            id: "case-repr".to_string(),
            trials: vec![passing_trial, failing_trial],
            aggregated_status: CaseStatus::Failed,
            pass_count: 1,
            total_trials: 2,
            pass_rate: 0.5,
            error_count: 0,
            scored_trials: 0,
        };
        write_case_trials_summary(dir.path(), "case-repr", &trials_result).unwrap();

        let results = read_case_results(dir.path()).unwrap();
        assert_eq!(results.len(), 1);
        let result = &results[0];
        assert_eq!(
            result.check_results.len(),
            1,
            "check_results must come from the representative (first failing) trial"
        );
        assert!(!result.check_results[0].passed);
        assert_eq!(
            result.check_results[0].message.as_deref(),
            Some("File 'out.txt' does not exist")
        );
        assert_eq!(
            result.error_message.as_deref(),
            Some("something went wrong"),
            "error_message on disk must survive the read"
        );
        assert_eq!(
            result.command_count,
            Some(7),
            "command_count must come from the representative trial"
        );
    }

    #[test]
    fn test_read_case_results_representative_defaults_to_first_trial() {
        use crate::checks::CheckResult;
        let dir = TempDir::new().unwrap();

        let trials_result = CaseTrialsResult {
            id: "case-allpass".to_string(),
            trials: vec![TrialResult {
                trial_id: 1,
                status: CaseStatus::Passed,
                command_count: Some(2),
                input_tokens: Some(10),
                output_tokens: Some(5),
                check_results: vec![CheckResult {
                    check_name: "max_tool_calls".to_string(),
                    passed: true,
                    required: true,
                    message: None,
                    not_observable: None,
                }],
                error_message: None,
                cost_usd: None,
                exit_code: None,
                terminal: None,
                tokens: Default::default(),
                skill_path: None,
            }],
            aggregated_status: CaseStatus::Passed,
            pass_count: 1,
            total_trials: 1,
            pass_rate: 1.0,
            error_count: 0,
            scored_trials: 0,
        };
        write_case_trials_summary(dir.path(), "case-allpass", &trials_result).unwrap();

        let results = read_case_results(dir.path()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].check_results.len(), 1);
        assert!(results[0].check_results[0].passed);
        assert_eq!(results[0].command_count, Some(2));
        assert_eq!(results[0].error_message, None);
    }

    #[test]
    fn test_write_and_read_summary() {
        let dir = TempDir::new().unwrap();
        let summary = SummaryResult {
            suite_pass: true,
            suite_pass_rate: Some(1.0),
            agent: "codex".to_string(),
            model: None,
            total_cases: 2,
            passed: 2,
            failed: 0,
            trials_per_case: Some(1),
            parallel: None,
            pass_threshold: Some(1.0),
            run_dir: dir.path().to_path_buf(),
            checks_path: None,
            skill_project_root: dir.path().to_path_buf(),
            isolation: Some(IsolationReport {
                requested: true,
                project_scope: ScopeFidelity::Isolated,
                user_scope: ScopeFidelity::Isolated,
                mechanism: Some("--setting-sources project".to_string()),
                agent_version: Some("2.1.215".to_string()),
                ambient_skills: vec!["probe-skill".to_string()],
                workspace_root: Some(dir.path().to_path_buf()),
                degrade_reason: None,
            }),
            cases: vec![],
        };

        write_summary(dir.path(), &summary).unwrap();
        let read = read_summary(dir.path()).unwrap();
        assert_eq!(read.total_cases, 2);
        assert!(read.suite_pass);
        let iso = read.isolation.expect("isolation block must round-trip");
        assert!(iso.requested);
        assert_eq!(iso.project_scope, ScopeFidelity::Isolated);
        assert_eq!(iso.ambient_skills, vec!["probe-skill".to_string()]);
    }

    /// spec 016 D6 back-compat: a pre-change summary.json with no `isolation`
    /// key must read as `None` (rendered "unknown"), never fail to parse.
    #[test]
    fn test_summary_backcompat_missing_isolation_reads_none() {
        let dir = TempDir::new().unwrap();
        let pre_016 = r#"{
            "suite_pass": true,
            "agent": "claude",
            "model": null,
            "total_cases": 1,
            "passed": 1,
            "failed": 0,
            "run_dir": "/tmp/run",
            "checks_path": null,
            "skill_project_root": "/tmp/proj",
            "cases": []
        }"#;
        std::fs::write(dir.path().join("summary.json"), pre_016).unwrap();

        let read = read_summary(dir.path()).expect("pre-016 summary.json must still parse");
        assert!(
            read.isolation.is_none(),
            "missing isolation key must deserialize as None, not a default claim"
        );
    }

    /// ADR 0020, additive: an artifact written before R7/R8 carries neither
    /// `skill_path` nor a case's `should_trigger`, and both must read as
    /// "not recorded" rather than as a claim.
    ///
    /// `should_trigger: None` reaching a scorer as `false` would be the worst
    /// of the two: `false` asserts the skill must *not* fire, so every case in
    /// a pre-R7 run would gain an inverted check nobody wrote and the whole
    /// suite would invert.
    #[test]
    fn test_summary_backcompat_missing_skill_path_and_should_trigger_read_none() {
        let dir = TempDir::new().unwrap();
        let pre_r7 = r#"{
            "suite_pass": true,
            "agent": "claude",
            "model": null,
            "total_cases": 1,
            "passed": 1,
            "failed": 0,
            "run_dir": "/tmp/run",
            "checks_path": null,
            "skill_project_root": "/tmp/proj",
            "cases": [{
                "id": "eval-skill",
                "status": "passed",
                "command_count": 3,
                "input_tokens": 10,
                "output_tokens": 20
            }]
        }"#;
        std::fs::write(dir.path().join("summary.json"), pre_r7).unwrap();

        let read = read_summary(dir.path()).expect("pre-R7 summary.json must still parse");
        assert!(
            read.cases[0].trials.is_empty(),
            "the fixture has no trials block; the assertions below are about the case"
        );
        assert!(
            read.cases[0].should_trigger.is_none(),
            "a missing should_trigger must stay None; Some(false) would invent an inverted check"
        );
    }

    /// The same two fields survive a write/read cycle, so a scorer reading a
    /// current artifact rebuilds the context the run actually used.
    #[test]
    fn test_skill_path_and_should_trigger_round_trip() {
        let dir = TempDir::new().unwrap();
        let staged = dir.path().join("skills/fastskill/SKILL.md");
        let summary = SummaryResult {
            suite_pass: false,
            suite_pass_rate: Some(0.0),
            agent: "pi".to_string(),
            model: None,
            total_cases: 1,
            passed: 0,
            failed: 1,
            trials_per_case: Some(1),
            parallel: None,
            pass_threshold: Some(1.0),
            run_dir: dir.path().to_path_buf(),
            checks_path: None,
            skill_project_root: dir.path().to_path_buf(),
            isolation: None,
            cases: vec![CaseSummary {
                id: "no-trigger".to_string(),
                status: CaseStatus::Failed,
                command_count: None,
                input_tokens: None,
                output_tokens: None,
                pass_count: Some(0),
                total_trials: Some(1),
                pass_rate: Some(0.0),
                error_count: Some(0),
                scored_trials: Some(1),
                should_trigger: Some(false),
                trials: vec![TrialResult {
                    trial_id: 1,
                    status: CaseStatus::Failed,
                    command_count: None,
                    input_tokens: None,
                    output_tokens: None,
                    check_results: vec![],
                    error_message: None,
                    exit_code: Some(0),
                    terminal: None,
                    cost_usd: None,
                    tokens: TokenBreakdown::default(),
                    skill_path: Some(staged.clone()),
                }],
            }],
        };

        write_summary(dir.path(), &summary).unwrap();
        let read = read_summary(dir.path()).unwrap();
        assert_eq!(
            read.cases[0].trials[0].skill_path.as_deref(),
            Some(staged.as_path())
        );
        assert_eq!(read.cases[0].should_trigger, Some(false));
    }

    #[test]
    fn test_scope_fidelity_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&ScopeFidelity::Unsupported).unwrap(),
            "\"unsupported\""
        );
        assert_eq!(
            serde_json::to_string(&ScopeFidelity::Isolated).unwrap(),
            "\"isolated\""
        );
        assert_eq!(
            serde_json::to_string(&ScopeFidelity::Inherited).unwrap(),
            "\"inherited\""
        );
    }

    // ── R4: errored trials are excluded from the rate ──────────────────────

    fn trial(id: u32, status: CaseStatus) -> TrialResult {
        TrialResult {
            trial_id: id,
            status,
            command_count: None,
            input_tokens: None,
            output_tokens: None,
            check_results: vec![],
            error_message: None,
            exit_code: None,
            terminal: None,
            cost_usd: None,
            tokens: TokenBreakdown::default(),
            skill_path: None,
        }
    }

    #[test]
    fn test_pass_rate_is_over_scored_trials_not_all_trials() {
        // Two passes and one outage. Counting the outage would report 0.67 and
        // blame the skill for a provider timeout; excluding it reports 1.0 and
        // says separately that one trial produced no measurement.
        let trials = vec![
            trial(1, CaseStatus::Passed),
            trial(2, CaseStatus::Error),
            trial(3, CaseStatus::Passed),
        ];
        let out = aggregate_trials("c1", trials, 3, 1.0);

        assert_eq!(out.error_count, 1);
        assert_eq!(out.scored_trials, 2);
        assert_eq!(out.pass_count, 2);
        assert_eq!(out.pass_rate, 1.0);
        assert_eq!(out.aggregated_status, CaseStatus::Passed);
        assert_eq!(
            out.total_trials, 3,
            "the excluded trial is still reported, never silently dropped"
        );
    }

    #[test]
    fn test_pass_rate_moves_when_the_errored_trial_is_counted_instead() {
        // The same three trials with the outage recorded as a failure — what
        // the engine did before R1 — score differently. This is the gap the
        // whole change exists to close.
        let as_failed = aggregate_trials(
            "c1",
            vec![
                trial(1, CaseStatus::Passed),
                trial(2, CaseStatus::Failed),
                trial(3, CaseStatus::Passed),
            ],
            3,
            1.0,
        );
        assert!((as_failed.pass_rate - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(as_failed.aggregated_status, CaseStatus::Failed);
        assert_eq!(as_failed.error_count, 0);
        assert_eq!(as_failed.scored_trials, 3);
    }

    #[test]
    fn test_case_with_no_scored_trials_is_error_not_failed() {
        let out = aggregate_trials(
            "c1",
            vec![trial(1, CaseStatus::Error), trial(2, CaseStatus::Error)],
            2,
            1.0,
        );
        assert_eq!(out.aggregated_status, CaseStatus::Error);
        assert_eq!(out.scored_trials, 0);
        assert_eq!(out.pass_rate, 0.0);
    }

    #[test]
    fn test_aggregate_trials_orders_trials_by_id() {
        let out = aggregate_trials(
            "c1",
            vec![
                trial(3, CaseStatus::Passed),
                trial(1, CaseStatus::Passed),
                trial(2, CaseStatus::Passed),
            ],
            3,
            1.0,
        );
        let ids: Vec<u32> = out.trials.iter().map(|t| t.trial_id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    // ── R5 / ADR 0020: the schema is additive ──────────────────────────────

    #[test]
    fn test_trial_result_written_before_this_version_still_deserializes() {
        // Exactly the artifact an older aikit-evals wrote. Every field added
        // by R5 must be absent-tolerant, or a committed fixture stops loading.
        let old = r#"{
            "trial_id": 1,
            "status": "passed",
            "command_count": 3,
            "input_tokens": 10,
            "output_tokens": 5,
            "check_results": [],
            "error_message": null
        }"#;
        let parsed: TrialResult = serde_json::from_str(old).unwrap();
        assert_eq!(parsed.trial_id, 1);
        assert_eq!(parsed.exit_code, None);
        assert!(parsed.terminal.is_none());
        assert_eq!(parsed.cost_usd, None);
        assert_eq!(parsed.tokens, TokenBreakdown::default());
        assert!(
            parsed.tokens.is_empty(),
            "absent means not recorded, never zero"
        );
    }

    #[test]
    fn test_case_trials_result_written_before_this_version_still_deserializes() {
        let old = r#"{
            "id": "c1",
            "trials": [],
            "aggregated_status": "passed",
            "pass_count": 2,
            "total_trials": 2,
            "pass_rate": 1.0
        }"#;
        let parsed: CaseTrialsResult = serde_json::from_str(old).unwrap();
        assert_eq!(parsed.error_count, 0);
        assert_eq!(parsed.scored_trials, 0);
    }

    #[test]
    fn test_terminal_and_cost_round_trip_through_the_artifact() {
        let mut t = trial(1, CaseStatus::Failed);
        t.exit_code = Some(0);
        t.cost_usd = Some(0.012_5);
        t.terminal = Some(TerminalRecord {
            outcome: TerminalOutcome::Error,
            reason: Some("error".to_string()),
            message: Some("Request timed out.".to_string()),
        });
        t.tokens = TokenBreakdown {
            total_tokens: Some(120),
            cache_read_tokens: Some(80),
            cache_creation_tokens: None,
            reasoning_tokens: Some(4),
        };
        let round: TrialResult = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(round.cost_usd, Some(0.012_5));
        assert_eq!(round.exit_code, Some(0));
        let term = round.terminal.unwrap();
        assert_eq!(term.outcome, TerminalOutcome::Error);
        assert_eq!(term.message.as_deref(), Some("Request timed out."));
        assert_eq!(round.tokens.cache_read_tokens, Some(80));
        assert_eq!(round.tokens.cache_creation_tokens, None);
    }

    #[test]
    fn test_case_status_error_serializes_lowercase() {
        // Serialized into every artifact; ADR 0020 forbids renaming it later.
        let json = serde_json::to_string(&CaseStatus::Error).unwrap();
        assert_eq!(json, "\"error\"");
    }

    fn a_trial() -> TrialResult {
        TrialResult {
            trial_id: 1,
            status: CaseStatus::Passed,
            command_count: Some(0),
            input_tokens: None,
            output_tokens: None,
            check_results: vec![],
            error_message: None,
            exit_code: Some(0),
            terminal: None,
            cost_usd: None,
            tokens: TokenBreakdown {
                total_tokens: None,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                reasoning_tokens: None,
            },
            skill_path: None,
        }
    }

    /// spec eval-judge R10: every seeded trial writes `workspace.diff`, an
    /// empty one included; a trial with no seeded state writes none.
    #[test]
    fn test_workspace_diff_is_written_when_seeded_and_absent_when_not() {
        let run = tempfile::tempdir().unwrap();
        let trial = a_trial();
        let base = TrialArtifacts {
            stdout: b"out",
            stderr: b"",
            trace_jsonl: "",
            workspace_diff: None,
            result: &trial,
        };

        let untouched = write_trial_artifacts(
            run.path(),
            "c",
            1,
            &TrialArtifacts {
                workspace_diff: Some(""),
                ..base
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(untouched.join("workspace.diff")).unwrap(),
            "",
            "an untouched workspace still writes the file"
        );

        let changed = write_trial_artifacts(
            run.path(),
            "c",
            2,
            &TrialArtifacts {
                workspace_diff: Some("--- /dev/null\n+++ b/x\n@@ -0,0 +1 @@\n+x\n"),
                ..base
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(changed.join("workspace.diff")).unwrap(),
            "--- /dev/null\n+++ b/x\n@@ -0,0 +1 @@\n+x\n"
        );

        let inherited = write_trial_artifacts(run.path(), "c", 3, &base).unwrap();
        assert!(
            !inherited.join("workspace.diff").exists(),
            "no seeded state must leave the file absent, not empty"
        );

        for dir in [&untouched, &changed, &inherited] {
            for name in ["stdout.txt", "stderr.txt", "trace.jsonl", "result.json"] {
                assert!(
                    dir.join(name).exists(),
                    "{name} missing in {}",
                    dir.display()
                );
            }
        }
        assert_eq!(std::fs::read(untouched.join("stdout.txt")).unwrap(), b"out");
    }
}
