//! Artifact layout and persistence for eval runs

use crate::checks::CheckResult;
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
}

/// Aggregated results for a case across multiple trials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseTrialsResult {
    pub id: String,
    pub trials: Vec<TrialResult>,
    pub aggregated_status: CaseStatus,
    pub pass_count: u32,
    pub total_trials: u32,
    pub pass_rate: f64,
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

/// Write per-trial artifacts (stdout.txt, stderr.txt, trace.jsonl, result.json) under:
/// `{run_dir}/{case_id}/trial-{trial_id}/`
pub fn write_trial_artifacts(
    run_dir: &Path,
    case_id: &str,
    trial_id: u32,
    stdout: &[u8],
    stderr: &[u8],
    trace_jsonl: &str,
    result: &TrialResult,
) -> Result<PathBuf, ArtifactsError> {
    let trial_dir = run_dir.join(case_id).join(format!("trial-{}", trial_id));
    std::fs::create_dir_all(&trial_dir)?;

    std::fs::write(trial_dir.join("stdout.txt"), stdout)?;
    std::fs::write(trial_dir.join("stderr.txt"), stderr)?;
    std::fs::write(trial_dir.join("trace.jsonl"), trace_jsonl)?;

    let result_json = serde_json::to_string_pretty(result)?;
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
    result: &CaseResult,
) -> Result<PathBuf, ArtifactsError> {
    let trial = case_result_to_trial(result, 1);
    let trial_dir =
        write_trial_artifacts(run_dir, case_id, 1, stdout, stderr, trace_jsonl, &trial)?;

    let pass_count = if result.status == CaseStatus::Passed {
        1
    } else {
        0
    };
    let pass_rate = pass_count as f64;
    let aggregated = CaseTrialsResult {
        id: result.id.clone(),
        trials: vec![trial],
        aggregated_status: result.status.clone(),
        pass_count,
        total_trials: 1,
        pass_rate,
    };
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
                },
                TrialResult {
                    trial_id: 2,
                    status: CaseStatus::Passed,
                    command_count: Some(1),
                    input_tokens: Some(200),
                    output_tokens: Some(80),
                    check_results: vec![],
                    error_message: None,
                },
            ],
            aggregated_status: CaseStatus::Passed,
            pass_count: 2,
            total_trials: 2,
            pass_rate: 1.0,
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
            }],
            aggregated_status: CaseStatus::Error,
            pass_count: 0,
            total_trials: 1,
            pass_rate: 0.0,
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
            }],
            error_message: None,
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
            }],
            error_message: Some("something went wrong".to_string()),
        };
        let trials_result = CaseTrialsResult {
            id: "case-repr".to_string(),
            trials: vec![passing_trial, failing_trial],
            aggregated_status: CaseStatus::Failed,
            pass_count: 1,
            total_trials: 2,
            pass_rate: 0.5,
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
                }],
                error_message: None,
            }],
            aggregated_status: CaseStatus::Passed,
            pass_count: 1,
            total_trials: 1,
            pass_rate: 1.0,
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
            cases: vec![],
        };

        write_summary(dir.path(), &summary).unwrap();
        let read = read_summary(dir.path()).unwrap();
        assert_eq!(read.total_cases, 2);
        assert!(read.suite_pass);
    }
}
