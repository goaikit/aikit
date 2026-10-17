use super::*;
use serde_json::{json, Value};
use std::fs;
use tempfile::TempDir;

mod argv;
mod codex;
mod cursor;
mod pi;

fn hook(id: &str, command: &str) -> SessionHook {
    SessionHook::new(id, command)
}

fn settings(home: &Path, key: &str) -> PathBuf {
    home.join(session_hook_support(key).unwrap().user_settings)
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

const OTHERS: &str = r#"{
  "zeta": 1,
  "hooks": {
"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "check"}]}],
"SessionStart": [
  {"matcher": "startup", "hooks": [{"type": "command", "command": "other-start", "name": "other"}]}
]
  },
  "alpha": {"b": 2, "a": 1}
}
"#;

#[test]
fn supported_agents_and_unsupported_keys() {
    assert_eq!(
        session_hook_agents(),
        vec!["claude", "gemini", "codex", "cursor", "pi"]
    );
    let c = session_hook_support("claude").unwrap();
    assert_eq!(c.user_settings, PathBuf::from(".claude/settings.json"));
    let g = session_hook_support("gemini").unwrap();
    assert_eq!(g.user_settings, PathBuf::from(".gemini/settings.json"));
    assert!(session_hook_support("copilot").is_none());

    let tmp = TempDir::new().unwrap();
    let h = hook("my-hook", "tool --id my-hook");
    assert!(matches!(
        register_session_hook(tmp.path(), "copilot", &h),
        Err(SessionHookError::Unsupported(_))
    ));
    assert!(matches!(
        unregister_session_hook(tmp.path(), "nope", "my-hook"),
        Err(SessionHookError::Unsupported(_))
    ));
    assert!(has_session_hook(tmp.path(), "nope", "my-hook").is_err());
    assert!(admin_hook_file("copilot").is_none());
    assert!(admin_hook_entry("copilot", &h).is_err());
    assert!(format_notice("copilot", "hi").is_none());
    assert!(!admin_hook_present("copilot", "my-hook"));
}

#[test]
fn approval_note_only_for_claude_and_codex() {
    for key in ["claude", "codex"] {
        let note = session_hook_support(key).unwrap().approval_note;
        assert!(note.unwrap().contains("/hooks"), "{key}");
    }
    for key in ["gemini", "cursor", "pi"] {
        assert!(session_hook_support(key).unwrap().approval_note.is_none());
    }
}

#[test]
fn id_and_command_are_validated() {
    let tmp = TempDir::new().unwrap();
    for id in ["", "a b", "a/b", "a.b", "é"] {
        assert!(matches!(
            register_session_hook(tmp.path(), "claude", &hook(id, &format!("x {id}"))),
            Err(SessionHookError::InvalidId(_))
        ));
    }
    for command in ["tool", "tool --id my-hook2", "tool xmy-hook"] {
        assert!(matches!(
            register_session_hook(tmp.path(), "claude", &hook("my-hook", command)),
            Err(SessionHookError::CommandMissingId { .. })
        ));
    }
    assert!(!settings(tmp.path(), "claude").exists());
    assert!(has_word("/opt/my-hook.sh", "my-hook"));
    assert!(has_word("tool --id=my-hook", "my-hook"));
}

#[test]
fn adds_to_missing_file_for_each_agent() {
    let tmp = TempDir::new().unwrap();
    let h = hook("my-hook", "tool start --id my-hook");

    assert_eq!(
        register_session_hook(tmp.path(), "claude", &h).unwrap(),
        HookChange::Added
    );
    let path = settings(tmp.path(), "claude");
    assert_eq!(
        read(&path),
        json!({"hooks": {"SessionStart": [
            {"matcher": "", "hooks": [{"type": "command", "command": "tool start --id my-hook"}]}
        ]}})
    );
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.ends_with("}\n"));
    assert!(text.contains("\n  \"hooks\": {\n    \"SessionStart\""));

    assert_eq!(
        register_session_hook(tmp.path(), "gemini", &h).unwrap(),
        HookChange::Added
    );
    assert_eq!(
        read(&settings(tmp.path(), "gemini")),
        json!({"hooks": {"SessionStart": [
            {"matcher": "*", "hooks": [
                {"type": "command", "command": "tool start --id my-hook", "name": "my-hook"}
            ]}
        ]}})
    );
    assert!(has_session_hook(tmp.path(), "claude", "my-hook").unwrap());
    assert!(has_session_hook(tmp.path(), "gemini", "my-hook").unwrap());
    assert!(!has_session_hook(tmp.path(), "gemini", "other").unwrap());
}

