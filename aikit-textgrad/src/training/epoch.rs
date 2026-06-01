//! Epoch-boundary passes: Slow Update and Meta-Skill.

use std::path::Path;

use aikit_evals::{score_cases, split_score, CaseRunOptions, EvalCase, EvalRunner, Scorer};
use aikit_sdk::{AgentRunner, Pipeline};

use crate::edit::{PROTECTED_BEGIN, PROTECTED_END};
use crate::training::config::{RunConfig, SlowUpdateMode, TextgradError};
use crate::training::state::{ensure_epoch_dir, RuntimeState};
use crate::training::Optimizable;

const PROTECTED_REGION_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["protected_region"],
  "properties": {
    "protected_region": {"type": "string"}
  }
}"#;

const META_SKILL_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["strategy"],
  "properties": {
    "strategy": {"type": "string"}
  }
}"#;

fn extract_protected_content(doc: &str) -> &str {
    let begin_end = doc
        .find(PROTECTED_BEGIN)
        .map(|p| p + PROTECTED_BEGIN.len())
        .unwrap_or(doc.len());
    let end_start = doc.find(PROTECTED_END).unwrap_or(doc.len());
    if begin_end <= end_start {
        &doc[begin_end..end_start]
    } else {
        ""
    }
}

fn replace_protected_content(doc: &str, new_content: &str) -> String {
    if let (Some(begin_pos), Some(end_start)) = (doc.find(PROTECTED_BEGIN), doc.find(PROTECTED_END))
    {
        let end_pos = end_start + PROTECTED_END.len();
        let mut result = doc[..begin_pos].to_string();
        result.push_str(PROTECTED_BEGIN);
        result.push_str(new_content);
        result.push_str(PROTECTED_END);
        if end_pos < doc.len() {
            result.push_str(&doc[end_pos..]);
        }
        result
    } else {
        doc.to_string()
    }
}

