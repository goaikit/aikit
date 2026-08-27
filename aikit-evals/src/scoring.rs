//! Scalar reward, gate-metric reduction, and pluggable Scorer trait.

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
/// All three variants reduce only checks whose `required` flag is true. Optional
/// check failures remain visible in the result vector but do not lower the item score.
/// An empty required-check set always yields `1.0` for all variants (vacuously successful).
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
/// When there are zero required checks, returns `1.0` regardless of metric (vacuously
/// successful). Optional check results are reported but ignored by the reduction.
pub fn item_score(results: &[CheckResult], metric: &GateMetric) -> f64 {
    let required_results: Vec<&CheckResult> = results.iter().filter(|r| r.required).collect();
    if required_results.is_empty() {
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
            let passed = required_results.iter().filter(|r| r.passed).count();
            passed as f64 / required_results.len() as f64
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
                    // spec 016 D2/D7: an isolated case ran in its own scratch
                    // workspace — score against THAT directory (the output
                    // keeps it alive until after scoring), falling back to
                    // the legacy project-root resolution under Inherit.
                    let working_dir = output
                        .workspace
                        .as_ref()
                        .map(|w| w.working_dir().to_path_buf())
                        .unwrap_or_else(|| match &case.workspace_subdir {
                            Some(subdir) => opts.project_root.join(subdir),
                            None => opts.project_root.clone(),
                        });
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
    // Aggregate by ordinal, not by check_name: `run_checks` names results by
    // check TYPE (e.g. "trigger_expectation"), so two same-typed checks would
    // collapse into one counter and a check failing every trial could report
    // passed on a same-typed sibling's votes. Every trial runs the same check
    // list in order, so the index within a trial's result vector is a stable
    // per-check identity; each trial contributes at most one vote per check.
    let check_count = trial_results.iter().map(Vec::len).max().unwrap_or(0);

    (0..check_count)
        .map(|idx| {
            let contributors: Vec<&CheckResult> = trial_results
                .iter()
                .filter_map(|trial| trial.get(idx))
                .collect();
            let check_name = contributors
                .first()
                .map(|r| r.check_name.clone())
                .unwrap_or_default();
            let pass_count = contributors.iter().filter(|r| r.passed).count();
            let required = contributors.iter().any(|r| r.required);
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
                check_name,
                passed,
                required,
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
            required: true,
            message: None,
        }
    }

    fn failed(name: &str) -> CheckResult {
        CheckResult {
            check_name: name.to_string(),
            passed: false,
            required: true,
            message: Some("fail".to_string()),
        }
    }

    fn optional_failed(name: &str) -> CheckResult {
        CheckResult {
            check_name: name.to_string(),
            passed: false,
            required: false,
            message: Some("optional fail".to_string()),
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
            isolation: crate::runner::IsolationMode::Inherit,
            retain_workspace_in: None,
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
    fn test_item_score_hard_ignores_optional_failure() {
        let r = vec![passed("required"), optional_failed("optional")];
        assert_eq!(item_score(&r, &GateMetric::Hard), 1.0);
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
    fn test_item_score_all_optional_failing_returns_one() {
        let r = vec![optional_failed("a"), optional_failed("b")];
        assert_eq!(item_score(&r, &GateMetric::Hard), 1.0);
        assert_eq!(item_score(&r, &GateMetric::Soft), 1.0);
        assert_eq!(item_score(&r, &GateMetric::Mixed { hard_weight: 0.5 }), 1.0);
    }

    #[test]
    fn test_item_score_soft_counts_required_checks_only() {
        let r = vec![passed("a"), failed("b"), optional_failed("c")];
        let expected = 1.0 / 2.0;
        let actual = item_score(&r, &GateMetric::Soft);
        assert!((actual - expected).abs() < 1e-12);
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
                workspace: None,
                isolation: None,
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
    async fn test_score_cases_majority_vote_preserves_required_flags() {
        let scorer = scripted_scorer(vec![
            vec![passed("required"), optional_failed("optional")],
            vec![passed("required"), optional_failed("optional")],
            vec![failed("required"), optional_failed("optional")],
        ]);
        let runner = ScriptedRunner;
        let cases = vec![make_case("c1")];
        let opts = make_opts();
        let result = score_cases(&runner, &cases, &opts, &scorer, 3, Some(1)).await;

        let required = result[0]
            .iter()
            .find(|r| r.check_name == "required")
            .unwrap();
        let optional = result[0]
            .iter()
            .find(|r| r.check_name == "optional")
            .unwrap();

        assert!(required.required);
        assert!(!optional.required);
        assert!(required.passed);
        assert!(!optional.passed);
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
    async fn test_score_cases_same_named_check_failing_all_trials_reports_failed() {
        // Two checks share the type name "trigger_expectation" (run_checks names
        // results by check TYPE). Check #2 fails every trial; it must not be
        // absorbed into check #1's pass counter.
        let scorer = scripted_scorer(vec![
            vec![passed("trigger_expectation"), failed("trigger_expectation")],
            vec![passed("trigger_expectation"), failed("trigger_expectation")],
            vec![passed("trigger_expectation"), failed("trigger_expectation")],
        ]);
        let runner = ScriptedRunner;
        let cases = vec![make_case("c1")];
        let opts = make_opts();
        let result = score_cases(&runner, &cases, &opts, &scorer, 3, Some(1)).await;

        assert_eq!(
            result[0].len(),
            2,
            "output must have one entry per input check, not per distinct name"
        );
        assert!(result[0][0].passed, "check #1 passed every trial");
        assert!(
            !result[0][1].passed,
            "check #2 failed every trial and must be reported failed"
        );
        assert!(
            !suite_passes(&result[0]),
            "a required check failing every trial must fail the case"
        );
    }

    #[tokio::test]
    async fn test_score_cases_ordinal_aggregation_caps_one_vote_per_trial() {
        // Two same-named checks both passing in one trial must contribute at
        // most one vote each — never two votes to a shared name counter.
        let scorer = scripted_scorer(vec![
            vec![passed("file_exists"), passed("file_exists")],
            vec![failed("file_exists"), failed("file_exists")],
            vec![failed("file_exists"), failed("file_exists")],
        ]);
        let runner = ScriptedRunner;
        let cases = vec![make_case("c1")];
        let opts = make_opts();
        let result = score_cases(&runner, &cases, &opts, &scorer, 3, Some(1)).await;

        assert_eq!(result[0].len(), 2);
        for r in &result[0] {
            assert!(
                !r.passed,
                "1/3 trials passing must not reach majority for either check"
            );
        }
    }

    /// spec 016 D7: the cross-case `file_exists` contamination test. Case A's
    /// agent creates a file; case B's `file_exists` on that file must NOT see
    /// it under per-case isolated workspaces. The `Inherit` half is the
    /// negative check: reverting to the shared directory makes case B pass on
    /// case A's leftovers, which is exactly the defect (ordering deciding the
    /// score) that per-case workspaces remove.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_score_cases_per_case_workspaces_prevent_file_exists_contamination() {
        use crate::runner::{AikitEvalRunner, IsolationMode, SkillSource};
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        // Fake claude: touches a file named by its prompt (read from stdin).
        let bin = tempfile::tempdir().unwrap();
        let script = bin.path().join("claude");
        {
            // Scoped so the handle is closed before the script is exec'd
            // (an open-for-write executable spawns with ETXTBSY).
            let mut f = std::fs::File::create(&script).unwrap();
            writeln!(
                f,
                "#!/bin/sh\np=$(cat)\ntouch \"$p\"\nprintf '%s\\n' '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"ok\",\"session_id\":\"s\"}}'"
            )
            .unwrap();
        }
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        let previous_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var(
            "PATH",
            format!("{}:{}", bin.path().display(), previous_path),
        );

        let case_a = EvalCase {
            id: "case-a".to_string(),
            prompt: "marker-a.txt".to_string(),
            should_trigger: true,
            tags: vec![],
            workspace_subdir: None,
        };
        let case_b = EvalCase {
            id: "case-b".to_string(),
            prompt: "marker-b.txt".to_string(),
            should_trigger: true,
            tags: vec![],
            workspace_subdir: None,
        };
        let cases = vec![case_a, case_b];
        // Both cases are scored on "did marker-a.txt appear in MY workspace".
        let scorer = ChecksScorer {
            checks: vec![CheckDefinition::FileExists {
                path: PathBuf::from("marker-a.txt"),
                required: true,
            }],
        };
        let runner = AikitEvalRunner::new();
        let project = tempfile::tempdir().unwrap();

        // Isolated: per-case workspaces — case B must NOT see case A's file.
        let isolated_opts = CaseRunOptions {
            agent_key: "claude".to_string(),
            model: None,
            project_root: project.path().to_path_buf(),
            timeout_seconds: 10,
            pass_threshold: 1.0,
            isolation: IsolationMode::Isolated {
                skill_name: "contamination-skill".to_string(),
                source: SkillSource::Inline("# s\n".to_string()),
            },
            retain_workspace_in: None,
        };
        let isolated = score_cases(&runner, &cases, &isolated_opts, &scorer, 1, Some(1)).await;
        assert!(
            isolated[0][0].passed,
            "case A created marker-a.txt in its own workspace: {:?}",
            isolated
        );
        assert!(
            !isolated[1][0].passed,
            "case B must NOT see case A's file under per-case workspaces"
        );

        // Inherit (the negative check): one shared directory — case B passes
        // on case A's leftover file, demonstrating the contamination.
        let shared = tempfile::tempdir().unwrap();
        let inherit_opts = CaseRunOptions {
            isolation: IsolationMode::Inherit,
            project_root: shared.path().to_path_buf(),
            ..isolated_opts
        };
        let inherited = score_cases(&runner, &cases, &inherit_opts, &scorer, 1, Some(1)).await;
        assert!(inherited[0][0].passed);
        assert!(
            inherited[1][0].passed,
            "shared-dir contamination: case B sees case A's file — the defect the isolated \
             path must not reproduce"
        );

        std::env::set_var("PATH", previous_path);
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
