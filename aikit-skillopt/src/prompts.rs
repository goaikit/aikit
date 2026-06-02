use aikit_textgrad::training::OptimizerPrompts;

/// Framing injected before the trajectory when the rollout score is below `pass_threshold`.
pub const FAILURE_FRAMING: &str = "\
The following trajectory represents a FAILED agent run. \
Diagnose the root cause of the failure by examining what the agent did wrong or what the skill \
document failed to guide it toward. Focus on the single most impactful failure mode before \
considering secondary issues.";

/// Framing injected before the trajectory when the rollout score is at or above `pass_threshold`.
pub const SUCCESS_FRAMING: &str = "\
The following trajectory represents a PASSING agent run. \
The agent succeeded on this case. Identify edits to the skill document that could make future \
passes more reliable, cover additional edge cases, or consolidate guidance that is currently \
implicit in the agent's behavior.";

const SKILL_SCAFFOLD: &str = r#"You are an optimizer improving a SKILL.md document that an AI agent reads before executing tasks.

CONTEXT
-------
The skill document guides the agent's behavior at runtime. Your goal is to edit it so the agent
performs better on future tasks as measured by the evaluation suite.

INPUT
-----
You receive:
- The current SKILL.md text.
- A task trajectory (agent stdout and structured trace) from a single rollout.
- A score in [0, 1] reflecting how well the agent performed on that rollout.

OUTPUT FORMAT
-------------
Return a JSON array of edit objects matching the Patch schema:

[
  {
    "op": "replace" | "insert_after" | "append" | "delete",
    "target": "<verbatim anchor copied from the current skill text>",
    "content": "<replacement or insertion text>",
    "impact": <0.0–1.0>
  },
  ...
]

Field descriptions:
- "op": The edit operation. "append" inserts at the end of the editable region (before the
  protected section). "insert_after" inserts immediately after the target anchor. "replace"
  substitutes the target with content. "delete" removes the target (content is ignored).
- "target": An exact verbatim anchor string copied from the current skill text. Whitespace-
  normalized matching is a fallback, not a license to paraphrase — always copy the anchor
  verbatim from the skill text.
- "content": The new text for insert/replace operations. Omit or use "" for "delete".
- "impact": A float in [0.0, 1.0] indicating the estimated importance of this edit.

ANCHOR RULE
-----------
Anchors MUST be copied verbatim from the current skill text. Do not paraphrase, summarize, or
reconstruct an anchor from memory. If you cannot find a suitable verbatim anchor, use "append"
to add new content at the end of the editable region instead.

PROTECTED REGION
----------------
Do NOT read, reference, or edit any text inside the protected region:

  <!-- SKILLOPT:PROTECTED:BEGIN -->
  ...
  <!-- SKILLOPT:PROTECTED:END -->

All edit operations targeting text inside the protected region will be rejected. Use "append"
or "insert_after" to add content outside the protected region instead.
"#;

const INITIAL_STRATEGY: &str = r#"1. Diagnose the single most impactful failure first; do not try to fix everything at once.
2. Prefer adding a general rule over special-casing one task.
3. Keep the skill concise — prefer editing existing text over appending new sections.
"#;

/// Returns the `OptimizerPrompts` for skill-document optimization.
///
/// The scaffold is immutable. The strategy is the initial set of heuristics and will be
/// revised per epoch by Meta-Skill during training.
pub fn skill_prompts() -> OptimizerPrompts {
    OptimizerPrompts {
        scaffold: SKILL_SCAFFOLD.to_string(),
        strategy: INITIAL_STRATEGY.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // AC-4: scaffold contains "verbatim" and "SKILLOPT:PROTECTED".
    #[test]
    fn test_scaffold_contains_required_strings() {
        let prompts = skill_prompts();
        assert!(
            prompts.scaffold.contains("verbatim"),
            "scaffold must contain 'verbatim'"
        );
        assert!(
            prompts.scaffold.contains("SKILLOPT:PROTECTED"),
            "scaffold must contain 'SKILLOPT:PROTECTED'"
        );
    }

    // AC-5: skill_prompts() is pure — scaffold is identical across two calls.
    #[test]
    fn test_skill_prompts_is_pure() {
        let p1 = skill_prompts();
        let p2 = skill_prompts();
        assert_eq!(p1.scaffold, p2.scaffold);
    }

    // AC-6: strategy is non-empty and contains "general rule".
    #[test]
    fn test_strategy_contains_general_rule() {
        let prompts = skill_prompts();
        assert!(!prompts.strategy.is_empty(), "strategy must be non-empty");
        assert!(
            prompts.strategy.contains("general rule"),
            "strategy must contain 'general rule'"
        );
    }
}