#[test]
fn adds_to_empty_file() {
    let tmp = TempDir::new().unwrap();
    let path = settings(tmp.path(), "claude");
    write(&path, "");
    let h = hook("my-hook", "tool --id my-hook");
    assert_eq!(
        register_session_hook(tmp.path(), "claude", &h).unwrap(),
        HookChange::Added
    );
    assert!(has_session_hook(tmp.path(), "claude", "my-hook").unwrap());
}

#[test]
fn adding_preserves_other_keys_hooks_and_order() {
    for key in ["claude", "gemini"] {
        let tmp = TempDir::new().unwrap();
        let path = settings(tmp.path(), key);
        write(&path, OTHERS);
        let h = hook("my-hook", "tool --id my-hook");
        assert_eq!(
            register_session_hook(tmp.path(), key, &h).unwrap(),
            HookChange::Added
        );
        let text = fs::read_to_string(&path).unwrap();
        let at = |needle: &str| text.find(needle).unwrap();
        assert!(at("\"zeta\"") < at("\"hooks\"") && at("\"hooks\"") < at("\"alpha\""));
        assert!(at("\"b\": 2") < at("\"a\": 1"), "{key}: nested order kept");
        assert!(at("\"PreToolUse\"") < at("\"SessionStart\""));
        let v: Value = serde_json::from_str(&text).unwrap();
        let start = v["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(start.len(), 2);
        assert_eq!(start[0]["hooks"][0]["command"], "other-start");
        assert_eq!(start[1]["hooks"][0]["command"], "tool --id my-hook");
        assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "check");
    }
}

#[test]
fn re_register_is_idempotent_and_updates_command() {
    for key in ["claude", "gemini"] {
        let tmp = TempDir::new().unwrap();
        let path = settings(tmp.path(), key);
        write(&path, OTHERS);
        let h = hook("my-hook", "tool --id my-hook");
        register_session_hook(tmp.path(), key, &h).unwrap();
        let before = fs::read_to_string(&path).unwrap();

        assert_eq!(
            register_session_hook(tmp.path(), key, &h).unwrap(),
            HookChange::Unchanged
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), before);

        let h2 = hook("my-hook", "tool --verbose --id my-hook");
        assert_eq!(
            register_session_hook(tmp.path(), key, &h2).unwrap(),
            HookChange::Updated
        );
        let v = read(&path);
        let start = v["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(start.len(), 2, "{key}: no duplicate group");
        assert_eq!(start[1]["hooks"].as_array().unwrap().len(), 1);
        assert_eq!(
            start[1]["hooks"][0]["command"],
            "tool --verbose --id my-hook"
        );
        assert_eq!(start[0]["hooks"][0]["command"], "other-start");
    }
}

#[test]
fn duplicates_collapse_to_one_entry() {
    let tmp = TempDir::new().unwrap();
    let path = settings(tmp.path(), "claude");
    write(
        &path,
        r#"{"hooks": {"SessionStart": [
            {"matcher": "", "hooks": [{"type": "command", "command": "old --id my-hook"}]},
            {"matcher": "", "hooks": [
                {"type": "command", "command": "keep"},
                {"type": "command", "command": "older --id my-hook"}
            ]},
            {"matcher": "", "hooks": [{"type": "command", "command": "oldest --id my-hook"}]}
        ]}}"#,
    );
    let h = hook("my-hook", "old --id my-hook");
    assert_eq!(
        register_session_hook(tmp.path(), "claude", &h).unwrap(),
        HookChange::Updated
    );
    assert_eq!(
        read(&path),
        json!({"hooks": {"SessionStart": [
            {"matcher": "", "hooks": [{"type": "command", "command": "old --id my-hook"}]},
            {"matcher": "", "hooks": [{"type": "command", "command": "keep"}]}
        ]}})
    );
}

#[test]
fn unregister_leaves_others_and_cleans_what_it_emptied() {
    for key in ["claude", "gemini"] {
        let tmp = TempDir::new().unwrap();
        let path = settings(tmp.path(), key);
        write(&path, OTHERS);
        let original = read(&path);
        let h = hook("my-hook", "tool --id my-hook");
        register_session_hook(tmp.path(), key, &h).unwrap();

        assert_eq!(
            unregister_session_hook(tmp.path(), key, "my-hook").unwrap(),
            HookChange::Removed
        );
        assert_eq!(read(&path), original, "{key}");
        assert!(!has_session_hook(tmp.path(), key, "my-hook").unwrap());
        assert_eq!(
            unregister_session_hook(tmp.path(), key, "my-hook").unwrap(),
            HookChange::Unchanged
        );
    }
}

