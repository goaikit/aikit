//! Scalar reward, gate-metric reduction, and pluggable Scorer trait.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::checks::{run_checks, suite_passes, CheckDefinition, CheckResult};
use crate::runner::{CaseRunOptions, EvalRunner};
use crate::suite::EvalCase;

/// A benchmark's reward function: maps one captured trajectory to per-item check results.
///
/// Returning `Vec<CheckResult>` (not a bare scalar) lets the gate metric decide hard vs soft.
pub trait Scorer: Send + Sync {
    fn score(&self, stdout: &str, trace_jsonl: &str, working_dir: &Path) -> Vec<CheckResult>;
}

/// Default scorer: the deterministic checks engine already in this crate.
pub struct ChecksScorer {
    pub checks: Vec<CheckDefinition>,
}

impl Scorer for ChecksScorer {
    fn score(&self, stdout: &str, trace_jsonl: &str, wd: &Path) -> Vec<CheckResult> {
        run_checks(&self.checks, stdout, trace_jsonl, wd)
    }
}

/// How to reduce a scorer's per-item results to a scalar in [0, 1].
///
/// All three variants treat every element in the input `Vec<CheckResult>` as a required check.
/// An empty input slice always yields `1.0` for all variants (vacuously successful).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GateMetric {
    /// Per item: 1.0 iff all checks pass, else 0.0. Split score = accuracy.
    Hard,
    /// Per item: fraction of checks passed. Empty input → 1.0.
    Soft,
    /// Per item: `clamp(hard_weight, 0.0, 1.0) * hard + (1 - clamped) * soft`.
    Mixed { hard_weight: f64 },
}

/// Reduce one item's `Vec<CheckResult>` to a scalar in [0, 1] under `metric`.
///
/// When `results` is empty, returns `1.0` regardless of metric (zero required checks =
/// vacuously successful).
pub fn item_score(results: &[CheckResult], metric: &GateMetric) -> f64 {
    if results.is_empty() {
        return 1.0;
    }
    match metric {
        GateMetric::Hard => {
            if suite_passes(results) {
                1.0
            } else {
                0.0
            }
        }
        GateMetric::Soft => {
            let passed = results.iter().filter(|r| r.passed).count();
            passed as f64 / results.len() as f64
        }
        GateMetric::Mixed { hard_weight } => {
            let w = *hard_weight;
            let clamped = if w.is_nan() || w.is_infinite() && w < 0.0 {
                0.0
            } else if w.is_infinite() {
                1.0
            } else {
                w.clamp(0.0, 1.0)
            };
            let hard = item_score(results, &GateMetric::Hard);
            let soft = item_score(results, &GateMetric::Soft);
            clamped * hard + (1.0 - clamped) * soft
        }
    }
}

/// Mean of `item_score` across a set of items = the split-level score.
///
/// Returns `0.0` on an empty `items` slice.
pub fn split_score(items: &[Vec<CheckResult>], metric: &GateMetric) -> f64 {
    if items.is_empty() {
        return 0.0;
    }
    let sum: f64 = items.iter().map(|r| item_score(r, metric)).sum();
    sum / items.len() as f64
}

/// Run `trials` trials per case concurrently (bounded by `max_parallelism`), score each trial
/// with `scorer`, and return per-check majority-vote aggregated results for each case.
///
/// Majority-vote rule: a check is `passed = true` for a case iff it passed in strictly more
/// than half of the trials. Ties (equal pass and fail counts) count as not passed.
///
/// Returns one `Vec<CheckResult>` per input case in the same order as `cases`.
pub async fn score_cases(
    runner: &dyn EvalRunner,
    cases: &[EvalCase],
    opts: &CaseRunOptions,
    scorer: &dyn Scorer,
    trials: u32,
    max_parallelism: Option<u32>,
) -> Vec<Vec<CheckResult>> {
    if cases.is_empty() {
        return vec![];
    }

    let max_parallel = max_parallelism
        .unwrap_or_else(|| num_cpus::get().max(1) as u32)
        .max(1) as usize;
    let semaphore = Arc::new(Semaphore::new(max_parallel));

    let mut all_trial_results: Vec<Vec<Vec<CheckResult>>> = vec![Vec::new(); cases.len()];

    for (case_idx, case) in cases.iter().enumerate() {
        for _ in 0..trials {
            let trial_check_results = match semaphore.acquire().await {
                Err(_) => vec![],
                Ok(_permit) => {
                    let (output, _case_result, trace_jsonl) =
                        runner.run_case(case, opts, &[]).await;
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let working_dir = match &case.workspace_subdir {
                        Some(subdir) => opts.project_root.join(subdir),
                        None => opts.project_root.clone(),
                    };
                    scorer.score(&stdout, &trace_jsonl, &working_dir)
                }
            };
            all_trial_results[case_idx].push(trial_check_results);
        }
    }

    all_trial_results
        .into_iter()
        .map(|trial_vecs| majority_vote(trial_vecs, trials as usize))
        .collect()
}

