use super::*;

const ENTRY: &str = "tool --id my-hook";

fn config(home: &Path) -> PathBuf {
    home.join(".codex/config.toml")
}

#[test]
fn adds_to_missing_and_empty_file() {
    for initial in [None, Some(""), Some("{}")] {
        let tmp = TempDir::new().unwrap();
        let path = settings(tmp.path(), "codex");
        assert_eq!(path, tmp.path().join(".codex/hooks.json"));
        if let Some(text) = initial {
            write(&path, text);
        }
        assert_eq!(
            register_session_hook(tmp.path(), "codex", &hook("my-hook", ENTRY)).unwrap(),
            HookChange::Added
        );
        assert_eq!(
            read(&path),
            json!({"hooks": {"SessionStart": [{"matcher": "startup|resume", "hooks": [
                {"type": "command", "command": ENTRY, "timeout": 10}
            ]}]}})
        );
        assert!(has_session_hook(tmp.path(), "codex", "my-hook").unwrap());
    }
}

#[test]
fn other_hooks_are_kept_and_the_cycle_reports_each_change() {
    let tmp = TempDir::new().unwrap();
    let path = settings(tmp.path(), "codex");
    write(&path, OTHERS);
    let original = read(&path);
    let h = hook("my-hook", ENTRY);
    assert_eq!(
        register_session_hook(tmp.path(), "codex", &h).unwrap(),
        HookChange::Added
    );
    let v = read(&path);
    assert_eq!(
        v["hooks"]["SessionStart"][0]["hooks"][0]["command"],
        "other-start"
    );
    assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "check");
    let before = fs::read_to_string(&path).unwrap();
    assert_eq!(
        register_session_hook(tmp.path(), "codex", &h).unwrap(),
        HookChange::Unchanged
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    assert_eq!(
        register_session_hook(tmp.path(), "codex", &hook("my-hook", "tool2 --id my-hook")).unwrap(),
        HookChange::Updated
    );
    assert_eq!(
        read(&path)["hooks"]["SessionStart"][1]["hooks"][0],
        json!({"type": "command", "command": "tool2 --id my-hook", "timeout": 10})
    );
    assert_eq!(
        unregister_session_hook(tmp.path(), "codex", "my-hook").unwrap(),
        HookChange::Removed
    );
    assert_eq!(read(&path), original);
    assert_eq!(
        unregister_session_hook(tmp.path(), "codex", "my-hook").unwrap(),
        HookChange::Unchanged
    );
}

#[test]
fn wrong_shape_is_refused_and_untouched() {
    for text in [
        "[]",
        r#"{"hooks": []}"#,
        r#"{"hooks": {"SessionStart": [1]}}"#,
    ] {
        let tmp = TempDir::new().unwrap();
        let path = settings(tmp.path(), "codex");
        write(&path, text);
        assert!(matches!(
            register_session_hook(tmp.path(), "codex", &hook("my-hook", ENTRY)),
            Err(SessionHookError::UnexpectedType { .. })
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }
}

#[test]
fn hooks_in_config_toml_are_refused_and_nothing_is_written() {
    for text in [
        "[[hooks.SessionStart]]\nmatcher = \"startup\"\n",
        "[hooks]\nmanaged_dir = \"/opt/hooks\"\n",
        "[hooks.state]\n[[hooks.Stop]]\nmatcher = \"\"\n",
        "model = \"x\"\nhooks = 3\n",
    ] {
        let tmp = TempDir::new().unwrap();
        write(&config(tmp.path()), text);
        match register_session_hook(tmp.path(), "codex", &hook("my-hook", ENTRY)) {
            Err(SessionHookError::OtherHookFile(p)) => assert_eq!(p, config(tmp.path())),
            other => panic!("{text}: {other:?}"),
        }
        assert!(!settings(tmp.path(), "codex").exists());
        assert_eq!(fs::read_to_string(config(tmp.path())).unwrap(), text);
    }
}

#[test]
fn config_toml_without_hooks_is_fine_and_invalid_toml_is_refused() {
    for text in [
        "model = \"x\"\n[features]\nhooks = true\n",
        "[hooks]\n",
        // What Codex 0.163.0 writes when the user trusts a hook in `/hooks`.
        "[hooks.state]\n\n[hooks.state.\"/h/.codex/hooks.json:session_start:0:0\"]\n\
         trusted_hash = \"sha256:00\"\n",
    ] {
        let tmp = TempDir::new().unwrap();
        write(&config(tmp.path()), text);
        assert_eq!(
            register_session_hook(tmp.path(), "codex", &hook("my-hook", ENTRY)).unwrap(),
            HookChange::Added,
            "{text}"
        );
        assert_eq!(fs::read_to_string(config(tmp.path())).unwrap(), text);
    }

    let tmp = TempDir::new().unwrap();
    write(&config(tmp.path()), "model = \n");
    assert!(matches!(
        register_session_hook(tmp.path(), "codex", &hook("my-hook", ENTRY)),
        Err(SessionHookError::InvalidToml { .. })
    ));
    assert!(!settings(tmp.path(), "codex").exists());
    // Removing never looks at config.toml.
    assert_eq!(
        unregister_session_hook(tmp.path(), "codex", "my-hook").unwrap(),
        HookChange::Unchanged
    );
}

#[test]
fn admin_entry_is_toml_for_requirements() {
    let entry = admin_hook_entry("codex", &hook("my-hook", r#"tool --id my-hook "q""#)).unwrap();
    assert_eq!(entry.format, AdminHookFormat::Toml);
    assert_eq!(
        entry.text,
        r#"[[hooks.SessionStart]]
matcher = "startup|resume"

[[hooks.SessionStart.hooks]]
type = "command"
command = 'tool --id my-hook "q"'
timeout = 10
"#
    );
    let parsed: toml::Table = toml::from_str(&entry.text).unwrap();
    let group = &parsed["hooks"]["SessionStart"][0];
    assert_eq!(
        group["hooks"][0]["command"].as_str(),
        Some(r#"tool --id my-hook "q""#)
    );
}

#[test]
fn admin_presence_reads_requirements_toml() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("requirements.toml");
    assert!(!admin_hook_present_in(&path, "codex", "my-hook"));
    let entry = admin_hook_entry("codex", &hook("my-hook", ENTRY)).unwrap();
    fs::write(
        &path,
        format!(
            "allowed_approval_policies = [\"never\"]\n\n[features]\nhooks = true\n\n\
             [hooks]\nmanaged_dir = \"/opt/hooks\"\n\n{}",
            entry.text
        ),
    )
    .unwrap();
    assert!(admin_hook_present_in(&path, "codex", "my-hook"));
    assert!(!admin_hook_present_in(&path, "codex", "other-hook"));
    fs::write(&path, "not = = toml").unwrap();
    assert!(!admin_hook_present_in(&path, "codex", "my-hook"));
    fs::write(&path, "[hooks]\nSessionStart = 3\n").unwrap();
    assert!(!admin_hook_present_in(&path, "codex", "my-hook"));
}

#[test]
fn admin_file_per_platform() {
    let file = admin_hook_file("codex").unwrap();
    if cfg!(windows) {
        assert_eq!(
            file,
            PathBuf::from(r"C:\ProgramData\OpenAI\Codex\requirements.toml")
        );
    } else {
        assert_eq!(file, PathBuf::from("/etc/codex/requirements.toml"));
    }
}
