#[cfg(unix)]
mod unix {
    use assert_cmd::prelude::*;
    use predicates::prelude::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    fn write_stub(dir: &std::path::Path, name: &str, body: &str) {
        let path = dir.join(name);
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "#!/bin/sh\n{}", body).unwrap();
        let mut perms = file.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        file.set_permissions(perms).unwrap();
    }

    #[test]
    fn test_run_events_emits_quota_exceeded_ndjson() {
        let dir = tempfile::tempdir().unwrap();
        write_stub(
            dir.path(),
            "claude",
            r#"printf 'Error: Failed to load usage data: {"error":{"type":"rate_limit_error","message":"Rate limited. Please try again later."}}\n' >&2
printf '{"type":"result","subtype":"success","result":"OK"}\n'"#,
        );

        let original_path = std::env::var("PATH").unwrap_or_default();
        let path = format!("{}:{}", dir.path().display(), original_path);

        let mut cmd = Command::new(env!("CARGO_BIN_EXE_aikit"));
        cmd.env("PATH", path)
            .arg("run")
            .arg("-a")
            .arg("claude")
            .arg("-p")
            .arg("test quota")
            .arg("--events");

        cmd.assert()
            .code(0)
            .stdout(predicate::str::contains(r#""payload":{"quota_exceeded":{"#));
    }
}
