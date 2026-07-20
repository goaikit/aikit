use std::path::Path;

use aikit_sdk::deploy_skill;
use aikit_textgrad::{edit::ensure_protected_region, training::Optimizable};
use async_trait::async_trait;

pub struct SkillArtifact {
    text: String,
    skill_name: String,
    target_agent: String,
}

impl SkillArtifact {
    /// Seed from an existing SKILL.md string. Ensures the protected-region sentinels exist.
    pub fn from_existing(skill_md: String, skill_name: String, target_agent: String) -> Self {
        let text = ensure_protected_region(&skill_md);
        Self {
            text,
            skill_name,
            target_agent,
        }
    }
}

#[async_trait]
impl Optimizable for SkillArtifact {
    fn text(&self) -> &str {
        &self.text
    }

    fn set_text(&mut self, t: String) {
        self.text = t;
    }

    async fn materialize(&self, workspace: &Path) -> anyhow::Result<()> {
        deploy_skill(
            &self.target_agent,
            workspace,
            &self.skill_name,
            &self.text,
            None,
        )
        .map_err(|e| anyhow::anyhow!("deploy_skill failed: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_sdk::skill_dir;
    use aikit_textgrad::edit::{PROTECTED_BEGIN, PROTECTED_END};

    fn count_occurrences(haystack: &str, needle: &str) -> usize {
        let mut count = 0;
        let mut start = 0;
        while let Some(pos) = haystack[start..].find(needle) {
            count += 1;
            start += pos + needle.len();
        }
        count
    }

    // AC-1: from_existing on text without sentinels injects them exactly once, empty region.
    #[test]
    fn test_from_existing_without_sentinels() {
        let skill_md = "# My Skill\n\nSome content.".to_string();
        let artifact =
            SkillArtifact::from_existing(skill_md, "my-skill".to_string(), "cursor".to_string());
        let text = artifact.text();
        assert_eq!(count_occurrences(text, PROTECTED_BEGIN), 1);
        assert_eq!(count_occurrences(text, PROTECTED_END), 1);
        // The region between BEGIN and END should contain only whitespace.
        let begin_pos = text.find(PROTECTED_BEGIN).unwrap() + PROTECTED_BEGIN.len();
        let end_pos = text.find(PROTECTED_END).unwrap();
        let between = &text[begin_pos..end_pos];
        assert!(
            between.trim().is_empty(),
            "Protected region should be empty, got: {between:?}"
        );
    }

    // AC-2: from_existing on text that already has sentinels produces identical text.
    #[test]
    fn test_from_existing_with_sentinels_is_idempotent() {
        let skill_md = format!("# My Skill\n\n{}\n\n{}\n", PROTECTED_BEGIN, PROTECTED_END);
        let artifact = SkillArtifact::from_existing(
            skill_md.clone(),
            "my-skill".to_string(),
            "cursor".to_string(),
        );
        assert_eq!(artifact.text(), skill_md.as_str());
    }

    // AC-3: materialize writes the correct file content at the expected path.
    #[tokio::test]
    async fn test_materialize_writes_correct_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let skill_md = "# My Skill\n\nContent.".to_string();
        let artifact =
            SkillArtifact::from_existing(skill_md, "my-skill".to_string(), "cursor".to_string());
        artifact.materialize(dir.path()).await.unwrap();

        // Determine expected path using the same logic as deploy_skill.
        let expected_path = skill_dir(dir.path(), "cursor", "my-skill")
            .unwrap()
            .join("SKILL.md");
        assert!(
            expected_path.exists(),
            "SKILL.md should exist at {expected_path:?}"
        );
        let content = std::fs::read_to_string(&expected_path).unwrap();
        assert_eq!(content, artifact.text());
    }

    // AC-14: materialize with unrecognized agent key returns Err containing "deploy_skill failed".
    #[tokio::test]
    async fn test_materialize_unsupported_agent_returns_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let artifact = SkillArtifact::from_existing(
            "# Skill".to_string(),
            "my-skill".to_string(),
            "qwen".to_string(), // unsupported agent
        );
        let result = artifact.materialize(dir.path()).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("deploy_skill failed"),
            "Expected 'deploy_skill failed' in: {err_msg}"
        );
    }
}
