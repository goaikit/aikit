//! Per-step pipeline: ROLLOUT → REFLECT → AGGREGATE → SELECT → UPDATE → GATE.

use std::path::Path;

use aikit_evals::{
    item_score, score_cases, split_score, CaseRunOptions, EvalCase, EvalRunner, Scorer,
};
use aikit_sdk::{AgentRunner, Pipeline};

use crate::edit::{apply_budgeted, Edit, Patch, SkipRecord};
use crate::training::config::{RunConfig, TextgradError};
use crate::training::lr::compute_lr;
use crate::training::state::{
    append_history, ensure_step_dir, save_accepted_artifact, sha256_hex, RejectedPatch,
    RuntimeState, StepRecord,
};
use crate::training::Optimizable;

const PATCH_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["edits"],
  "properties": {
    "edits": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["op", "impact"],
        "properties": {
          "op": {"enum": ["append", "insert_after", "replace", "delete"]},
          "target": {"type": "string"},
          "content": {"type": "string"},
          "impact": {"type": "number", "minimum": 0, "maximum": 1}
        }
      }
    }
  }
}"#;

struct Trajectory {
    case_id: String,
    stdout: String,
    trace_jsonl: String,
    score: f64,
}

pub(super) struct StepResult {
    pub intra_patch_skips: Vec<SkipRecord>,
}

