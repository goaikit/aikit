//! Runtime state types, I/O helpers, and run-dir layout management.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::edit::Patch;
use crate::training::config::{RunConfig, TextgradError};

/// Persisted state written atomically after each step.
#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeState {
    pub config: RunConfig,
    pub epoch: u32,
    pub step_in_epoch: u32,
    pub global_step: u32,
    pub best_score: f64,
    pub current_score: f64,
    pub rejected_edit_buffer: Vec<RejectedPatch>,
    pub optimizer_strategy: String,
}

/// A gate-rejected patch together with context for Slow Update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedPatch {
    pub patch: Patch,
    pub text_snapshot: String,
    pub score_delta: f64,
}

/// Per-step history record appended to `history.json`.
#[derive(Debug, Serialize, Deserialize)]
pub struct StepRecord {
    pub global_step: u32,
    pub epoch: u32,
    pub hash_before: String,
    pub hash_after: String,
    pub score_current: f64,
    pub score_candidate: f64,
    pub accepted: bool,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// Final outcome of a training run.
#[derive(Debug)]
pub struct TrainingOutcome {
    pub best_text: String,
    pub best_score: f64,
    /// Score on the test split (if any test cases); otherwise equals `best_score`.
    pub final_score: f64,
    /// Path to `best_{stem}.md` inside `run_dir`.
    pub best_artifact_path: PathBuf,
}

// ---- I/O helpers ----

/// SHA-256 hex digest of `text` as UTF-8 bytes.
pub fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Write `state` to `run_dir/runtime_state.json` atomically (write-to-tmp then rename).
pub async fn write_runtime_state(
    run_dir: &Path,
    state: &RuntimeState,
) -> Result<(), std::io::Error> {
    let path = run_dir.join("runtime_state.json");
    let tmp_path = run_dir.join("runtime_state.json.tmp");
    let content = serde_json::to_vec_pretty(state).unwrap_or_else(|_| b"{}".to_vec());
    tokio::fs::write(&tmp_path, &content).await?;
    tokio::fs::rename(&tmp_path, &path).await?;
    Ok(())
}

/// Read and deserialize `run_dir/runtime_state.json`.
pub async fn read_runtime_state(run_dir: &Path) -> Result<RuntimeState, TextgradError> {
    let path = run_dir.join("runtime_state.json");
    let bytes = tokio::fs::read(&path).await.map_err(|e| {
        TextgradError::ResumeStateCorrupt(format!("cannot read runtime_state.json: {e}"))
    })?;
    serde_json::from_slice::<RuntimeState>(&bytes).map_err(|e| {
        TextgradError::ResumeStateCorrupt(format!("malformed runtime_state.json: {e}"))
    })
}

/// Append a `StepRecord` to `run_dir/history.json`.
pub async fn append_history(run_dir: &Path, record: &StepRecord) -> Result<(), std::io::Error> {
    let path = run_dir.join("history.json");
    let existing = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    let mut records: Vec<serde_json::Value> = serde_json::from_str(&existing).unwrap_or_default();
    if let Ok(v) = serde_json::to_value(record) {
        records.push(v);
    }
    let content = serde_json::to_vec_pretty(&records).unwrap_or_else(|_| b"[]".to_vec());
    tokio::fs::write(&path, &content).await
}

/// Create the initial run-dir layout (idempotent).
pub async fn init_run_dir(run_dir: &Path, config: &RunConfig) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(run_dir).await?;
    tokio::fs::create_dir_all(run_dir.join("steps")).await?;
    let versions_dir = format!("{}s", config.artifact_stem);
    tokio::fs::create_dir_all(run_dir.join(versions_dir)).await?;
    Ok(())
}

/// Create the directory for a specific step.
pub async fn ensure_step_dir(run_dir: &Path, global_step: u32) -> Result<PathBuf, std::io::Error> {
    let dir = run_dir.join(format!("steps/step_{global_step:04}"));
    tokio::fs::create_dir_all(&dir).await?;
    Ok(dir)
}

/// Create the directory for a specific epoch boundary.
pub async fn ensure_epoch_dir(run_dir: &Path, epoch: u32) -> Result<PathBuf, std::io::Error> {
    let dir = run_dir.join(format!("epoch_{epoch:02}"));
    tokio::fs::create_dir_all(&dir).await?;
    Ok(dir)
}

