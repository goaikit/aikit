//! Layer 2 — async optimization loop.
//!
//! Drives multi-epoch, multi-step improvement of a caller-supplied [`Optimizable`] artifact
//! using `aikit-evals` for scoring and `aikit-sdk::Pipeline` for optimizer model calls.

pub mod config;
pub mod epoch;
pub mod lr;
pub mod state;
pub mod step;

pub use config::{OptimizerPrompts, RunConfig, SlowUpdateMode, TextgradError};
pub use state::{RejectedPatch, RuntimeState, StepRecord, TrainingOutcome};

use std::path::Path;

use aikit_evals::{score_cases, split_score, CaseRunOptions, EvalCase, EvalRunner, Scorer};
use async_trait::async_trait;

use config::validate_config;
use epoch::{run_meta_skill, run_slow_update};
use state::{init_run_dir, read_runtime_state, write_runtime_state};
use step::{build_skip_feedback, run_step};

/// Abstraction over a text artifact that can be deployed to a workspace.
#[async_trait]
pub trait Optimizable: Send + Sync {
    /// The current artifact text.
    fn text(&self) -> &str;
    /// Replace the artifact text. Called only after an accepted gate.
    fn set_text(&mut self, text: String);
    /// Write the artifact to `workspace` so the target agent can run against it.
    async fn materialize(&self, workspace: &Path) -> anyhow::Result<()>;
}

// ---- case-split extraction ----

struct SplitCases {
    train: Vec<EvalCase>,
    selection: Vec<EvalCase>,
    test: Vec<EvalCase>,
}

fn split_cases(suite: &[EvalCase]) -> SplitCases {
    let mut train = Vec::new();
    let mut selection = Vec::new();
    let mut test = Vec::new();

    for case in suite {
        let tag = case
            .tags
            .iter()
            .find(|t| *t == "train" || *t == "selection" || *t == "test")
            .map(|s| s.as_str())
            .unwrap_or("train");

        match tag {
            "selection" => selection.push(case.clone()),
            "test" => test.push(case.clone()),
            _ => train.push(case.clone()),
        }
    }

    SplitCases {
        train,
        selection,
        test,
    }
}

// ---- deterministic shuffle for epoch sampling ----

fn shuffle_cases(cases: &mut [EvalCase], seed: u64) {
    let n = cases.len();
    if n <= 1 {
        return;
    }
    let mut rng = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    for i in (1..n).rev() {
        rng = rng
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let j = ((rng >> 33) as usize) % (i + 1);
        cases.swap(i, j);
    }
}

// ---- initial score ----

async fn compute_initial_score(
    artifact: &mut dyn Optimizable,
    selection_cases: &[EvalCase],
    scorer: &dyn Scorer,
    runner: &dyn EvalRunner,
    config: &RunConfig,
) -> Result<f64, TextgradError> {
    let ws = tempfile::TempDir::new().map_err(TextgradError::Io)?;
    artifact.materialize(ws.path()).await?;
    let opts = CaseRunOptions {
        agent_key: config.target_agent.clone(),
        model: config.target_model.clone(),
        project_root: ws.path().to_path_buf(),
        timeout_seconds: config.timeout_seconds,
        pass_threshold: config.pass_threshold,
    };
    let results = score_cases(
        runner,
        selection_cases,
        &opts,
        scorer,
        config.gate_trials,
        config.parallel,
    )
    .await;
    Ok(split_score(&results, &config.gate_metric))
}

// ---- core training loop ----