fn majority_vote(trial_results: Vec<Vec<CheckResult>>, total_trials: usize) -> Vec<CheckResult> {
    let mut check_names: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for trial in &trial_results {
        for result in trial {
            if seen.insert(result.check_name.clone()) {
                check_names.push(result.check_name.clone());
            }
        }
    }

    let mut pass_counts: HashMap<String, usize> = HashMap::new();
    for trial in &trial_results {
        for result in trial {
            if result.passed {
                *pass_counts.entry(result.check_name.clone()).or_insert(0) += 1;
            }
        }
    }

    check_names
        .into_iter()
        .map(|name| {
            let pass_count = *pass_counts.get(&name).unwrap_or(&0);
            let passed = pass_count > total_trials / 2;
            let message = if passed {
                None
            } else {
                Some(format!(
                    "Majority vote failed: {}/{} trials passed",
                    pass_count, total_trials
                ))
            };
            CheckResult {
                check_name: name,
                passed,
                message,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use crate::artifacts::{CaseResult, CaseStatus, CaseTrialsResult, TrialResult};
    use crate::runner::CaseRunOutput;
    use crate::suite::EvalCase;
    use async_trait::async_trait;

    fn passed(name: &str) -> CheckResult {
        CheckResult {
            check_name: name.to_string(),
            passed: true,
            message: None,
        }
    }

    fn failed(name: &str) -> CheckResult {
        CheckResult {
            check_name: name.to_string(),
            passed: false,
            message: Some("fail".to_string()),
        }
    }

    fn make_case(id: &str) -> EvalCase {
        EvalCase {
            id: id.to_string(),
            prompt: "p".to_string(),
            should_trigger: true,
            tags: vec![],
            workspace_subdir: None,
        }
    }

    fn make_opts() -> CaseRunOptions {
        CaseRunOptions {
            agent_key: "stub".to_string(),
            model: None,
            project_root: PathBuf::from("/tmp"),
            timeout_seconds: 1,
            pass_threshold: 1.0,
        }
    }

    // ---- Scorer trait tests ----

    #[test]
    fn test_checks_scorer_identity_with_run_checks() {
        use crate::checks::CheckDefinition;
        let checks = vec![CheckDefinition::TriggerExpectation {
            pattern: "hello".to_string(),
            expected: true,
            required: true,
        }];
        let scorer = ChecksScorer {
            checks: checks.clone(),
        };
        let wd = Path::new("/tmp");
        let via_scorer = scorer.score("hello world", "", wd);
        let via_direct = run_checks(&checks, "hello world", "", wd);
        assert_eq!(via_scorer.len(), via_direct.len());
        for (a, b) in via_scorer.iter().zip(via_direct.iter()) {
            assert_eq!(a.check_name, b.check_name);
            assert_eq!(a.passed, b.passed);
        }
    }

    #[test]
    fn test_box_dyn_scorer_compiles() {
        let _: Box<dyn Scorer> = Box::new(ChecksScorer { checks: vec![] });
    }

    #[test]
    fn test_checks_scorer_empty_checks_returns_empty() {
        let scorer = ChecksScorer { checks: vec![] };
        let result = scorer.score("", "", Path::new("/tmp"));
        assert!(result.is_empty());
    }

    // ---- item_score tests ----

    #[test]
    fn test_item_score_hard_all_pass() {
        let r = vec![passed("a"), passed("b")];
        assert_eq!(item_score(&r, &GateMetric::Hard), 1.0);
        assert!(suite_passes(&r));
    }

    #[test]
    fn test_item_score_hard_any_fail() {
        let r = vec![passed("a"), failed("b")];
        assert_eq!(item_score(&r, &GateMetric::Hard), 0.0);
    }

    #[test]
    fn test_item_score_soft_fraction() {
        let r = vec![passed("a"), passed("b"), failed("c")];
        let expected = 2.0 / 3.0;
        let actual = item_score(&r, &GateMetric::Soft);
        assert!((actual - expected).abs() < 1e-12);
    }

    #[test]
    fn test_item_score_soft_empty_returns_one() {
        assert_eq!(item_score(&[], &GateMetric::Soft), 1.0);
    }

    #[test]
    fn test_item_score_mixed_weight_one_equals_hard() {
        let r = vec![passed("a"), failed("b")];
        let mixed = item_score(&r, &GateMetric::Mixed { hard_weight: 1.0 });
        let hard = item_score(&r, &GateMetric::Hard);
        assert!((mixed - hard).abs() < 1e-12);
    }

    #[test]
    fn test_item_score_mixed_weight_zero_equals_soft() {
        let r = vec![passed("a"), failed("b")];
        let mixed = item_score(&r, &GateMetric::Mixed { hard_weight: 0.0 });
        let soft = item_score(&r, &GateMetric::Soft);
        assert!((mixed - soft).abs() < 1e-12);
    }

    #[test]
    fn test_item_score_mixed_weight_half_is_midpoint() {
        let r = vec![passed("a"), failed("b")];
        let hard = item_score(&r, &GateMetric::Hard);
        let soft = item_score(&r, &GateMetric::Soft);
        let expected = 0.5 * hard + 0.5 * soft;
        let actual = item_score(&r, &GateMetric::Mixed { hard_weight: 0.5 });
        assert!((actual - expected).abs() < 1e-12);
    }

    #[test]
    fn test_item_score_mixed_no_panic_nan() {
        let r = vec![passed("a"), failed("b")];
        let v = item_score(
            &r,
            &GateMetric::Mixed {
                hard_weight: f64::NAN,
            },
        );
        assert!((0.0_f64..=1.0).contains(&v));
    }

    #[test]
    fn test_item_score_mixed_no_panic_infinity() {
        let r = vec![passed("a"), failed("b")];
        let v = item_score(
            &r,
            &GateMetric::Mixed {
                hard_weight: f64::INFINITY,
            },
        );
        assert!((0.0_f64..=1.0).contains(&v));
    }

    #[test]
    fn test_item_score_mixed_no_panic_neg_one() {
        let r = vec![passed("a"), failed("b")];
        let v = item_score(&r, &GateMetric::Mixed { hard_weight: -1.0 });
        assert!((0.0_f64..=1.0).contains(&v));
    }

    #[test]
    fn test_item_score_mixed_no_panic_two() {
        let r = vec![passed("a"), failed("b")];
        let v = item_score(&r, &GateMetric::Mixed { hard_weight: 2.0 });
        assert!((0.0_f64..=1.0).contains(&v));
    }

    // ---- split_score tests ----

    #[test]
    fn test_split_score_empty_returns_zero() {
        assert_eq!(split_score(&[], &GateMetric::Hard), 0.0);
    }

    #[test]
    fn test_split_score_arithmetic_mean() {
        let items: Vec<Vec<CheckResult>> = vec![
            vec![passed("a"), passed("b")],
            vec![passed("a"), failed("b")],
            vec![failed("a"), failed("b")],
        ];
        let expected = (item_score(&items[0], &GateMetric::Soft)
            + item_score(&items[1], &GateMetric::Soft)
            + item_score(&items[2], &GateMetric::Soft))
            / 3.0;
        let actual = split_score(&items, &GateMetric::Soft);
        assert!((actual - expected).abs() < 1e-12);
    }

    // ---- score_cases tests ----

    struct ScriptedScorer {
        queue: Arc<Mutex<VecDeque<Vec<CheckResult>>>>,
    }

    impl Scorer for ScriptedScorer {
        fn score(&self, _stdout: &str, _trace: &str, _wd: &Path) -> Vec<CheckResult> {
            self.queue.lock().unwrap().pop_front().unwrap_or_default()
        }
    }

    struct ScriptedRunner;

    #[async_trait]
    impl EvalRunner for ScriptedRunner {
        async fn run_case(
            &self,
            case: &EvalCase,
            _opts: &CaseRunOptions,
            _checks: &[CheckDefinition],
        ) -> (CaseRunOutput, CaseResult, String) {
            let out = CaseRunOutput {
                stdout: b"".to_vec(),
                stderr: vec![],
                exit_code: Some(0),
                timed_out: false,
            };
            let result = CaseResult {
                id: case.id.clone(),
                status: CaseStatus::Passed,
                command_count: Some(0),
                input_tokens: None,
                output_tokens: None,
                check_results: vec![],
                error_message: None,
            };
            (out, result, String::new())
        }

        async fn run_case_trials(
            &self,
            case: &EvalCase,
            opts: &CaseRunOptions,
            checks: &[CheckDefinition],
            trial_count: u32,
            _max_parallelism: Option<u32>,
        ) -> CaseTrialsResult {
            let mut trials = Vec::new();
            for trial_id in 1..=trial_count {
                let (_out, result, _trace) = self.run_case(case, opts, checks).await;
                trials.push(TrialResult {
                    trial_id,
                    status: result.status,
                    command_count: result.command_count,
                    input_tokens: result.input_tokens,
                    output_tokens: result.output_tokens,
                    check_results: result.check_results,
                    error_message: result.error_message,
                });
            }
            let pass_count = trials
                .iter()
                .filter(|t| t.status == CaseStatus::Passed)
                .count() as u32;
            let total_trials = trial_count.max(1);
            let pass_rate = pass_count as f64 / total_trials as f64;
            let aggregated_status = if pass_rate >= opts.pass_threshold {
                CaseStatus::Passed
            } else {
                CaseStatus::Failed
            };
            CaseTrialsResult {
                id: case.id.clone(),
                trials,
                aggregated_status,
                pass_count,
                total_trials,
                pass_rate,
            }
        }
    }

    fn scripted_scorer(results: Vec<Vec<CheckResult>>) -> ScriptedScorer {
        ScriptedScorer {
            queue: Arc::new(Mutex::new(results.into_iter().collect())),
        }
    }

    #[tokio::test]
    async fn test_score_cases_majority_vote_two_of_three_pass() {
        // 2/3 trials pass "foo" → passed: true
        let scorer = scripted_scorer(vec![
            vec![passed("foo")],
            vec![passed("foo")],
            vec![failed("foo")],
        ]);
        let runner = ScriptedRunner;
        let cases = vec![make_case("c1")];
        let opts = make_opts();
        let result = score_cases(&runner, &cases, &opts, &scorer, 3, Some(2)).await;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 1);
        assert!(result[0][0].passed, "2/3 should pass");
    }

    #[tokio::test]
    async fn test_score_cases_majority_vote_one_of_three_fail() {
        // 1/3 trials pass "foo" → passed: false
        let scorer = scripted_scorer(vec![
            vec![passed("foo")],
            vec![failed("foo")],
            vec![failed("foo")],
        ]);
        let runner = ScriptedRunner;
        let cases = vec![make_case("c1")];
        let opts = make_opts();
        let result = score_cases(&runner, &cases, &opts, &scorer, 3, Some(2)).await;
        assert_eq!(result.len(), 1);
        assert!(!result[0][0].passed, "1/3 should not pass");
    }

    #[tokio::test]
    async fn test_score_cases_majority_vote_tie_fails() {
        // 1/2 (tie) → passed: false
        let scorer = scripted_scorer(vec![vec![passed("foo")], vec![failed("foo")]]);
        let runner = ScriptedRunner;
        let cases = vec![make_case("c1")];
        let opts = make_opts();
        let result = score_cases(&runner, &cases, &opts, &scorer, 2, Some(2)).await;
        assert_eq!(result.len(), 1);
        assert!(!result[0][0].passed, "tie (1/2) should not pass");
    }

    #[tokio::test]
    async fn test_score_cases_output_length_equals_case_count() {
        let scorer = scripted_scorer(vec![
            vec![passed("x")],
            vec![passed("x")],
            vec![passed("x")],
            vec![passed("x")],
            vec![passed("x")],
            vec![passed("x")],
        ]);
        let runner = ScriptedRunner;
        let cases = vec![make_case("c1"), make_case("c2")];
        let opts = make_opts();
        let result = score_cases(&runner, &cases, &opts, &scorer, 3, Some(2)).await;
        assert_eq!(result.len(), cases.len());
    }

    #[tokio::test]
    async fn test_score_cases_order_preserved() {
        // c1 gets 3 passed "foo", c2 gets 3 failed "foo"
        let scorer = scripted_scorer(vec![
            vec![passed("foo")],
            vec![passed("foo")],
            vec![passed("foo")],
            vec![failed("foo")],
            vec![failed("foo")],
            vec![failed("foo")],
        ]);
        let runner = ScriptedRunner;
        let cases = vec![make_case("c1"), make_case("c2")];
        let opts = make_opts();
        let result = score_cases(&runner, &cases, &opts, &scorer, 3, None).await;
        assert!(result[0][0].passed, "c1 should pass");
        assert!(!result[1][0].passed, "c2 should fail");
    }

    #[tokio::test]
    async fn test_score_cases_empty_cases_returns_empty() {
        let scorer = scripted_scorer(vec![]);
        let runner = ScriptedRunner;
        let cases: Vec<EvalCase> = vec![];
        let opts = make_opts();
        let result = score_cases(&runner, &cases, &opts, &scorer, 3, None).await;
        assert!(result.is_empty());
    }
}