fn parse_patch_from_value(data: &serde_json::Value) -> Patch {
    data["edits"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

async fn call_optimizer_pipeline(
    prompt: String,
    agent_key: String,
    model: Option<String>,
) -> Patch {
    let result = tokio::task::spawn_blocking(move || {
        let runner = AgentRunner::new().agent(&agent_key);
        let runner = if let Some(ref m) = model {
            runner.model(m)
        } else {
            runner
        };
        Pipeline::new(prompt, PATCH_SCHEMA)
            .max_retries(2)
            .run(&[], runner)
    })
    .await;

    match result {
        Ok(Ok(pr)) => parse_patch_from_value(&pr.data),
        _ => vec![],
    }
}

async fn reflect(
    trajectories: &[Trajectory],
    config: &RunConfig,
    scaffold: &str,
    strategy: &str,
    skip_feedback: &str,
) -> Vec<Patch> {
    let mut patches = Vec::new();
    for traj in trajectories {
        let branch = if traj.score < config.pass_threshold {
            format!(
                "FAILURE (score={:.3}): Diagnose what went wrong and propose corrective edits.",
                traj.score
            )
        } else {
            format!(
                "SUCCESS (score={:.3}): Reinforce what worked and propose generalizing edits.",
                traj.score
            )
        };

        let mut prompt = format!(
            "{scaffold}\n\n## Strategy\n{strategy}\n\n## Case: {}\n### Stdout\n{}\n### Trace\n{}\n\n## Instruction\n{branch}",
            traj.case_id, traj.stdout, traj.trace_jsonl
        );
        if !skip_feedback.is_empty() {
            prompt.push_str(&format!(
                "\n\n## Skip feedback from previous step\n{skip_feedback}"
            ));
        }

        let patch = call_optimizer_pipeline(
            prompt,
            config.optimizer_agent.clone(),
            config.optimizer_model.clone(),
        )
        .await;
        patches.push(patch);
    }
    patches
}

async fn aggregate(patches: Vec<Patch>, config: &RunConfig) -> Vec<Edit> {
    if patches.is_empty() {
        return vec![];
    }

    let mut current_level: Vec<Vec<Edit>> = patches;

    while current_level.len() > 1 {
        let k = config.aggregate_group_size as usize;
        let mut next_level: Vec<Vec<Edit>> = Vec::new();

        for chunk in current_level.chunks(k) {
            let merged = merge_patch_group(chunk, config).await;
            next_level.push(merged);
        }

        current_level = next_level;
    }

    current_level.into_iter().next().unwrap_or_default()
}

async fn merge_patch_group(group: &[Vec<Edit>], config: &RunConfig) -> Vec<Edit> {
    if group.len() == 1 {
        return group[0].clone();
    }
    let patches_json = serde_json::to_string(&group).unwrap_or_default();
    let prompt = format!(
        "Merge these patches into one ranked patch (highest impact first):\n{patches_json}\n\nReturn a single merged patch JSON."
    );

    let merged = call_optimizer_pipeline(
        prompt,
        config.optimizer_agent.clone(),
        config.optimizer_model.clone(),
    )
    .await;

    if merged.is_empty() {
        // Fallback: concatenated union
        group.iter().flatten().cloned().collect()
    } else {
        merged
    }
}

fn select_ranked_pool(edits: Vec<Edit>) -> Vec<Edit> {
    let mut ranked = edits;
    // Stable sort by impact descending; ties keep declaration order.
    ranked.sort_by(|a, b| {
        b.impact
            .partial_cmp(&a.impact)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked
}

fn format_skip_feedback(skips: &[SkipRecord]) -> String {
    if skips.is_empty() {
        return String::new();
    }
    let mut s = String::from("The following edits were skipped in the previous step:\n");
    for skip in skips {
        s.push_str(&format!("  - Edit #{}: {:?}\n", skip.index, skip.reason));
    }
    s
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_step(
    artifact: &mut dyn Optimizable,
    step_cases: &[EvalCase],
    selection_cases: &[EvalCase],
    scorer: &dyn Scorer,
    runner: &dyn EvalRunner,
    scaffold: &str,
    strategy: &str,
    config: &RunConfig,
    state: &mut RuntimeState,
    run_dir: &Path,
    skip_feedback: &str,
) -> Result<StepResult, TextgradError> {
    let text_before = artifact.text().to_string();
    let hash_before = sha256_hex(&text_before);

    // ------------------------------------------------------------------
    // ROLLOUT: materialize artifact into per-rollout workspaces, run,
    // score each trajectory.
    // ------------------------------------------------------------------
    let mut trajectories: Vec<Trajectory> = Vec::new();
    for case in step_cases {
        let ws = tempfile::TempDir::new().map_err(TextgradError::Io)?;
        artifact.materialize(ws.path()).await?;
        let opts = CaseRunOptions {
            agent_key: config.target_agent.clone(),
            model: config.target_model.clone(),
            project_root: ws.path().to_path_buf(),
            timeout_seconds: config.timeout_seconds,
            pass_threshold: config.pass_threshold,
        };
        let (output, _case_result, trace_jsonl) = runner.run_case(case, &opts, &[]).await;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let check_results = scorer.score(&stdout, &trace_jsonl, ws.path());
        let score = item_score(&check_results, &config.gate_metric);
        trajectories.push(Trajectory {
            case_id: case.id.clone(),
            stdout,
            trace_jsonl,
            score,
        });
    }

    // ------------------------------------------------------------------
    // REFLECT: one Pipeline call per trajectory → Patch.
    // ------------------------------------------------------------------
    let patches = reflect(&trajectories, config, scaffold, strategy, skip_feedback).await;

    // ------------------------------------------------------------------
    // AGGREGATE: hierarchically merge all patches.
    // ------------------------------------------------------------------
    let merged_edits = aggregate(patches, config).await;

    // ------------------------------------------------------------------
    // SELECT: rank by impact descending.
    // ------------------------------------------------------------------
    let ranked_pool = select_ranked_pool(merged_edits);

    // ------------------------------------------------------------------
    // UPDATE: apply_budgeted with budget = lr(epoch).
    // ------------------------------------------------------------------
    let lr = compute_lr(state.epoch, config.n_epochs, config.lr_0);
    let budgeted = apply_budgeted(&text_before, &ranked_pool, lr);
    let candidate_text = budgeted.report.result.clone();
    let intra_patch_skips = budgeted.intra_patch_skips.clone();

    // ------------------------------------------------------------------
    // GATE: score candidate on the selection split.
    // ------------------------------------------------------------------
    let gate_opts = CaseRunOptions {
        agent_key: config.target_agent.clone(),
        model: config.target_model.clone(),
        project_root: run_dir.to_path_buf(),
        timeout_seconds: config.timeout_seconds,
        pass_threshold: config.pass_threshold,
    };

    // Temporarily set candidate text to materialize into the gate workspace.
    let original_text = artifact.text().to_string();
    artifact.set_text(candidate_text.clone());
    let gate_ws = tempfile::TempDir::new().map_err(TextgradError::Io)?;
    let mat_result = artifact.materialize(gate_ws.path()).await;
    if mat_result.is_err() {
        artifact.set_text(original_text.clone());
        mat_result?;
    }
    let gate_opts_ws = CaseRunOptions {
        project_root: gate_ws.path().to_path_buf(),
        ..gate_opts
    };
    let gate_results = score_cases(
        runner,
        selection_cases,
        &gate_opts_ws,
        scorer,
        config.gate_trials,
        config.parallel,
    )
    .await;
    let gate_score = split_score(&gate_results, &config.gate_metric);

    let accepted = gate_score > state.best_score + config.gate_epsilon;

    if accepted {
        // Leave artifact with candidate text.
        state.best_score = gate_score;
        let best_path =
            save_accepted_artifact(run_dir, &config.artifact_stem, &candidate_text).await?;
        let _ = best_path; // path is written; best_artifact_path is tracked by caller
    } else {
        // Restore original text.
        artifact.set_text(original_text);
        // Push to rejected edit buffer if there were actual edits proposed.
        if !ranked_pool.is_empty() {
            let score_delta = gate_score - state.best_score;
            state.rejected_edit_buffer.push(RejectedPatch {
                patch: ranked_pool.clone(),
                text_snapshot: candidate_text.clone(),
                score_delta,
            });
        }
    }

    state.current_score = gate_score;

    // ------------------------------------------------------------------
    // Write step artifacts.
    // ------------------------------------------------------------------
    let step_dir = ensure_step_dir(run_dir, state.global_step).await?;

    let rollouts_json: Vec<serde_json::Value> = trajectories
        .iter()
        .map(|t| {
            serde_json::json!({
                "case_id": t.case_id,
                "score": t.score,
            })
        })
        .collect();
    tokio::fs::write(
        step_dir.join("rollouts.json"),
        serde_json::to_vec_pretty(&rollouts_json).unwrap_or_default(),
    )
    .await?;

    tokio::fs::write(
        step_dir.join("patch.json"),
        serde_json::to_vec_pretty(&ranked_pool).unwrap_or_default(),
    )
    .await?;

    let update_info = serde_json::json!({
        "budget": lr,
        "chosen": budgeted.chosen,
        "skipped_count": intra_patch_skips.len(),
    });
    tokio::fs::write(
        step_dir.join("update.json"),
        serde_json::to_vec_pretty(&update_info).unwrap_or_default(),
    )
    .await?;

    let gate_info = serde_json::json!({
        "score": gate_score,
        "best_score": state.best_score,
        "accepted": accepted,
    });
    tokio::fs::write(
        step_dir.join("gate.json"),
        serde_json::to_vec_pretty(&gate_info).unwrap_or_default(),
    )
    .await?;

    // Append to history.json.
    let text_after = artifact.text().to_string();
    let hash_after = sha256_hex(&text_after);
    let record = StepRecord {
        global_step: state.global_step,
        epoch: state.epoch,
        hash_before,
        hash_after,
        score_current: state.current_score,
        score_candidate: gate_score,
        accepted,
        input_tokens: None,
        output_tokens: None,
    };
    append_history(run_dir, &record).await?;

    Ok(StepResult { intra_patch_skips })
}

/// Format intra-patch skips from the previous step into a human-readable feedback note.
pub(super) fn build_skip_feedback(skips: &[SkipRecord]) -> String {
    format_skip_feedback(skips)
}