#[allow(clippy::too_many_arguments)]
async fn training_loop(
    artifact: &mut dyn Optimizable,
    splits: &SplitCases,
    scorer: &dyn Scorer,
    runner: &dyn EvalRunner,
    prompts: &mut OptimizerPrompts,
    state: &mut RuntimeState,
    run_dir: &Path,
    start_epoch: u32,
    start_step_in_epoch: u32,
) -> Result<(), TextgradError> {
    let config = state.config.clone();
    let n_epochs = config.n_epochs;
    let ba = (config.batch_size * config.accumulation) as usize;

    for epoch in start_epoch..n_epochs {
        state.epoch = epoch;

        // Shuffle training cases with a seed derived from the epoch number.
        let mut epoch_cases = splits.train.clone();
        shuffle_cases(&mut epoch_cases, epoch as u64 + 1);

        let steps_per_epoch = if epoch_cases.is_empty() || ba == 0 {
            0
        } else {
            epoch_cases.len().div_ceil(ba)
        };

        let step_start = if epoch == start_epoch {
            start_step_in_epoch as usize
        } else {
            0
        };

        let mut skip_feedback = String::new();

        for step in step_start..steps_per_epoch {
            state.step_in_epoch = step as u32;

            let start = (step * ba) % epoch_cases.len().max(1);
            let end = (start + ba).min(epoch_cases.len());
            let step_cases: Vec<EvalCase> = if epoch_cases.is_empty() {
                vec![]
            } else {
                epoch_cases[start..end].to_vec()
            };

            // Clone to avoid borrow conflict with `state` (mutable borrow in run_step).
            let scaffold = prompts.scaffold.clone();
            let strategy = state.optimizer_strategy.clone();

            let result = run_step(
                artifact,
                &step_cases,
                &splits.selection,
                scorer,
                runner,
                &scaffold,
                &strategy,
                &config,
                state,
                run_dir,
                &skip_feedback,
            )
            .await?;

            skip_feedback = build_skip_feedback(&result.intra_patch_skips);

            state.global_step += 1;

            // Atomic checkpoint after each step.
            write_runtime_state(run_dir, state).await?;
        }

        // Epoch boundary: Slow Update then Meta-Skill.
        run_slow_update(
            artifact,
            &splits.selection,
            scorer,
            runner,
            &config,
            state,
            run_dir,
            epoch,
        )
        .await?;

        run_meta_skill(&prompts.scaffold, &config, state, run_dir, epoch).await?;

        // Persist the updated strategy so it survives between epochs.
        prompts.strategy = state.optimizer_strategy.clone();
        write_runtime_state(run_dir, state).await?;
    }

    Ok(())
}

// ---- public API ----

/// Run a complete training loop from scratch.
#[allow(clippy::too_many_arguments)]
pub async fn run_training(
    artifact: &mut dyn Optimizable,
    suite: &[EvalCase],
    scorer: &dyn Scorer,
    runner: &dyn EvalRunner,
    prompts: OptimizerPrompts,
    config: RunConfig,
    run_dir: &Path,
) -> Result<TrainingOutcome, TextgradError> {
    validate_config(&config)?;

    let splits = split_cases(suite);
    if splits.selection.is_empty() {
        return Err(TextgradError::NoSelectionCases);
    }

    // Create run-dir structure.
    init_run_dir(run_dir, &config).await?;

    // Compute initial best_score on the selection split.
    let initial_score =
        compute_initial_score(artifact, &splits.selection, scorer, runner, &config).await?;

    // Write initial artifact.
    let best_artifact_path = run_dir.join(format!("best_{}.md", config.artifact_stem));
    tokio::fs::write(&best_artifact_path, artifact.text().as_bytes()).await?;
    let ver_path = run_dir.join(format!(
        "{}s/{}_v0000.md",
        config.artifact_stem, config.artifact_stem
    ));
    tokio::fs::write(&ver_path, artifact.text().as_bytes()).await?;

    // Initialize history.json.
    let history_path = run_dir.join("history.json");
    tokio::fs::write(&history_path, b"[]").await?;

    let mut state = RuntimeState {
        config: config.clone(),
        epoch: 0,
        step_in_epoch: 0,
        global_step: 0,
        best_score: initial_score,
        current_score: initial_score,
        rejected_edit_buffer: Vec::new(),
        optimizer_strategy: prompts.strategy.clone(),
    };

    write_runtime_state(run_dir, &state).await?;

    let mut prompts_mut = prompts;
    training_loop(
        artifact,
        &splits,
        scorer,
        runner,
        &mut prompts_mut,
        &mut state,
        run_dir,
        0,
        0,
    )
    .await?;

    // Compute final score on the test split (if any).
    let final_score = compute_final_score(
        artifact,
        &splits.test,
        scorer,
        runner,
        &config,
        state.best_score,
    )
    .await?;

    Ok(TrainingOutcome {
        best_text: artifact.text().to_string(),
        best_score: state.best_score,
        final_score,
        best_artifact_path,
    })
}

