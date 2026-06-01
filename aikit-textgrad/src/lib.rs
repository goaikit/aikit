//! `aikit-textgrad`: Artifact-agnostic text-gradient optimization loop.
//!
//! Two-layer architecture:
//!
//! - **Layer 1** (`aikit_textgrad::edit`): Pure, deterministic string-transformation substrate.
//!   No dependency on `aikit-evals` or `aikit-sdk`; independently compilable and unit-testable.
//! - **Layer 2** (`aikit_textgrad::training`): Async optimization loop using `aikit-evals` for
//!   scoring and `aikit-sdk::Pipeline` for optimizer model calls.

pub mod edit;
pub mod training;
