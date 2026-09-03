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

/// Reject an empty check set at the skillopt boundary.
///
/// With zero checks every case's `item_score` is a vacuous 1.0, so `best_score` starts at
/// 1.0 and the gate condition (`score > best + epsilon`) is unsatisfiable — the whole
/// training run would be a silent no-op that still returns `Ok`.
fn validate_checks(checks: &[CheckDefinition]) -> anyhow::Result<()> {
    if checks.is_empty() {
        anyhow::bail!(
            "SKILLOPT_EMPTY_CHECKS: checks must be non-empty — with zero checks every case \
             scores a vacuous 1.0 and the gate can never accept, so training would be a no-op"
        );
    }
    Ok(())
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
    validate_checks(&inputs.checks)?;
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
    validate_checks(&checks)?;
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
        aggregate_trials, stdout_to_trace, trace_to_jsonl, AikitEvalRunner, CaseResult,
        CaseRunOptions, CaseRunOutput, CaseStatus, CaseTrialsResult, EvalCase, TrialResult,
    };
    use aikit_textgrad::training::state::{init_run_dir, write_runtime_state, RuntimeState};
    use aikit_textgrad::training::{SlowUpdateMode, StepRecord};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tempfile::TempDir;

    // ---- ScriptedEvalRunner: injectable EvalRunner double (F6) ----
    //
    // Two independent markers ("M1"/"M2"), scored with GateMetric::Soft, give three
    // controllable score levels per scripted call: neither present = 0.0, one = 0.5,
    // both = 1.0. This lets tests script realistic multi-step score trajectories (improve,
    // regress, mixed) and observe the GATE's actual accept/reject decisions and best_score
    // bookkeeping — coverage the old windsurf-agent no-op could never exercise, since every
    // call there failed identically and no score ever changed.
    //
    // The markers must reach the *trace*: checks score canonical trace JSONL and deliberately
    // ignore raw stdout, because raw stdout carries agent capability listings that make any
    // skill-name match vacuous. `run_case` therefore feeds the scripted bytes through
    // `stdout_to_trace`. A double that only set `CaseRunOutput::stdout` would score 0.0 on
    // every call and silently stop testing the gate at all.
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
                cases: None,
            },
            CheckDefinition::TriggerExpectation {
                pattern: "M2".to_string(),
                expected: true,
                required: true,
                cases: None,
            },
        ]
    }

    /// Pops one scripted outcome per `run_case` call, in call order: initial score, then per
    /// step (rollout, gate). Panics on an exhausted queue — that means the test's assumed call
    /// count has drifted from the loop's actual behavior, which is itself worth surfacing loudly.
    struct ScriptedEvalRunner {
        outcomes: Mutex<VecDeque<ScriptedOutcome>>,
        calls: AtomicUsize,
        /// Every `CaseRunOptions.isolation` the loop handed us, in call order
        /// (spec 016 D7: proves the isolation field arrives at the scoring
        /// sites — without this capture, D7 is unenforced).
        captured_isolation: Mutex<Vec<aikit_evals::IsolationMode>>,
    }

    impl ScriptedEvalRunner {
        fn new(outcomes: Vec<ScriptedOutcome>) -> Self {
            Self {
                outcomes: Mutex::new(outcomes.into_iter().collect()),
                calls: AtomicUsize::new(0),
                captured_isolation: Mutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn captured_isolation(&self) -> Vec<aikit_evals::IsolationMode> {
            self.captured_isolation.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl EvalRunner for ScriptedEvalRunner {
        async fn run_case(
            &self,
            case: &EvalCase,
            opts: &CaseRunOptions,
            _checks: &[CheckDefinition],
        ) -> (CaseRunOutput, CaseResult, String) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.captured_isolation
                .lock()
                .unwrap()
                .push(opts.isolation.clone());
            let outcome = self.outcomes.lock().unwrap().pop_front().expect(
                "ScriptedEvalRunner queue exhausted — expected call count no longer matches",
            );
            let output = CaseRunOutput {
                stdout: outcome.stdout().to_vec(),
                stderr: vec![],
                exit_code: Some(0),
                timed_out: matches!(outcome, ScriptedOutcome::TimedOut),
                workspace: None,
                isolation: None,
                workspace_diff: None,
            };
            // Checks score the canonical trace, never raw stdout, so the double has to put
            // its markers where a real agent's output actually lands. `stdout_to_trace` is
            // the crate's own conversion, so this double exercises the real path rather than
            // a hand-rolled approximation of it.
            let trace_jsonl = trace_to_jsonl(&stdout_to_trace(outcome.stdout()));
            let result = CaseResult {
                id: case.id.clone(),
                status: CaseStatus::Passed,
                command_count: Some(0),
                input_tokens: None,
                output_tokens: None,
                check_results: vec![],
                error_message: None,
                cost_usd: None,
                exit_code: None,
                terminal: None,
                tokens: Default::default(),
                skill_path: None,
            };
            (output, result, trace_jsonl)
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
                    cost_usd: None,
                    exit_code: None,
                    terminal: None,
                    tokens: Default::default(),
                    skill_path: None,
                    judge_excluded: false,
                });
            }
            aggregate_trials(&case.id, trials, trial_count, opts.pass_threshold)
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
            extra: Default::default(),
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
            isolate: true,
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
            // Non-empty by construction: an empty check set is rejected at the boundary
            // (SKILLOPT_EMPTY_CHECKS) because it makes every score a vacuous 1.0.
            checks: score_markers(),
            config: make_config(),
            run_dir: dir.path().to_path_buf(),
        }
    }

    /// Like [`make_inputs`], but wired for [`ScriptedEvalRunner`]: `n_epochs` steps
    /// (batch=1, one train case) so the scripted queue maps one-to-one onto the loop's
    /// `run_case` calls.
    fn make_scripted_inputs(dir: &TempDir, n_epochs: u32) -> SkillOptInputs {
        let mut inputs = make_inputs(dir);
        inputs.config.n_epochs = n_epochs;
        inputs
    }

    // AC-7: train_skill runs end-to-end and returns Ok with best_artifact_path = best_skill.md.
    #[tokio::test]
    async fn test_train_skill_end_to_end() {
        let dir = TempDir::new().unwrap();
        let inputs = make_inputs(&dir);
        let result = train_skill(inputs, &AikitEvalRunner::new()).await;
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
        let outcome = train_skill(inputs, &AikitEvalRunner::new()).await.unwrap();
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
            score_markers(),
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

    // T2: an empty check set makes every item_score a vacuous 1.0, so best_score starts at
    // 1.0 and the gate (`> best + epsilon`) can never accept — the whole run would be a
    // silent no-op returning Ok. Both entry points must reject it up front.
    #[tokio::test]
    async fn test_train_skill_empty_checks_rejected() {
        let dir = TempDir::new().unwrap();
        let mut inputs = make_inputs(&dir);
        inputs.checks = vec![];
        // Validation fails before the runner is ever touched — empty queue proves it.
        let result = train_skill(inputs, &ScriptedEvalRunner::new(vec![])).await;
        assert!(result.is_err(), "empty checks must be rejected");
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("SKILLOPT_EMPTY_CHECKS"),
            "expected SKILLOPT_EMPTY_CHECKS in: {err}"
        );
    }

    #[tokio::test]
    async fn test_resume_skill_empty_checks_rejected() {
        let dir = TempDir::new().unwrap();
        let config = make_config();
        init_run_dir(dir.path(), &config).await.unwrap();
        let state = RuntimeState {
            config: config.clone(),
            epoch: config.n_epochs,
            step_in_epoch: 0,
            global_step: 1,
            best_score: 0.9,
            current_score: 0.9,
            rejected_edit_buffer: vec![],
            optimizer_strategy: "saved strategy".to_string(),
        };
        write_runtime_state(dir.path(), &state).await.unwrap();

        let result = resume_skill(
            dir.path().to_path_buf(),
            "# Test Skill\n\nOriginal.".to_string(),
            "test-skill".to_string(),
            vec![
                make_eval_case("train-1", &["train"]),
                make_eval_case("sel-1", &["selection"]),
            ],
            vec![],
            config,
            &ScriptedEvalRunner::new(vec![]),
        )
        .await;
        assert!(result.is_err(), "empty checks must be rejected on resume");
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("SKILLOPT_EMPTY_CHECKS"),
            "expected SKILLOPT_EMPTY_CHECKS in: {err}"
        );
    }

    // ---- F6 / T6: gate integrity for no-edit candidates via ScriptedEvalRunner ----
    //
    // The stub optimizer agent never produces a patch in tests, so every step's candidate
    // is byte-identical to the current text. The gate MUST therefore be skipped: the gate
    // agent is nondeterministic, and scoring an identical artifact could exceed
    // best_score + epsilon on pure noise, promoting best_score and ratcheting the
    // acceptance bar for an unchanged artifact. (The genuine accept path now requires a
    // real optimizer that produces edits; its decision arithmetic is unit-tested in
    // aikit-textgrad step.rs instead.)

    #[tokio::test]
    async fn test_no_edit_candidate_skips_gate_and_never_promotes_best_score() {
        use ScriptedOutcome::*;
        let dir = TempDir::new().unwrap();
        let inputs = make_scripted_inputs(&dir, 1);
        // Queue a would-be gate call scoring 1.0. Before the no-edit skip, the gate ran on
        // the identical artifact, saw 1.0 > 0.0 + epsilon, and "accepted" a non-change.
        let runner = ScriptedEvalRunner::new(vec![
            Score0, // initial score = 0.0
            Score1, // epoch0 rollout
            Score2, // would-be gate = 1.0 — must never be consumed
        ]);

        let outcome = train_skill(inputs, &runner).await.unwrap();
        assert!(
            (outcome.best_score - 0.0).abs() < 1e-9,
            "no-edit candidate must not promote best_score, got {}",
            outcome.best_score
        );
        assert_eq!(
            runner.call_count(),
            2,
            "gate must not run for a no-edit candidate (initial + rollout only)"
        );

        let history = read_history(&dir).await;
        assert_eq!(history.len(), 1);
        assert!(!history[0].accepted, "no-edit step must not be accepted");
        assert!(history[0].no_edit, "step must be recorded as no-edit");
    }

    #[tokio::test]
    async fn test_no_edit_steps_recorded_across_epochs() {
        use ScriptedOutcome::*;
        let dir = TempDir::new().unwrap();
        let inputs = make_scripted_inputs(&dir, 2);
        let runner = ScriptedEvalRunner::new(vec![
            Score1, // initial score = 0.5
            Score0, // epoch0 rollout (gate skipped: no edits)
            Score2, // epoch1 rollout (gate skipped: no edits)
        ]);

        let outcome = train_skill(inputs, &runner).await.unwrap();
        assert!(
            (outcome.best_score - 0.5).abs() < 1e-9,
            "best_score must stay at the initial 0.5, got {}",
            outcome.best_score
        );
        assert_eq!(runner.call_count(), 3, "initial + one rollout per epoch");

        let history = read_history(&dir).await;
        assert_eq!(history.len(), 2);
        assert!(
            history.iter().all(|r| !r.accepted && r.no_edit),
            "every no-edit step must be recorded as skipped and unaccepted"
        );
    }

    #[tokio::test]
    async fn test_timed_out_rollout_does_not_crash_loop() {
        use ScriptedOutcome::*;
        let dir = TempDir::new().unwrap();
        let inputs = make_scripted_inputs(&dir, 1);
        let runner = ScriptedEvalRunner::new(vec![
            Score1,   // initial score = 0.5
            TimedOut, // rollout times out -> empty stdout -> trajectory score 0.0
        ]);

        let outcome = train_skill(inputs, &runner).await.unwrap();
        assert!(
            (outcome.best_score - 0.5).abs() < 1e-9,
            "a timed-out rollout must not change best_score or crash the loop"
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
            isolation: aikit_evals::IsolationMode::Inherit,
            retain_workspace_in: None,
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
    async fn test_scripted_runner_call_count_matches_loop_calls() {
        use ScriptedOutcome::*;
        let dir = TempDir::new().unwrap();
        let inputs = make_scripted_inputs(&dir, 1);
        let runner = ScriptedEvalRunner::new(vec![Score1, Score1]);

        train_skill(inputs, &runner).await.unwrap();

        // 1 initial-score call + 1 rollout (1 train case, 1 selection case, gate_trials=1,
        // n_epochs=1). The gate is skipped because the stubbed optimizer applies no edits.
        assert_eq!(runner.call_count(), 2);
    }

    // ---- spec 016 D5/D7: isolation threads through the optimize loop ----

    /// spec 016 D7 enforcement: every scoring call the loop makes (initial
    /// score + rollout here; the gate shares the same `scoring_opts`
    /// construction path in aikit-textgrad and is skipped for no-edit
    /// candidates since #159) must arrive with `IsolationMode::Isolated`
    /// carrying the skill under test as an inline source.
    #[tokio::test]
    async fn test_isolation_mode_reaches_every_scoring_call() {
        use aikit_evals::{IsolationMode, SkillSource};
        use ScriptedOutcome::*;
        let dir = TempDir::new().unwrap();
        let inputs = make_scripted_inputs(&dir, 1);
        let runner = ScriptedEvalRunner::new(vec![Score1, Score1]);

        train_skill(inputs, &runner).await.unwrap();

        let captured = runner.captured_isolation();
        assert_eq!(captured.len(), 2, "initial score + one rollout");
        for (i, iso) in captured.iter().enumerate() {
            match iso {
                IsolationMode::Isolated { skill_name, source } => {
                    assert_eq!(skill_name, "test-skill", "call {i}");
                    match source {
                        SkillSource::Inline(text) => assert!(
                            text.contains("Test Skill"),
                            "call {i}: inline source must carry the artifact text"
                        ),
                        other => panic!("call {i}: expected Inline source, got {other:?}"),
                    }
                }
                IsolationMode::Inherit => {
                    panic!("call {i}: scoring call arrived WITHOUT isolation (spec 016 D7)")
                }
            }
        }
    }

    /// The opt-out (`isolate: false`, downstream --no-isolation) restores the
    /// legacy inherit behaviour at every scoring call.
    #[tokio::test]
    async fn test_isolate_false_scoring_calls_inherit() {
        use aikit_evals::IsolationMode;
        use ScriptedOutcome::*;
        let dir = TempDir::new().unwrap();
        let mut inputs = make_scripted_inputs(&dir, 1);
        inputs.config.isolate = false;
        let runner = ScriptedEvalRunner::new(vec![Score1, Score1]);

        train_skill(inputs, &runner).await.unwrap();

        let captured = runner.captured_isolation();
        assert_eq!(captured.len(), 2);
        assert!(
            captured.iter().all(|iso| *iso == IsolationMode::Inherit),
            "isolate: false must restore legacy Inherit everywhere: {captured:?}"
        );
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
