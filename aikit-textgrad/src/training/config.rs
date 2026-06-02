//! Run configuration, prompt containers, and error types for the training loop.

use aikit_evals::GateMetric;
use serde::{Deserialize, Serialize};

/// Complete configuration for a training run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunConfig {
    /// Number of training epochs.
    pub n_epochs: u32,
    /// Rollout batch size (B): training cases per accumulation step.
    pub batch_size: u32,
    /// Gradient-accumulation steps (A): rollout mini-batches merged per optimizer call.
    pub accumulation: u32,
    /// Minibatch size for hierarchical patch aggregation (K).
    pub aggregate_group_size: u32,
    /// Initial learning rate (edit budget at epoch 0).
    pub lr_0: u32,
    /// Score threshold below which a trajectory is treated as a failure in REFLECT.
    pub pass_threshold: f64,
    /// Gate metric used for `split_score` in both the GATE stage and Slow Update.
    pub gate_metric: GateMetric,
    /// Number of trials per selection-split case during gating.
    pub gate_trials: u32,
    /// Minimum improvement required for a candidate to be accepted (`score > best + epsilon`).
    pub gate_epsilon: f64,
    /// Whether Slow Update writes the protected region unconditionally or via the gate.
    pub slow_update_mode: SlowUpdateMode,
    /// Soft character cap hinted to the optimizer for the protected region.
    pub protected_soft_cap_chars: usize,
    /// Agent key for the target agent (run during ROLLOUT).
    pub target_agent: String,
    /// Optional model override for the target agent.
    pub target_model: Option<String>,
    /// Agent key for the optimizer agent (REFLECT, AGGREGATE, Slow Update, Meta-Skill).
    pub optimizer_agent: String,
    /// Optional model override for the optimizer agent.
    pub optimizer_model: Option<String>,
    /// Timeout in seconds per rollout case execution.
    pub timeout_seconds: u64,
    /// Maximum concurrent rollout executions (default: number of CPUs).
    pub parallel: Option<u32>,
    /// Stem for artifact filenames: `best_{stem}.md`, `{stem}s/{stem}_vNNNN.md`.
    pub artifact_stem: String,
}

/// Controls how Slow Update applies the revised protected region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SlowUpdateMode {
    /// Write the revised protected region unconditionally.
    ForceAccept,
    /// Write only if the revised artifact passes the gate on the selection split.
    Gated,
}

/// Immutable scaffold and mutable strategy for the optimizer model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerPrompts {
    /// Structural instructions, JSON schema, and anchor rules. Never modified.
    pub scaffold: String,
    /// Heuristics revised by Meta-Skill each epoch.
    pub strategy: String,
}

/// Errors returned by `run_training` and `resume_training`.
#[derive(Debug, thiserror::Error)]
pub enum TextgradError {
    #[error("TEXTGRAD_NO_SELECTION_CASES: EvalSuite has zero cases tagged 'selection'")]
    NoSelectionCases,
    #[error("TEXTGRAD_INVALID_CONFIG: {0}")]
    InvalidConfig(String),
    #[error("TEXTGRAD_RESUME_STATE_CORRUPT: {0}")]
    ResumeStateCorrupt(String),
    #[error("TEXTGRAD_MATERIALIZE_FAILED: {0}")]
    MaterializeFailed(#[from] anyhow::Error),
    #[error("TEXTGRAD_IO: {0}")]
    Io(#[from] std::io::Error),
}

/// Validate `RunConfig` fields that must be non-zero.
pub fn validate_config(config: &RunConfig) -> Result<(), TextgradError> {
    if config.batch_size == 0 {
        return Err(TextgradError::InvalidConfig(
            "batch_size must be > 0".to_string(),
        ));
    }
    if config.accumulation == 0 {
        return Err(TextgradError::InvalidConfig(
            "accumulation must be > 0".to_string(),
        ));
    }
    if config.n_epochs == 0 {
        return Err(TextgradError::InvalidConfig(
            "n_epochs must be > 0".to_string(),
        ));
    }
    if config.gate_trials == 0 {
        return Err(TextgradError::InvalidConfig(
            "gate_trials must be > 0".to_string(),
        ));
    }
    if config.lr_0 == 0 {
        return Err(TextgradError::InvalidConfig("lr_0 must be > 0".to_string()));
    }
    if config.aggregate_group_size == 0 {
        return Err(TextgradError::InvalidConfig(
            "aggregate_group_size must be > 0".to_string(),
        ));
    }
    Ok(())
}