#[test]
fn unregister_removes_containers_it_emptied() {
    let tmp = TempDir::new().unwrap();
    let path = settings(tmp.path(), "gemini");
    write(&path, r#"{"model": "x"}"#);
    let h = hook("my-hook", "tool --id my-hook");
    register_session_hook(tmp.path(), "gemini", &h).unwrap();
    unregister_session_hook(tmp.path(), "gemini", "my-hook").unwrap();
    assert_eq!(read(&path), json!({"model": "x"}));

    // Containers that were already empty are left alone.
    write(
        &path,
        r#"{"hooks": {"SessionStart": [], "Other": []}, "x": {"hooks": {}}}"#,
    );
    assert_eq!(
        unregister_session_hook(tmp.path(), "gemini", "my-hook").unwrap(),
        HookChange::Unchanged
    );
    assert_eq!(
        read(&path),
        json!({"hooks": {"SessionStart": [], "Other": []}, "x": {"hooks": {}}})
    );

    // Removing the last SessionStart group keeps a non-empty `hooks`.
    write(
        &path,
        r#"{"hooks": {"Other": [], "SessionStart": [{"matcher": "*", "hooks": [{"type": "command", "command": "a my-hook", "name": "my-hook"}]}]}}"#,
    );
    unregister_session_hook(tmp.path(), "gemini", "my-hook").unwrap();
    assert_eq!(read(&path), json!({"hooks": {"Other": []}}));
}

#[test]
fn unregister_on_missing_file() {
    let tmp = TempDir::new().unwrap();
    assert_eq!(
        unregister_session_hook(tmp.path(), "claude", "my-hook").unwrap(),
        HookChange::Unchanged
    );
    assert!(!settings(tmp.path(), "claude").exists());
    assert!(!has_session_hook(tmp.path(), "claude", "my-hook").unwrap());
}

#[test]
fn invalid_json_is_refused_and_untouched() {
    let tmp = TempDir::new().unwrap();
    let path = settings(tmp.path(), "claude");
    let bad = "{\"hooks\": {,}\n";
    write(&path, bad);
    let h = hook("my-hook", "tool --id my-hook");
    assert!(matches!(
        register_session_hook(tmp.path(), "claude", &h),
        Err(SessionHookError::InvalidJson { .. })
    ));
    assert!(matches!(
        unregister_session_hook(tmp.path(), "claude", "my-hook"),
        Err(SessionHookError::InvalidJson { .. })
    ));
    assert!(has_session_hook(tmp.path(), "claude", "my-hook").is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), bad);
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn wrong_types_are_refused_and_untouched() {
    let h = hook("my-hook", "tool --id my-hook");
    for (text, at) in [
        ("[]", "the top level"),
        (r#"{"hooks": []}"#, "hooks"),
        (r#"{"hooks": {"SessionStart": {}}}"#, "hooks.SessionStart"),
        (
            r#"{"hooks": {"SessionStart": ["x"]}}"#,
            "hooks.SessionStart[0]",
        ),
        (
            r#"{"hooks": {"SessionStart": [{"hooks": {}}]}}"#,
            "hooks.SessionStart[0].hooks",
        ),
    ] {
        let tmp = TempDir::new().unwrap();
        let path = settings(tmp.path(), "gemini");
        write(&path, text);
        match register_session_hook(tmp.path(), "gemini", &h) {
            Err(SessionHookError::UnexpectedType { at: got, .. }) => assert_eq!(got, at),
            other => panic!("{text}: {other:?}"),
        }
        assert!(unregister_session_hook(tmp.path(), "gemini", "my-hook").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }
}

#[test]
fn admin_entry_shape_per_agent() {
    let h = hook("my-hook", "tool --id my-hook");
    let entry = |key| {
        let e = admin_hook_entry(key, &h).unwrap();
        assert_eq!(e.format, AdminHookFormat::Json, "{key}");
        assert!(e.text.ends_with("}\n"), "{key}");
        serde_json::from_str::<Value>(&e.text).unwrap()
    };
    assert_eq!(
        entry("claude"),
        json!({"hooks": {"SessionStart": [
            {"matcher": "", "hooks": [{"type": "command", "command": "tool --id my-hook"}]}
        ]}})
    );
    assert_eq!(
        entry("gemini"),
        json!({"hooks": {"SessionStart": [
            {"matcher": "*", "hooks": [
                {"type": "command", "command": "tool --id my-hook", "name": "my-hook"}
            ]}
        ]}})
    );
}

#[test]
fn admin_file_per_platform() {
    let claude = admin_hook_file("claude");
    let gemini = admin_hook_file("gemini");
    if cfg!(target_os = "linux") {
        assert_eq!(
            claude.unwrap(),
            PathBuf::from("/etc/claude-code/managed-settings.json")
        );
        assert_eq!(
            gemini.unwrap(),
            PathBuf::from("/etc/gemini-cli/settings.json")
        );
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            claude.unwrap(),
            PathBuf::from("/Library/Application Support/ClaudeCode/managed-settings.json")
        );
        assert_eq!(
            gemini.unwrap(),
            PathBuf::from("/Library/Application Support/GeminiCli/settings.json")
        );
    } else if cfg!(windows) {
        assert_eq!(
            claude.unwrap(),
            PathBuf::from(r"C:\Program Files\ClaudeCode\managed-settings.json")
        );
        assert_eq!(
            gemini.unwrap(),
            PathBuf::from(r"C:\ProgramData\gemini-cli\settings.json")
        );
    }
}

#[test]
fn admin_presence_reads_the_given_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("managed.json");
    let h = hook("my-hook", "tool --id my-hook");
    assert!(!admin_hook_present_in(&path, "claude", "my-hook"));
    fs::write(&path, "not json").unwrap();
    assert!(!admin_hook_present_in(&path, "claude", "my-hook"));
    for key in ["claude", "gemini", "cursor"] {
        let mut entry: Value =
            serde_json::from_str(&admin_hook_entry(key, &h).unwrap().text).unwrap();
        entry["other"] = json!(true);
        fs::write(&path, entry.to_string()).unwrap();
        assert!(admin_hook_present_in(&path, key, "my-hook"), "{key}");
        assert!(!admin_hook_present_in(&path, key, "other-hook"));
        assert!(!admin_hook_present_in(&path, "copilot", "my-hook"));
        assert!(!admin_hook_present_in(&path, "pi", "my-hook"));
    }
}

#[test]
fn notice_format_per_agent() {
    let text = "Ready: 3 \"items\"\nsynced";
    for (key, field) in [
        ("claude", "systemMessage"),
        ("gemini", "systemMessage"),
        ("codex", "systemMessage"),
        ("cursor", "additional_context"),
    ] {
        let out = format_notice(key, text).unwrap();
        assert!(!out.contains('\n'), "{key}: one line");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v, json!({ field: text }), "{key}");
    }
    assert_eq!(format_notice("pi", text).unwrap(), text);
}

#[test]
fn unrelated_values_round_trip() {
    let tmp = TempDir::new().unwrap();
    let path = settings(tmp.path(), "claude");
    let text = r#"{"n": null, "t": true, "i": -3, "u": 18446744073709551615, "f": 1.5, "s": "é\u00e9", "a": [1, {"y": 1, "x": 2}]}"#;
    write(&path, text);
    register_session_hook(tmp.path(), "claude", &hook("my-hook", "t my-hook")).unwrap();
    unregister_session_hook(tmp.path(), "claude", "my-hook").unwrap();
    let out = fs::read_to_string(&path).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap(),
        serde_json::from_str::<Value>(text).unwrap()
    );
    assert!(out.find("\"y\"").unwrap() < out.find("\"x\"").unwrap());
    assert!(out.find("\"n\"").unwrap() < out.find("\"a\"").unwrap());
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn symlinked_settings_are_written_through_to_the_target() {
        let tmp = TempDir::new().unwrap();
        let real = tmp.path().join("dotfiles/claude-settings.json");
        write(&real, r#"{"keep": 1}"#);
        let path = settings(tmp.path(), "claude");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        symlink(&real, &path).unwrap();

        let h = hook("my-hook", "tool --id my-hook");
        register_session_hook(tmp.path(), "claude", &h).unwrap();
        assert!(fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_link(&path).unwrap(), real);
        let v = read(&real);
        assert_eq!(v["keep"], 1);
        assert!(has_session_hook(tmp.path(), "claude", "my-hook").unwrap());
        // No temporary files left beside the target or the link.
        assert_eq!(fs::read_dir(real.parent().unwrap()).unwrap().count(), 1);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);

        unregister_session_hook(tmp.path(), "claude", "my-hook").unwrap();
        assert!(fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(read(&real), json!({"keep": 1}));
    }

    #[test]
    fn dangling_symlink_is_refused() {
        let tmp = TempDir::new().unwrap();
        let path = settings(tmp.path(), "gemini");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        symlink(tmp.path().join("gone.json"), &path).unwrap();
        let h = hook("my-hook", "tool --id my-hook");
        assert!(matches!(
            register_session_hook(tmp.path(), "gemini", &h),
            Err(SessionHookError::DanglingSymlink(_))
        ));
        assert!(!tmp.path().join("gone.json").exists());
        assert!(fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn existing_permissions_are_kept() {
        let tmp = TempDir::new().unwrap();
        let path = settings(tmp.path(), "claude");
        write(&path, "{}");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        register_session_hook(tmp.path(), "claude", &hook("my-hook", "t my-hook")).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }
}