/// Write (or overwrite) `best_{stem}.md` and create a new versioned copy.
pub async fn save_accepted_artifact(
    run_dir: &Path,
    stem: &str,
    text: &str,
) -> Result<PathBuf, std::io::Error> {
    let best_path = run_dir.join(format!("best_{stem}.md"));
    tokio::fs::write(&best_path, text.as_bytes()).await?;

    // Determine next version number by counting existing versions.
    let versions_dir = run_dir.join(format!("{stem}s"));
    let prefix = format!("{stem}_v");
    let mut next_ver: u32 = 0;
    if let Ok(mut entries) = tokio::fs::read_dir(&versions_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with(&prefix) && name_str.ends_with(".md") {
                if let Some(ver_str) = name_str
                    .strip_prefix(&prefix)
                    .and_then(|s| s.strip_suffix(".md"))
                {
                    if let Ok(v) = ver_str.parse::<u32>() {
                        next_ver = next_ver.max(v + 1);
                    }
                }
            }
        }
    }

    let ver_path = versions_dir.join(format!("{stem}_v{next_ver:04}.md"));
    tokio::fs::write(&ver_path, text.as_bytes()).await?;
    Ok(best_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::training::config::{RunConfig, SlowUpdateMode};
    use aikit_evals::GateMetric;
    use tempfile::TempDir;

    fn make_config() -> RunConfig {
        RunConfig {
            n_epochs: 1,
            batch_size: 1,
            accumulation: 1,
            aggregate_group_size: 2,
            lr_0: 2,
            pass_threshold: 0.5,
            gate_metric: GateMetric::Soft,
            gate_trials: 1,
            gate_epsilon: 0.01,
            slow_update_mode: SlowUpdateMode::ForceAccept,
            protected_soft_cap_chars: 1000,
            target_agent: "stub".to_string(),
            target_model: None,
            optimizer_agent: "stub-opt".to_string(),
            optimizer_model: None,
            timeout_seconds: 30,
            parallel: Some(1),
            artifact_stem: "artifact".to_string(),
        }
    }

    fn make_state(config: RunConfig) -> RuntimeState {
        RuntimeState {
            epoch: 0,
            step_in_epoch: 0,
            global_step: 0,
            best_score: 0.5,
            current_score: 0.5,
            rejected_edit_buffer: vec![],
            optimizer_strategy: "strategy".to_string(),
            config,
        }
    }

    // AC29: atomic write leaves no partial state observable
    #[tokio::test]
    async fn test_runtime_state_round_trip() {
        let dir = TempDir::new().unwrap();
        let state = make_state(make_config());
        write_runtime_state(dir.path(), &state).await.unwrap();

        // The main file must exist after write
        assert!(dir.path().join("runtime_state.json").exists());
        // The temp file must NOT exist after the rename
        assert!(!dir.path().join("runtime_state.json.tmp").exists());

        // Read back and verify key fields
        let restored = read_runtime_state(dir.path()).await.unwrap();
        assert_eq!(restored.epoch, state.epoch);
        assert_eq!(restored.global_step, state.global_step);
        assert!((restored.best_score - state.best_score).abs() < 1e-9);
        assert_eq!(restored.optimizer_strategy, state.optimizer_strategy);
    }

    // RuntimeState round-trip verifies rejected_edit_buffer and optimizer_strategy survive
    #[tokio::test]
    async fn test_rejected_edit_buffer_survives_checkpoint() {
        let dir = TempDir::new().unwrap();
        let config = make_config();
        let mut state = make_state(config);
        state.rejected_edit_buffer = vec![RejectedPatch {
            patch: vec![crate::edit::Edit {
                op: crate::edit::EditOp::Append,
                target: None,
                content: Some("extra".to_string()),
                impact: 0.5,
            }],
            text_snapshot: "snap".to_string(),
            score_delta: -0.1,
        }];
        state.optimizer_strategy = "custom strategy".to_string();

        write_runtime_state(dir.path(), &state).await.unwrap();
        let restored = read_runtime_state(dir.path()).await.unwrap();

        assert_eq!(restored.rejected_edit_buffer.len(), 1);
        assert_eq!(restored.optimizer_strategy, "custom strategy");
    }

    #[test]
    fn test_sha256_hex_deterministic() {
        let h1 = sha256_hex("hello");
        let h2 = sha256_hex("hello");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 = 32 bytes = 64 hex chars
    }

    #[test]
    fn test_sha256_hex_different_for_different_inputs() {
        assert_ne!(sha256_hex("hello"), sha256_hex("world"));
    }

    #[tokio::test]
    async fn test_read_runtime_state_missing_returns_error() {
        let dir = TempDir::new().unwrap();
        let result = read_runtime_state(dir.path()).await;
        assert!(matches!(result, Err(TextgradError::ResumeStateCorrupt(_))));
    }
}
