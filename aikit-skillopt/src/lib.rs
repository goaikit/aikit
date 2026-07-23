pub mod artifact;
pub mod prompts;

pub use aikit_textgrad::training::{RunConfig, TrainingOutcome};
pub use artifact::SkillArtifact;
pub use prompts::skill_prompts;

use std::path::PathBuf;

use aikit_evals::{CheckDefinition, ChecksScorer, EvalCase, EvalRunner};
use aikit_textgrad::training::{resume_training, run_training};

/// All caller-supplied data for a new training run.
pub struct SkillOptInputs {
    /// Raw content of the seed SKILL.md. MUST be non-empty.
    pub initial_skill_md: String,
    /// Skill name (e.g. "research-assistant"). Passed to deploy_skill and SkillArtifact.
    pub skill_name: String,
    /// Eval suite. Split tags ("train"/"selection"/"test") MUST already be set.
    /// MUST contain at least one case tagged "selection".
    pub suite: Vec<EvalCase>,
    /// Deterministic checks for the ChecksScorer.
    pub checks: Vec<CheckDefinition>,
    /// Run configuration. `config.artifact_stem` MUST equal "skill".
    pub config: RunConfig,
    /// Writable run directory for state and artifact persistence.
    pub run_dir: PathBuf,
}

/// Run a complete training loop for a skill document from scratch.
///
/// `runner` drives every eval-case execution the loop performs (initial score, per-step
/// rollouts, gate re-scoring). Callers pass `&AikitEvalRunner` in production; tests inject a
/// scripted double to exercise gate accept/reject dynamics deterministically.
pub async fn train_skill(
    inputs: SkillOptInputs,
    runner: &dyn EvalRunner,
) -> anyhow::Result<TrainingOutcome> {
    let mut artifact = SkillArtifact::from_existing(
        inputs.initial_skill_md,
        inputs.skill_name,
        inputs.config.target_agent.clone(),
    );
    let scorer = ChecksScorer {
        checks: inputs.checks,
    };
    run_training(
        &mut artifact,
        &inputs.suite,
        &scorer,
        runner,
        skill_prompts(),
        inputs.config,
        &inputs.run_dir,
    )
    .await
    .map_err(anyhow::Error::from)
}

