use std::process::Command;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_help_contains_resume_flag() {
        let output = Command::new("aikit")
            .arg("run")
            .arg("--help")
            .output()
            .expect("Failed to execute aikit run --help");

        assert!(output.status.success());
        let output_str = String::from_utf8(output.stdout).unwrap();

        assert!(
            output_str.contains("--resume") || output_str.contains("-r"),
            "aikit run --help must list --resume / -r flag; got:\n{}",
            output_str
        );
        assert!(
            output_str.contains("resume-last") || output_str.contains("--resume"),
            "aikit run --help must list session resume flags; got:\n{}",
            output_str
        );
    }

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
    fn test_run_deprecated_emits_warning() {
        let output = Command::new("aikit")
            .args(["run", "--agent", "codex", "--dry-run", "-p", "hello"])
            .output()
            .expect("Failed to execute aikit run");

        assert!(output.status.success());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains("deprecated"),
            "aikit run must print a deprecation warning; stderr:\n{}",
            stderr
        );
    }
}
