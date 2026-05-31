use std::process::Command;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_run_help_contains_resume_flag() {
        let output = Command::new("aikit")
            .arg("agent")
            .arg("run")
            .arg("--help")
            .output()
            .expect("Failed to execute aikit agent run --help");

        assert!(output.status.success());
        let output_str = String::from_utf8(output.stdout).unwrap();

        assert!(
            output_str.contains("--resume") || output_str.contains("-r"),
            "aikit agent run --help must list --resume / -r flag; got:\n{}",
            output_str
        );
    }

    #[test]
    fn test_agent_help_lists_subcommands() {
        let output = Command::new("aikit")
            .args(["agent", "--help"])
            .output()
            .expect("Failed to execute aikit agent --help");

        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        for sub in &["run", "list", "mcp", "check"] {
            assert!(
                stdout.contains(sub),
                "aikit agent --help must list '{}' subcommand; got:\n{}",
                sub,
                stdout
            );
        }
    }
}
