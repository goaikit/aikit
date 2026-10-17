use super::*;

#[test]
fn from_argv_keeps_argv_and_derives_the_command() {
    let h = SessionHook::from_argv(
        "my-hook",
        "/opt/tool/bin/tool",
        ["start", "--id", "my-hook"],
    );
    assert_eq!(
        h.argv.as_deref().unwrap(),
        ["/opt/tool/bin/tool", "start", "--id", "my-hook"]
    );
    assert_eq!(h.command, "/opt/tool/bin/tool start --id my-hook");
    assert_eq!(SessionHook::new("my-hook", "x my-hook").argv, None);
}

#[test]
fn posix_quoting() {
    for (arg, quoted) in [
        ("plain-arg_1.0", "plain-arg_1.0"),
        ("/opt/a,b/c:d@e%f+g", "/opt/a,b/c:d@e%f+g"),
        ("", "''"),
        ("a b", "'a b'"),
        ("it's", r"'it'\''s'"),
        ("$HOME", "'$HOME'"),
        ("~/bin/tool", "'~/bin/tool'"),
        ("a=b", "'a=b'"),
        ("a;b", "'a;b'"),
    ] {
        assert_eq!(quote_posix(arg), quoted, "{arg}");
    }
}

#[test]
fn cmd_quoting() {
    for (arg, quoted) in [
        (r"C:\tools\tool.exe", r"C:\tools\tool.exe"),
        (
            r"C:\Program Files\Tool\tool.exe",
            r#""C:\Program Files\Tool\tool.exe""#,
        ),
        ("", r#""""#),
        (r#"a"b"#, r#""a""b""#),
        ("a&b", r#""a&b""#),
    ] {
        assert_eq!(quote_cmd(arg), quoted, "{arg}");
    }
}

#[cfg(unix)]
#[test]
fn derived_command_runs_the_same_argv_in_a_shell() {
    let h = SessionHook::from_argv(
        "my-hook",
        "printf",
        ["%s|", "a b", "it's", "$HOME", "", "my-hook"],
    );
    let out = std::process::Command::new("sh")
        .args(["-c", &h.command])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "a b|it's|$HOME||my-hook|"
    );
}

#[test]
fn argv_must_hold_the_id_as_an_argument() {
    let tmp = TempDir::new().unwrap();
    for h in [
        SessionHook::from_argv("my-hook", "tool", ["--id", "my-hook2"]),
        SessionHook::from_argv("my-hook", "/opt/my-hook", Vec::<String>::new()),
        SessionHook::from_argv("my-hook", "tool", ["--id=my-hook"]),
    ] {
        assert!(
            matches!(
                register_session_hook(tmp.path(), "claude", &h),
                Err(SessionHookError::CommandMissingId { .. })
            ),
            "{h:?}"
        );
    }
    assert!(!settings(tmp.path(), "claude").exists());
}

#[test]
fn argv_hooks_register_their_command_in_json_files() {
    let tmp = TempDir::new().unwrap();
    let h = SessionHook::from_argv("my-hook", "/opt/my tool", ["--id", "my-hook"]);
    register_session_hook(tmp.path(), "claude", &h).unwrap();
    let v = read(&settings(tmp.path(), "claude"));
    let expected = if cfg!(windows) {
        r#""/opt/my tool" --id my-hook"#
    } else {
        "'/opt/my tool' --id my-hook"
    };
    assert_eq!(
        v["hooks"]["SessionStart"][0]["hooks"][0]["command"],
        expected
    );
}
