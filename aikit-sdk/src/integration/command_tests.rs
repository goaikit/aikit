use super::*;

#[test]
fn native_command_preserves_argv_utf8_streams_and_exit_status() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    let probe_dir = dir.path().join("spaces ' $ ; unicode é");
    std::fs::create_dir(&probe_dir).unwrap();
    let executable = probe_dir.join(if cfg!(windows) { "probe.exe" } else { "probe" });
    let arguments = vec![
        String::new(),
        "quote\"and\\".into(),
        "'$(not-a-command); & %NAME% !NAME!\nUTF8-é".into(),
    ];
    // A tiny native process tests the real platform parser and byte streams,
    // rather than validating a serializer against another copy of its rules.
    let source = format!(
        r#"
        use std::io::{{Read, Write}};
        fn main() {{
            assert_eq!(std::env::args().skip(1).collect::<Vec<_>>(), {arguments:?});
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).unwrap();
            assert_eq!(input, "stdin-é\\n");
            std::io::stdout().write_all("stdout-é".as_bytes()).unwrap();
            std::io::stderr().write_all("stderr-é".as_bytes()).unwrap();
            std::process::exit(23);
        }}
    "#
    );
    let source_path = dir.path().join("probe.rs");
    std::fs::write(&source_path, source).unwrap();
    let compiler = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg(&source_path)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compiler.status.success(),
        "{}",
        String::from_utf8_lossy(&compiler.stderr)
    );
    let serialized = command(&HookCommand {
        executable,
        arguments,
    })
    .unwrap();
    let mut process = if cfg!(windows) {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(serialized.split(' ').skip(1));
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", &serialized]);
        cmd
    };
    let mut child = process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all("stdin-é\\n".as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(23),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, "stdout-é".as_bytes());
    assert_eq!(output.stderr, "stderr-é".as_bytes());
}

#[test]
fn command_serialization_keeps_shell_syntax_inside_literal_data() {
    use base64::Engine;
    let handler = HookCommand {
        executable: "C:/handler with spaces.exe".into(),
        arguments: vec![
            "".into(),
            "quote\"and\\".into(),
            "'$(unsafe); & %NAME% !NAME!\nUTF8-é".into(),
        ],
    };
    let unix = command_for(&handler, false).unwrap();
    assert!(unix.contains("'\\''"));
    let windows = command_for(&handler, true).unwrap();
    let encoded = windows
        .strip_prefix("powershell.exe -NoProfile -NonInteractive -EncodedCommand ")
        .unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    let script = String::from_utf16(
        &bytes
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert!(script.contains("$p.UseShellExecute=$false"));
    assert!(script.contains("$p.FileName='C:/handler with spaces.exe'"));
    assert!(script.contains("''$(unsafe)"));
    assert_eq!(windows_argument(""), "\"\"");
    assert_eq!(windows_argument("a\"b\\"), "\"a\\\"b\\\\\"");
}