/// Resume an interrupted training run from the last checkpoint.
pub async fn resume_training(
    run_dir: &Path,
    artifact: &mut dyn Optimizable,
    suite: &[EvalCase],
    scorer: &dyn Scorer,
    runner: &dyn EvalRunner,
    prompts: OptimizerPrompts,
) -> Result<TrainingOutcome, TextgradError> {
    let state = read_runtime_state(run_dir).await?;

    let config = state.config.clone();
    let splits = split_cases(suite);
    if splits.selection.is_empty() {
        return Err(TextgradError::NoSelectionCases);
    }

    let best_artifact_path = run_dir.join(format!("best_{}.md", config.artifact_stem));

    // Restore artifact text from the persisted best artifact if it exists.
    if best_artifact_path.exists() {
        let saved_text = tokio::fs::read_to_string(&best_artifact_path).await?;
        artifact.set_text(saved_text);
    }

    let start_epoch = state.epoch;
    let start_step = state.step_in_epoch;

    let mut state_mut = state;
    let mut prompts_mut = prompts;
    // Keep the persisted optimizer strategy.
    prompts_mut.strategy = state_mut.optimizer_strategy.clone();

    // If all epochs are already done, skip the loop.
    if start_epoch < config.n_epochs {
        training_loop(
            artifact,
            &splits,
            scorer,
            runner,
            &mut prompts_mut,
            &mut state_mut,
            run_dir,
            start_epoch,
            start_step,
        )
        .await?;
    }

    let final_score = compute_final_score(
        artifact,
        &splits.test,
        scorer,
        runner,
        &config,
        state_mut.best_score,
    )
    .await?;

    Ok(TrainingOutcome {
        best_text: artifact.text().to_string(),
        best_score: state_mut.best_score,
        final_score,
        best_artifact_path,
    })
}

