//! Integration tests for agent command adaptation

use std::fs;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_generation_for_multiple_agents() {
        use aikit::core::agent::{get_agent_configs, AgentConfig};
        use aikit::models::package::{CommandDefinition, Package};
        use std::collections::HashMap;

        // Create a test package
        let mut package = Package::new(
            "test-agent-adaptation".to_string(),
            "1.0.0".to_string(),
            "Test package for agent adaptation".to_string(),
        );

        // Add test commands
        package.commands.insert(
            "analyze".to_string(),
            CommandDefinition {
                description: "Analyze code quality".to_string(),
                template: Some("analyze.md".to_string()),
            },
        );

        package.commands.insert(
            "format".to_string(),
            CommandDefinition {
                description: "Format code".to_string(),
                template: Some("format.md".to_string()),
            },
        );

        // Test command generation for first 3 agents to keep test fast
        let agents_to_test = get_agent_configs().into_iter().take(3).collect::<Vec<_>>();

        for agent in agents_to_test {
            // Test namespace prefix
            let prefix = agent.get_namespace_prefix("test-agent-adaptation");
            assert!(prefix.starts_with("test-agent-adaptation."));
            assert!(prefix.ends_with(&format!(".{}", agent.key)));

            // Test command generation
            let command_content = agent.generate_package_command(
                "test-agent-adaptation",
                "analyze",
                "Analyze code quality",
                "echo 'Running analysis...'",
            );

            // Verify command content includes expected elements
            assert!(command_content.contains("test-agent-adaptation.analyze"));
            assert!(command_content.contains("Analyze code quality"));
            assert!(command_content.contains("echo 'Running analysis...'"));

            // Verify agent-specific formatting
            match agent.output_format {
                aikit::core::agent::OutputFormat::Markdown => {
                    assert!(command_content.contains("# "));
                }
                aikit::core::agent::OutputFormat::Toml => {
                    assert!(command_content.contains("command = "));
                }
                aikit::core::agent::OutputFormat::AgentMd => {
                    assert!(command_content.contains("Command: "));
                }
            }
        }
    }

    #[test]
    fn test_agent_override_handling() {
        use aikit::core::agent::{get_agent_config, AgentConfig};
        use std::collections::HashMap;

        let agent = get_agent_config("claude").expect("Claude agent should exist");

        // Test override handling
        let mut overrides = HashMap::new();
        overrides.insert("{{args}}".to_string(), "--custom-args".to_string());
        overrides.insert("default_script".to_string(), "custom_script.sh".to_string());

        let content = "# Command\nArgs: {{args}}\nScript: default_script";
        let adapted = agent.apply_overrides(content, &overrides);

        // Should replace {{args}} with agent's placeholder, and apply custom overrides
        assert!(adapted.contains(&agent.arg_placeholder));
        assert!(adapted.contains("custom_script.sh"));
    }

    #[test]
    fn test_namespace_prefix_uniqueness() {
        use aikit::core::agent::get_agent_configs;

        let agents = get_agent_configs();
        let mut prefixes = std::collections::HashSet::new();

        // Generate prefixes for a test package
        for agent in &agents {
            let prefix = agent.get_namespace_prefix("test-package");
            // Should be unique across agents
            assert!(
                prefixes.insert(prefix),
                "Duplicate prefix found for agent {}",
                agent.key
            );
        }

        // Should have same number of unique prefixes as agents
        assert_eq!(prefixes.len(), agents.len());
    }

    #[test]
    fn test_all_17_agents_supported() {
        use aikit::core::agent::get_agent_configs;

        let agents = get_agent_configs();
        assert_eq!(agents.len(), 17, "Should support exactly 17 agents");

        // Verify all expected agents are present - using actual 17 keys from ai-agent-deploy catalog
        let expected_agents = vec![
            "claude",
            "gemini",
            "copilot",
            "cursor-agent",
            "qwen",
            "opencode",
            "codex",
            "windsurf",
            "kilocode",
            "auggie",
            "roo",
            "codebuddy",
            "qoder",
            "amp",
            "shai",
            "q",
            "bob",
        ];

        for expected in expected_agents {
            assert!(
                agents.iter().any(|a| a.key == expected),
                "Agent '{}' should be in the supported list",
                expected
            );
        }
    }
}