/// Resume an interrupted training run from the last checkpoint.
///
/// The caller MUST supply the same `skill_name` and `target_agent` as the original run.
/// `resume_training` restores the artifact text from `best_skill.md` in `run_dir`.
/// See [`train_skill`] for the meaning of `runner`.
#[allow(clippy::too_many_arguments)]
pub async fn resume_skill(
    run_dir: PathBuf,
    initial_skill_md: String,
    skill_name: String,
    suite: Vec<EvalCase>,
    checks: Vec<CheckDefinition>,
    config: RunConfig,
    runner: &dyn EvalRunner,
) -> anyhow::Result<TrainingOutcome> {
    let mut artifact =
        SkillArtifact::from_existing(initial_skill_md, skill_name, config.target_agent.clone());
    let scorer = ChecksScorer { checks };
    resume_training(
        &run_dir,
        &mut artifact,
        &suite,
        &scorer,
        runner,
        skill_prompts(),
    )
    .await
    .map_err(anyhow::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_evals::{
        AikitEvalRunner, CaseResult, CaseRunOptions, CaseRunOutput, CaseStatus, CaseTrialsResult,
        EvalCase, TrialResult,
    };
    use aikit_textgrad::training::state::{init_run_dir, write_runtime_state, RuntimeState};
    use aikit_textgrad::training::{SlowUpdateMode, StepRecord};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tempfile::TempDir;

    // ---- ScriptedEvalRunner: injectable EvalRunner double (F6) ----
    //
    // Two independent trigger-expectation markers ("M1"/"M2"), scored with GateMetric::Soft,
    // give three controllable score levels per scripted call: neither present = 0.0, one = 0.5,
    // both = 1.0. This lets tests script realistic multi-step score trajectories (improve,
    // regress, mixed) and observe the GATE's actual accept/reject decisions and best_score
    // bookkeeping — coverage the old windsurf-agent no-op could never exercise, since every
    // call there failed identically and no score ever changed.
    #[derive(Clone, Copy)]
    enum ScriptedOutcome {
        Score0,
        Score1,
        Score2,
        TimedOut,
    }

    impl ScriptedOutcome {
        fn stdout(self) -> &'static [u8] {
            match self {
                ScriptedOutcome::Score0 | ScriptedOutcome::TimedOut => b"",
                ScriptedOutcome::Score1 => b"M1",
                ScriptedOutcome::Score2 => b"M1 M2",
            }
        }
    }

    /// Two checks of *different* kinds (not two `TriggerExpectation`s): every
    /// `TriggerExpectation` reports the same fixed `check_name` ("trigger_expectation")
    /// regardless of pattern, and the GATE path routes through `score_cases`'s
    /// majority-vote-by-name aggregation, which collapses same-named results — so two
    /// same-kind checks would silently merge into one and only ever yield 0.0/1.0. Distinct
    /// check kinds keep them as two separate named results, giving a real 0.0/0.5/1.0 range.
    fn score_markers() -> Vec<CheckDefinition> {
        vec![
            CheckDefinition::CommandContains {
                pattern: "M1".to_string(),
                required: true,
            },
            CheckDefinition::TriggerExpectation {
                pattern: "M2".to_string(),
                expected: true,
                required: true,
            },
        ]
    }

    /// Pops one scripted outcome per `run_case` call, in call order: initial score, then per
    /// step (rollout, gate). Panics on an exhausted queue — that means the test's assumed call
    /// count has drifted from the loop's actual behavior, which is itself worth surfacing loudly.
    struct ScriptedEvalRunner {
        outcomes: Mutex<VecDeque<ScriptedOutcome>>,
        calls: AtomicUsize,
    }

    impl ScriptedEvalRunner {
        fn new(outcomes: Vec<ScriptedOutcome>) -> Self {
            Self {
                outcomes: Mutex::new(outcomes.into_iter().collect()),
                calls: AtomicUsize::new(0),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl EvalRunner for ScriptedEvalRunner {
        async fn run_case(
            &self,
            case: &EvalCase,
            _opts: &CaseRunOptions,
            _checks: &[CheckDefinition],
        ) -> (CaseRunOutput, CaseResult, String) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let outcome = self.outcomes.lock().unwrap().pop_front().expect(
                "ScriptedEvalRunner queue exhausted — expected call count no longer matches",
            );
            let output = CaseRunOutput {
                stdout: outcome.stdout().to_vec(),
                stderr: vec![],
                exit_code: Some(0),
                timed_out: matches!(outcome, ScriptedOutcome::TimedOut),
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
            (output, result, String::new())
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

    /// Read and deserialize `run_dir/history.json`, written one `StepRecord` per accepted step.
    async fn read_history(dir: &TempDir) -> Vec<StepRecord> {
        let bytes = tokio::fs::read(dir.path().join("history.json"))
            .await
            .expect("history.json should exist after a run");
        serde_json::from_slice(&bytes).expect("history.json should deserialize as Vec<StepRecord>")
    }

    fn make_eval_case(id: &str, tags: &[&str]) -> EvalCase {
        EvalCase {
            id: id.to_string(),
            prompt: format!("prompt for {id}"),
            should_trigger: true,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            workspace_subdir: None,
        }
    }

    fn make_config() -> RunConfig {
        RunConfig {
            n_epochs: 1,
            batch_size: 1,
            accumulation: 1,
            aggregate_group_size: 2,
            lr_0: 2,
            pass_threshold: 0.5,
            gate_metric: aikit_evals::GateMetric::Soft,
            gate_trials: 1,
            gate_epsilon: 0.01,
            slow_update_mode: SlowUpdateMode::ForceAccept,
            protected_soft_cap_chars: 500,
            // `windsurf` supports skill deployment (`skills: Some(".windsurf/skills")`, so
            // materialize resolves a skill dir) but is NOT a runnable Backend, so AikitEvalRunner
            // fails fast without spawning any subprocess. Used only by the retained
            // `test_train_skill_end_to_end`/`test_best_skill_md_content_matches_outcome` smoke
            // tests below; the value is irrelevant to ScriptedEvalRunner-based tests, which never
            // spawn a real agent regardless of this key (see F6 spec).
            target_agent: "windsurf".to_string(),
            target_model: None,
            optimizer_agent: "windsurf".to_string(),
            optimizer_model: None,
            timeout_seconds: 30,
            parallel: Some(1),
            artifact_stem: "skill".to_string(),
        }
    }

    fn make_inputs(dir: &TempDir) -> SkillOptInputs {
        SkillOptInputs {
            initial_skill_md: "# Test Skill\n\nSome skill content.".to_string(),
            skill_name: "test-skill".to_string(),
            suite: vec![
                make_eval_case("train-1", &["train"]),
                make_eval_case("sel-1", &["selection"]),
            ],
            checks: vec![],
            config: make_config(),
            run_dir: dir.path().to_path_buf(),
        }
    }

    /// Like [`make_inputs`], but wired for [`ScriptedEvalRunner`]: uses the two-marker checks
    /// so scores are controllable, and `n_epochs` steps (batch=1, one train case) so the queue
    /// has exactly one rollout+gate pair per epoch.
    fn make_scripted_inputs(dir: &TempDir, n_epochs: u32) -> SkillOptInputs {
        let mut inputs = make_inputs(dir);
        inputs.checks = score_markers();
        inputs.config.n_epochs = n_epochs;
        inputs
    }

    // AC-7: train_skill runs end-to-end and returns Ok with best_artifact_path = best_skill.md.
    #[tokio::test]
    async fn test_train_skill_end_to_end() {
        let dir = TempDir::new().unwrap();
        let inputs = make_inputs(&dir);
        let result = train_skill(inputs, &AikitEvalRunner).await;
        assert!(result.is_ok(), "train_skill failed: {result:?}");
        let outcome = result.unwrap();
        let expected_path = dir.path().join("best_skill.md");
        assert_eq!(outcome.best_artifact_path, expected_path);
        assert!(expected_path.exists(), "best_skill.md should exist");
    }

    // AC-8: best_skill.md content equals TrainingOutcome::best_text.
    #[tokio::test]
    async fn test_best_skill_md_content_matches_outcome() {
        let dir = TempDir::new().unwrap();
        let inputs = make_inputs(&dir);
        let outcome = train_skill(inputs, &AikitEvalRunner).await.unwrap();
        let on_disk = std::fs::read_to_string(&outcome.best_artifact_path).unwrap();
        assert_eq!(on_disk, outcome.best_text);
    }

    // AC-9: resume_skill after completed run (epoch >= n_epochs) skips the loop.
    #[tokio::test]
    async fn test_resume_skill_after_completed_run() {
        let dir = TempDir::new().unwrap();
        let config = make_config();
        let suite = vec![
            make_eval_case("train-1", &["train"]),
            make_eval_case("sel-1", &["selection"]),
        ];

        // Manually set up a completed run-dir state.
        init_run_dir(dir.path(), &config).await.unwrap();
        let state = RuntimeState {
            config: config.clone(),
            epoch: config.n_epochs, // all epochs done
            step_in_epoch: 0,
            global_step: 1,
            best_score: 0.9,
            current_score: 0.9,
            rejected_edit_buffer: vec![],
            optimizer_strategy: "saved strategy".to_string(),
        };
        write_runtime_state(dir.path(), &state).await.unwrap();
        tokio::fs::write(
            dir.path().join("best_skill.md"),
            b"# Saved Skill\n\nSaved content.",
        )
        .await
        .unwrap();
        tokio::fs::write(dir.path().join("history.json"), b"[]")
            .await
            .unwrap();

        // Empty queue: a completed run (epoch >= n_epochs) with no test cases must resume
        // without ever consulting the runner. `ScriptedEvalRunner` panics on any pop, so this
        // proves the short-circuit rather than merely asserting the outcome.
        let runner = ScriptedEvalRunner::new(vec![]);
        let result = resume_skill(
            dir.path().to_path_buf(),
            "# Test Skill\n\nOriginal.".to_string(),
            "test-skill".to_string(),
            suite,
            vec![],
            config,
            &runner,
        )
        .await;

        assert!(result.is_ok(), "resume_skill failed: {result:?}");
        let outcome = result.unwrap();
        assert!(
            (outcome.best_score - 0.9).abs() < 1e-9,
            "expected best_score 0.9, got {}",
            outcome.best_score
        );
        assert_eq!(
            runner.call_count(),
            0,
            "completed resume must not call the runner"
        );
    }

    // AC-12: train_skill with zero selection cases returns TEXTGRAD_NO_SELECTION_CASES error.
    #[tokio::test]
    async fn test_train_skill_no_selection_cases() {
        let dir = TempDir::new().unwrap();
        let mut inputs = make_inputs(&dir);
        inputs.suite = vec![make_eval_case("train-1", &["train"])];
        // Validation fails before the runner is ever touched — empty queue proves it.
        let result = train_skill(inputs, &ScriptedEvalRunner::new(vec![])).await;
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("TEXTGRAD_NO_SELECTION_CASES"),
            "expected TEXTGRAD_NO_SELECTION_CASES in: {err}"
        );
    }

    // AC-13: train_skill with config.batch_size == 0 returns TEXTGRAD_INVALID_CONFIG error.
    #[tokio::test]
    async fn test_train_skill_invalid_config() {
        let dir = TempDir::new().unwrap();
        let mut inputs = make_inputs(&dir);
        inputs.config.batch_size = 0;
        let result = train_skill(inputs, &ScriptedEvalRunner::new(vec![])).await;
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("TEXTGRAD_INVALID_CONFIG"),
            "expected TEXTGRAD_INVALID_CONFIG in: {err}"
        );
    }

    // ---- F6 payoff: genuine GATE accept/reject coverage via ScriptedEvalRunner ----

    #[tokio::test]
    async fn test_gate_accepts_improving_scores() {
        use ScriptedOutcome::*;
        let dir = TempDir::new().unwrap();
        let inputs = make_scripted_inputs(&dir, 2);
        let runner = ScriptedEvalRunner::new(vec![
            Score0, // initial score = 0.0
            Score1, Score1, // epoch0: rollout (irrelevant), gate = 0.5 -> accept (best=0.5)
            Score1, Score2, // epoch1: rollout (irrelevant), gate = 1.0 -> accept (best=1.0)
        ]);

        let outcome = train_skill(inputs, &runner).await.unwrap();
        assert!(
            (outcome.best_score - 1.0).abs() < 1e-9,
            "expected best_score 1.0, got {}",
            outcome.best_score
        );

        let history = read_history(&dir).await;
        assert_eq!(history.len(), 2);
        assert!(
            history[0].accepted && history[1].accepted,
            "both steps should accept"
        );
        assert!((history[0].score_candidate - 0.5).abs() < 1e-9);
        assert!((history[1].score_candidate - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn test_gate_rejects_flat_or_regressing_scores() {
        use ScriptedOutcome::*;
        let dir = TempDir::new().unwrap();
        let inputs = make_scripted_inputs(&dir, 2);
        let runner = ScriptedEvalRunner::new(vec![
            Score1, // initial score = 0.5
            Score0, Score0, // epoch0: rollout, gate = 0.0 -> reject (best stays 0.5)
            Score0, Score1, // epoch1: rollout, gate = 0.5 (not > 0.5+epsilon) -> reject
        ]);

        let outcome = train_skill(inputs, &runner).await.unwrap();
        assert!(
            (outcome.best_score - 0.5).abs() < 1e-9,
            "expected best_score to stay at 0.5, got {}",
            outcome.best_score
        );

        let history = read_history(&dir).await;
        assert_eq!(history.len(), 2);
        assert!(
            !history[0].accepted && !history[1].accepted,
            "neither step should accept"
        );
    }

    #[tokio::test]
    async fn test_gate_tracks_max_through_mixed_trajectory() {
        use ScriptedOutcome::*;
        let dir = TempDir::new().unwrap();
        let inputs = make_scripted_inputs(&dir, 3);
        let runner = ScriptedEvalRunner::new(vec![
            Score0, // initial score = 0.0
            Score1, Score1, // epoch0: gate = 0.5 -> accept (best=0.5)
            Score1, Score0, // epoch1: gate = 0.0 -> reject (best stays 0.5)
            Score1, Score2, // epoch2: gate = 1.0 -> accept (best=1.0)
        ]);

        let outcome = train_skill(inputs, &runner).await.unwrap();
        assert!(
            (outcome.best_score - 1.0).abs() < 1e-9,
            "best_score must track the max seen (1.0), got {}",
            outcome.best_score
        );

        let history = read_history(&dir).await;
        assert_eq!(history.len(), 3);
        let accepted: Vec<bool> = history.iter().map(|r| r.accepted).collect();
        assert_eq!(
            accepted,
            vec![true, false, true],
            "expected improve/reject/improve pattern, got {accepted:?}"
        );
    }

    #[tokio::test]
    async fn test_gate_rejects_timed_out_rollout_without_panicking() {
        use ScriptedOutcome::*;
        let dir = TempDir::new().unwrap();
        let inputs = make_scripted_inputs(&dir, 1);
        let runner = ScriptedEvalRunner::new(vec![
            Score1, // initial score = 0.5
            Score1, TimedOut, // rollout (irrelevant), gate times out -> empty stdout -> 0.0
        ]);

        let outcome = train_skill(inputs, &runner).await.unwrap();
        assert!(
            (outcome.best_score - 0.5).abs() < 1e-9,
            "a timed-out gate call must be treated as a reject, not crash the loop"
        );

        let history = read_history(&dir).await;
        assert_eq!(history.len(), 1);
        assert!(!history[0].accepted);
    }

    #[tokio::test]
    async fn test_scripted_runner_run_case_trials_aggregates_pass_rate() {
        // run_case_trials is never called by the training loop itself (only run_case is —
        // see F6 spec), but ScriptedEvalRunner must still implement it correctly as a trait
        // member, matching the house StubRunner/StubEvalRunner pattern.
        use ScriptedOutcome::*;
        let runner = ScriptedEvalRunner::new(vec![Score2, Score0, Score2]);
        let case = make_eval_case("c1", &["train"]);
        let opts = CaseRunOptions {
            agent_key: "scripted".to_string(),
            model: None,
            project_root: std::path::PathBuf::from("/tmp"),
            timeout_seconds: 1,
            pass_threshold: 0.5,
        };

        let result = runner.run_case_trials(&case, &opts, &[], 3, None).await;

        assert_eq!(result.total_trials, 3);
        assert_eq!(
            result.pass_count, 3,
            "CaseStatus::Passed regardless of stdout content"
        );
        assert_eq!(runner.call_count(), 3);
    }

    #[tokio::test]
    async fn test_scripted_runner_call_count_matches_rollout_and_gate_calls() {
        use ScriptedOutcome::*;
        let dir = TempDir::new().unwrap();
        let inputs = make_scripted_inputs(&dir, 1);
        let runner = ScriptedEvalRunner::new(vec![Score1, Score1, Score1]);

        train_skill(inputs, &runner).await.unwrap();

        // 1 initial-score call + 1 rollout + 1 gate call (1 train case, 1 selection case,
        // gate_trials=1, n_epochs=1).
        assert_eq!(runner.call_count(), 3);
    }

    // AC-10: no epoch/gate/edit logic in this crate.
    // Production source files (artifact.rs, prompts.rs) must not call training-loop internals.
    // lib.rs is excluded because this test file itself references the symbol names as strings.
    #[test]
    fn test_no_internal_loop_logic() {
        let artifact_src = include_str!("artifact.rs");
        let prompts_src = include_str!("prompts.rs");
        let combined = format!("{artifact_src}{prompts_src}");
        // These symbols indicate loop/gate/edit logic that must stay in aikit-textgrad only.
        let forbidden = [
            "run_slow_update",
            "run_meta_skill",
            "run_step",
            "apply_budgeted",
        ];
        for symbol in &forbidden {
            assert!(
                !combined.contains(symbol),
                "artifact.rs/prompts.rs must not contain '{symbol}'"
            );
        }
    }
}
