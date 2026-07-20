use assert_cmd::Command;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serve_help_lists_flags() {
        let mut cmd = Command::cargo_bin("aikit").expect("aikit binary must exist");
        cmd.args(["serve", "--help"]);
        let output = cmd.output().expect("Failed to execute aikit serve --help");

        assert!(output.status.success(), "aikit serve --help must exit 0");

        let stdout = String::from_utf8(output.stdout).unwrap();

        for flag in &[
            "--host",
            "--port",
            "--run-timeout-secs",
            "--max-sessions",
            "--api-key",
            "--insecure",
        ] {
            assert!(
                stdout.contains(flag),
                "aikit serve --help must list {}; got:\n{}",
                flag,
                stdout
            );
        }
    }
}