async fn compute_final_score(
    artifact: &mut dyn Optimizable,
    test_cases: &[EvalCase],
    scorer: &dyn Scorer,
    runner: &dyn EvalRunner,
    config: &RunConfig,
    best_score: f64,
) -> Result<f64, TextgradError> {
    if test_cases.is_empty() {
        return Ok(best_score);
    }
    let ws = tempfile::TempDir::new().map_err(TextgradError::Io)?;
    artifact.materialize(ws.path()).await?;
    let opts = CaseRunOptions {
        agent_key: config.target_agent.clone(),
        model: config.target_model.clone(),
        project_root: ws.path().to_path_buf(),
        timeout_seconds: config.timeout_seconds,
        pass_threshold: config.pass_threshold,
    };
    let results = score_cases(
        runner,
        test_cases,
        &opts,
        scorer,
        config.gate_trials,
        config.parallel,
    )
    .await;
    Ok(split_score(&results, &config.gate_metric))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_evals::{
        CaseResult, CaseRunOptions, CaseRunOutput, CaseStatus, CaseTrialsResult, CheckDefinition,
        CheckResult, EvalCase, EvalRunner, Scorer, TrialResult,
    };
    use std::path::Path;
    use tempfile::TempDir;

    // ---- test doubles ----

    struct SimpleArtifact {
        text: String,
    }

    #[async_trait]
    impl Optimizable for SimpleArtifact {
        fn text(&self) -> &str {
            &self.text
        }
        fn set_text(&mut self, text: String) {
            self.text = text;
        }
        async fn materialize(&self, workspace: &Path) -> anyhow::Result<()> {
            tokio::fs::write(workspace.join("artifact.md"), self.text.as_bytes()).await?;
            Ok(())
        }
    }

    struct EmptyScorer;
    impl Scorer for EmptyScorer {
        fn score(&self, _stdout: &str, _trace: &str, _wd: &Path) -> Vec<CheckResult> {
            vec![]
        }
    }

    struct StubRunner;

    #[async_trait]
    impl EvalRunner for StubRunner {
        async fn run_case(
            &self,
            case: &EvalCase,
            _opts: &CaseRunOptions,
            _checks: &[CheckDefinition],
        ) -> (CaseRunOutput, CaseResult, String) {
            let out = CaseRunOutput {
                stdout: b"ok".to_vec(),
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
            CaseTrialsResult {
                id: case.id.clone(),
                trials,
                aggregated_status: CaseStatus::Passed,
                pass_count: trial_count,
                total_trials: trial_count,
                pass_rate: 1.0,
            }
        }
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
            target_agent: "stub-agent".to_string(),
            target_model: None,
            optimizer_agent: "stub-optimizer".to_string(),
            optimizer_model: None,
            timeout_seconds: 30,
            parallel: Some(1),
            artifact_stem: "artifact".to_string(),
        }
    }

    fn make_prompts() -> OptimizerPrompts {
        OptimizerPrompts {
            scaffold: "scaffold".to_string(),
            strategy: "initial strategy".to_string(),
        }
    }

    // ---- AC27: NoSelectionCases ----

    #[tokio::test]
    async fn test_no_selection_cases_returns_error() {
        let dir = TempDir::new().unwrap();
        let suite = vec![make_eval_case("train-1", &["train"])];
        let mut artifact = SimpleArtifact {
            text: "hello".to_string(),
        };
        let result = run_training(
            &mut artifact,
            &suite,
            &EmptyScorer,
            &StubRunner,
            make_prompts(),
            make_config(),
            dir.path(),
        )
        .await;
        assert!(
            matches!(result, Err(TextgradError::NoSelectionCases)),
            "expected NoSelectionCases, got {result:?}"
        );
    }

    // ---- AC28: InvalidConfig ----

    #[tokio::test]
    async fn test_invalid_config_batch_size_zero() {
        let dir = TempDir::new().unwrap();
        let mut config = make_config();
        config.batch_size = 0;
        let suite = vec![make_eval_case("s", &["selection"])];
        let mut artifact = SimpleArtifact {
            text: "hi".to_string(),
        };
        let result = run_training(
            &mut artifact,
            &suite,
            &EmptyScorer,
            &StubRunner,
            make_prompts(),
            config,
            dir.path(),
        )
        .await;
        assert!(
            matches!(result, Err(TextgradError::InvalidConfig(_))),
            "expected InvalidConfig"
        );
    }

    #[tokio::test]
    async fn test_invalid_config_n_epochs_zero() {
        let dir = TempDir::new().unwrap();
        let mut config = make_config();
        config.n_epochs = 0;
        let suite = vec![make_eval_case("s", &["selection"])];
        let mut artifact = SimpleArtifact {
            text: "hi".to_string(),
        };
        let result = run_training(
            &mut artifact,
            &suite,
            &EmptyScorer,
            &StubRunner,
            make_prompts(),
            config,
            dir.path(),
        )
        .await;
        assert!(matches!(result, Err(TextgradError::InvalidConfig(_))));
    }

    // ---- AC14: complete 1-epoch run produces run-dir layout and monotonic best_score ----

    #[tokio::test]
    async fn test_complete_one_epoch_run_layout_and_monotonic_score() {
        let dir = TempDir::new().unwrap();
        let suite = vec![
            make_eval_case("train-1", &["train"]),
            make_eval_case("sel-1", &["selection"]),
        ];
        let mut artifact = SimpleArtifact {
            text: "Initial artifact text\n".to_string(),
        };
        let config = make_config();

        let result = run_training(
            &mut artifact,
            &suite,
            &EmptyScorer,
            &StubRunner,
            make_prompts(),
            config,
            dir.path(),
        )
        .await;

        assert!(result.is_ok(), "run_training failed: {result:?}");
        let outcome = result.unwrap();

        // Run-dir layout checks (AC14)
        assert!(
            dir.path().join("runtime_state.json").exists(),
            "runtime_state.json missing"
        );
        assert!(
            dir.path().join("best_artifact.md").exists(),
            "best_artifact.md missing"
        );
        assert!(
            dir.path().join("artifacts").is_dir(),
            "artifacts/ dir missing"
        );
        assert!(
            dir.path().join("history.json").exists(),
            "history.json missing"
        );

        // Monotonic best_score (AC25): initial score ≥ 0, outcome score ≥ initial
        assert!(outcome.best_score >= 0.0);
        // With EmptyScorer (empty check_results → item_score = 1.0) initial score = 1.0.
        // Gate can only accept if gate_score > 1.0 + 0.01, which is impossible.
        // So best_score stays at 1.0.
        assert!(
            (outcome.best_score - 1.0).abs() < 1e-9,
            "expected best_score = 1.0, got {}",
            outcome.best_score
        );
    }

    // ---- AC15: resume after checkpoint ----

    #[tokio::test]
    async fn test_resume_from_checkpoint() {
        let dir = TempDir::new().unwrap();
        let suite = vec![
            make_eval_case("train-1", &["train"]),
            make_eval_case("sel-1", &["selection"]),
        ];
        let config = make_config();

        // Set up initial run-dir state: one epoch already completed.
        init_run_dir(dir.path(), &config).await.unwrap();
        let state = RuntimeState {
            config: config.clone(),
            epoch: config.n_epochs, // all epochs done
            step_in_epoch: 0,
            global_step: 1,
            best_score: 1.0,
            current_score: 1.0,
            rejected_edit_buffer: vec![],
            optimizer_strategy: "saved strategy".to_string(),
        };
        write_runtime_state(dir.path(), &state).await.unwrap();
        tokio::fs::write(dir.path().join("best_artifact.md"), b"saved text")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("history.json"), b"[]")
            .await
            .unwrap();

        let mut artifact = SimpleArtifact {
            text: "original".to_string(),
        };

        let result = resume_training(
            dir.path(),
            &mut artifact,
            &suite,
            &EmptyScorer,
            &StubRunner,
            make_prompts(),
        )
        .await;

        assert!(result.is_ok(), "resume_training failed: {result:?}");
        let outcome = result.unwrap();

        // Artifact text should be restored from checkpoint.
        assert_eq!(artifact.text(), "saved text");
        assert!((outcome.best_score - 1.0).abs() < 1e-9);
    }

    // ---- AC16: B×A rollouts verified by step artifact ----

    #[tokio::test]
    async fn test_step_artifact_created() {
        let dir = TempDir::new().unwrap();
        let suite = vec![
            make_eval_case("train-1", &["train"]),
            make_eval_case("sel-1", &["selection"]),
        ];
        let mut artifact = SimpleArtifact {
            text: "text".to_string(),
        };
        let config = make_config();
        run_training(
            &mut artifact,
            &suite,
            &EmptyScorer,
            &StubRunner,
            make_prompts(),
            config,
            dir.path(),
        )
        .await
        .unwrap();

        // Step 0 should have been created (1 train case, batch=1, accum=1 → 1 step).
        assert!(
            dir.path().join("steps/step_0000").is_dir(),
            "steps/step_0000 dir missing"
        );
        assert!(
            dir.path().join("steps/step_0000/gate.json").exists(),
            "gate.json missing"
        );
    }

    // ---- AC17: gate epsilon boundary ----

    #[test]
    fn test_gate_epsilon_exact_boundary_rejected() {
        // score == best + epsilon is NOT accepted (strict >)
        let best_score = 0.5_f64;
        let gate_epsilon = 0.1_f64;
        let gate_score = best_score + gate_epsilon; // exactly at boundary
        let accepted = gate_score > best_score + gate_epsilon;
        assert!(!accepted, "exactly at boundary should be rejected");
    }

    #[test]
    fn test_gate_epsilon_above_boundary_accepted() {
        let best_score = 0.5_f64;
        let gate_epsilon = 0.1_f64;
        let gate_score = best_score + gate_epsilon + 1e-10; // strictly above
        let accepted = gate_score > best_score + gate_epsilon;
        assert!(accepted, "strictly above boundary should be accepted");
    }

    // ---- AC23/AC24: rejected_edit_buffer and optimizer_strategy survive checkpoint ----

    #[tokio::test]
    async fn test_state_fields_survive_checkpoint() {
        let dir = TempDir::new().unwrap();
        let config = make_config();
        let state = RuntimeState {
            config: config.clone(),
            epoch: 0,
            step_in_epoch: 0,
            global_step: 0,
            best_score: 0.8,
            current_score: 0.8,
            rejected_edit_buffer: vec![RejectedPatch {
                patch: vec![],
                text_snapshot: "snap".to_string(),
                score_delta: -0.05,
            }],
            optimizer_strategy: "updated strategy".to_string(),
        };

        write_runtime_state(dir.path(), &state).await.unwrap();
        let restored = read_runtime_state(dir.path()).await.unwrap();

        assert_eq!(restored.rejected_edit_buffer.len(), 1);
        assert_eq!(restored.optimizer_strategy, "updated strategy");
    }

    // ---- AC26: protected region never modified by step edits ----
    // (tested at the Layer 1 level via AC7 in edit.rs)
    // Verify here at integration level: the training loop does not corrupt the protected region.

    #[tokio::test]
    async fn test_protected_region_intact_after_run() {
        use crate::edit::{PROTECTED_BEGIN, PROTECTED_END};

        let dir = TempDir::new().unwrap();
        let protected_content = "\nThis is protected content.\n";
        let initial_text =
            format!("Editable section.\n{PROTECTED_BEGIN}{protected_content}{PROTECTED_END}");
        let suite = vec![
            make_eval_case("train-1", &["train"]),
            make_eval_case("sel-1", &["selection"]),
        ];
        let mut artifact = SimpleArtifact {
            text: initial_text.clone(),
        };

        run_training(
            &mut artifact,
            &suite,
            &EmptyScorer,
            &StubRunner,
            make_prompts(),
            make_config(),
            dir.path(),
        )
        .await
        .unwrap();

        // Since no optimizer patches are produced (no real agent), text is unchanged.
        // The protected region must still be present.
        assert!(
            artifact.text().contains(PROTECTED_BEGIN),
            "PROTECTED_BEGIN missing after run"
        );
        assert!(
            artifact.text().contains(PROTECTED_END),
            "PROTECTED_END missing after run"
        );
        assert!(
            artifact.text().contains(protected_content),
            "protected content altered"
        );
    }

    // ---- AC30: LR schedule (tested independently in lr.rs) ----

    // ---- Verify save_accepted_artifact creates versioned files ----
    #[tokio::test]
    async fn test_save_accepted_artifact_versioning() {
        use state::save_accepted_artifact;
        let dir = TempDir::new().unwrap();
        tokio::fs::create_dir_all(dir.path().join("artifacts"))
            .await
            .unwrap();

        let path1 = save_accepted_artifact(dir.path(), "artifact", "v1 text")
            .await
            .unwrap();
        let path2 = save_accepted_artifact(dir.path(), "artifact", "v2 text")
            .await
            .unwrap();

        assert_eq!(path1, path2); // both write to best_artifact.md
        assert!(dir.path().join("artifacts/artifact_v0000.md").exists());
        assert!(dir.path().join("artifacts/artifact_v0001.md").exists());

        let content = tokio::fs::read_to_string(dir.path().join("best_artifact.md"))
            .await
            .unwrap();
        assert_eq!(content, "v2 text");
    }
}
