pub mod artifact;
pub mod prompts;

pub use aikit_textgrad::training::{RunConfig, TrainingOutcome};
pub use artifact::SkillArtifact;
pub use prompts::skill_prompts;

use std::path::PathBuf;

use aikit_evals::{AikitEvalRunner, CheckDefinition, ChecksScorer, EvalCase};
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
pub async fn train_skill(inputs: SkillOptInputs) -> anyhow::Result<TrainingOutcome> {
    let mut artifact = SkillArtifact::from_existing(
        inputs.initial_skill_md,
        inputs.skill_name,
        inputs.config.target_agent.clone(),
    );
    let scorer = ChecksScorer {
        checks: inputs.checks,
    };
    let runner = AikitEvalRunner;
    run_training(
        &mut artifact,
        &inputs.suite,
        &scorer,
        &runner,
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
pub async fn resume_skill(
    run_dir: PathBuf,
    initial_skill_md: String,
    skill_name: String,
    suite: Vec<EvalCase>,
    checks: Vec<CheckDefinition>,
    config: RunConfig,
) -> anyhow::Result<TrainingOutcome> {
    let mut artifact =
        SkillArtifact::from_existing(initial_skill_md, skill_name, config.target_agent.clone());
    let scorer = ChecksScorer { checks };
    let runner = AikitEvalRunner;
    resume_training(
        &run_dir,
        &mut artifact,
        &suite,
        &scorer,
        &runner,
        skill_prompts(),
    )
    .await
    .map_err(anyhow::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_evals::EvalCase;
    use aikit_textgrad::training::state::{init_run_dir, write_runtime_state, RuntimeState};
    use aikit_textgrad::training::SlowUpdateMode;
    use tempfile::TempDir;

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
            // These are orchestration smoke tests: they verify train_skill/resume_skill wire
            // up state, checkpoints, and best_skill.md — NOT a live optimization against a real
            // agent. `windsurf` supports skill deployment (`skills: Some(".windsurf/skills")`, so
            // materialize resolves a skill dir) but is NOT a runnable Backend, so the eval runner
            // fails fast without spawning any subprocess: the loop is a deterministic no-op and
            // best_skill.md == best_text. Previously these used "cursor-agent", which passed for
            // the same reason (skill-capable, not runnable); ARCH-2's canonical "cursor" key
            // turned them into slow (~10min), nondeterministic real-CLI runs. A follow-up should
            // give train_skill an injectable mock EvalRunner for genuine loop coverage.
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

    // AC-7: train_skill runs end-to-end and returns Ok with best_artifact_path = best_skill.md.
    #[tokio::test]
    async fn test_train_skill_end_to_end() {
        let dir = TempDir::new().unwrap();
        let inputs = make_inputs(&dir);
        let result = train_skill(inputs).await;
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
        let outcome = train_skill(inputs).await.unwrap();
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

        let result = resume_skill(
            dir.path().to_path_buf(),
            "# Test Skill\n\nOriginal.".to_string(),
            "test-skill".to_string(),
            suite,
            vec![],
            config,
        )
        .await;

        assert!(result.is_ok(), "resume_skill failed: {result:?}");
        let outcome = result.unwrap();
        assert!(
            (outcome.best_score - 0.9).abs() < 1e-9,
            "expected best_score 0.9, got {}",
            outcome.best_score
        );
    }

    // AC-12: train_skill with zero selection cases returns TEXTGRAD_NO_SELECTION_CASES error.
    #[tokio::test]
    async fn test_train_skill_no_selection_cases() {
        let dir = TempDir::new().unwrap();
        let mut inputs = make_inputs(&dir);
        inputs.suite = vec![make_eval_case("train-1", &["train"])];
        let result = train_skill(inputs).await;
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
        let result = train_skill(inputs).await;
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("TEXTGRAD_INVALID_CONFIG"),
            "expected TEXTGRAD_INVALID_CONFIG in: {err}"
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