async fn call_pipeline_string(
    prompt: String,
    schema: &'static str,
    agent_key: String,
    model: Option<String>,
) -> Option<serde_json::Value> {
    let result = tokio::task::spawn_blocking(move || {
        let runner = AgentRunner::new().agent(&agent_key);
        let runner = if let Some(ref m) = model {
            runner.model(m)
        } else {
            runner
        };
        Pipeline::new(prompt, schema)
            .max_retries(2)
            .run(&[], runner)
    })
    .await;

    match result {
        Ok(Ok(pr)) => Some(pr.data),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_slow_update(
    artifact: &mut dyn Optimizable,
    selection_cases: &[EvalCase],
    scorer: &dyn Scorer,
    runner: &dyn EvalRunner,
    config: &RunConfig,
    state: &mut RuntimeState,
    run_dir: &Path,
    epoch: u32,
) -> Result<(), TextgradError> {
    let current_text = artifact.text().to_string();
    let protected_content = extract_protected_content(&current_text);

    let rejected_summary: Vec<serde_json::Value> = state
        .rejected_edit_buffer
        .iter()
        .map(|rp| {
            serde_json::json!({
                "score_delta": rp.score_delta,
                "patch_size": rp.patch.len(),
            })
        })
        .collect();

    let prompt = format!(
        "You are revising the protected region of a skill document.\n\
        Current protected region:\n{protected_content}\n\n\
        Rejected patches this epoch ({}): {}\n\n\
        Soft character cap: {} chars.\n\
        Return the revised protected region as a JSON object with key 'protected_region'.",
        state.rejected_edit_buffer.len(),
        serde_json::to_string(&rejected_summary).unwrap_or_default(),
        config.protected_soft_cap_chars,
    );

    let data = call_pipeline_string(
        prompt,
        PROTECTED_REGION_SCHEMA,
        config.optimizer_agent.clone(),
        config.optimizer_model.clone(),
    )
    .await;

    // Always clear the rejected_edit_buffer after Slow Update.
    state.rejected_edit_buffer.clear();

    let epoch_dir = ensure_epoch_dir(run_dir, epoch).await?;

    if let Some(val) = data {
        if let Some(new_content) = val["protected_region"].as_str() {
            let candidate_text = replace_protected_content(&current_text, new_content);

            let accepted = match &config.slow_update_mode {
                SlowUpdateMode::ForceAccept => true,
                SlowUpdateMode::Gated => {
                    // Score the candidate on the selection split.
                    let original_text = artifact.text().to_string();
                    artifact.set_text(candidate_text.clone());
                    let gate_ws = tempfile::TempDir::new().map_err(TextgradError::Io)?;
                    let mat_result = artifact.materialize(gate_ws.path()).await;
                    if mat_result.is_err() {
                        artifact.set_text(original_text.clone());
                        mat_result?;
                    }
                    let gate_opts = CaseRunOptions {
                        agent_key: config.target_agent.clone(),
                        model: config.target_model.clone(),
                        project_root: gate_ws.path().to_path_buf(),
                        timeout_seconds: config.timeout_seconds,
                        pass_threshold: config.pass_threshold,
                    };
                    let results = score_cases(
                        runner,
                        selection_cases,
                        &gate_opts,
                        scorer,
                        config.gate_trials,
                        config.parallel,
                    )
                    .await;
                    let score = split_score(&results, &config.gate_metric);
                    let passes = score > state.best_score + config.gate_epsilon;
                    if !passes {
                        artifact.set_text(original_text);
                    }
                    passes
                }
            };

            if accepted {
                artifact.set_text(candidate_text.clone());
            }

            let slow_update_info = serde_json::json!({
                "accepted": accepted,
                "mode": format!("{:?}", config.slow_update_mode),
            });
            tokio::fs::write(
                epoch_dir.join("slow_update.json"),
                serde_json::to_vec_pretty(&slow_update_info).unwrap_or_default(),
            )
            .await?;
        } else {
            write_slow_update_noop(&epoch_dir).await?;
        }
    } else {
        write_slow_update_noop(&epoch_dir).await?;
    }

    Ok(())
}

async fn write_slow_update_noop(epoch_dir: &Path) -> Result<(), std::io::Error> {
    let info = serde_json::json!({"accepted": false, "reason": "pipeline_noop"});
    tokio::fs::write(
        epoch_dir.join("slow_update.json"),
        serde_json::to_vec_pretty(&info).unwrap_or_default(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_meta_skill(
    scaffold: &str,
    config: &RunConfig,
    state: &mut RuntimeState,
    run_dir: &Path,
    epoch: u32,
) -> Result<(), TextgradError> {
    let prompt = format!(
        "You are the Meta-Skill optimizer. Review the training history and revise the strategy.\n\
        Scaffold (immutable — do not change):\n{scaffold}\n\n\
        Current strategy:\n{}\n\n\
        Return only the revised strategy as JSON: {{\"strategy\": \"...\"}}",
        state.optimizer_strategy,
    );

    let data = call_pipeline_string(
        prompt,
        META_SKILL_SCHEMA,
        config.optimizer_agent.clone(),
        config.optimizer_model.clone(),
    )
    .await;

    let epoch_dir = ensure_epoch_dir(run_dir, epoch).await?;

    if let Some(val) = data {
        if let Some(new_strategy) = val["strategy"].as_str() {
            // Only update the strategy; scaffold is immutable.
            state.optimizer_strategy = new_strategy.to_string();
            let meta_info = serde_json::json!({
                "epoch": epoch,
                "strategy": new_strategy,
            });
            tokio::fs::write(
                epoch_dir.join("meta_skill.json"),
                serde_json::to_vec_pretty(&meta_info).unwrap_or_default(),
            )
            .await?;
        } else {
            write_meta_skill_noop(&epoch_dir, epoch).await?;
        }
    } else {
        write_meta_skill_noop(&epoch_dir, epoch).await?;
    }

    Ok(())
}

async fn write_meta_skill_noop(epoch_dir: &Path, epoch: u32) -> Result<(), std::io::Error> {
    let info = serde_json::json!({"epoch": epoch, "reason": "pipeline_noop"});
    tokio::fs::write(
        epoch_dir.join("meta_skill.json"),
        serde_json::to_vec_pretty(&info).unwrap_or_default(),
    )
    .await
}
