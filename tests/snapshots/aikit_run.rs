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
}
