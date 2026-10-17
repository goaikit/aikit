use super::*;

const ENTRY: &str = "tool --id my-hook";

fn ours() -> Value {
    json!({"command": ENTRY, "timeout": 10})
}

#[test]
fn adds_to_missing_and_empty_file_with_version_first() {
    for initial in [None, Some(""), Some("{}")] {
        let tmp = TempDir::new().unwrap();
        let path = settings(tmp.path(), "cursor");
        assert_eq!(path, tmp.path().join(".cursor/hooks.json"));
        if let Some(text) = initial {
            write(&path, text);
        }
        assert_eq!(
            register_session_hook(tmp.path(), "cursor", &hook("my-hook", ENTRY)).unwrap(),
            HookChange::Added
        );
        assert_eq!(
            read(&path),
            json!({"version": 1, "hooks": {"sessionStart": [ours()]}})
        );
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("{\n  \"version\": 1,"));
        assert!(has_session_hook(tmp.path(), "cursor", "my-hook").unwrap());
    }
}

#[test]
fn a_file_without_version_gets_it_first() {
    let tmp = TempDir::new().unwrap();
    let path = settings(tmp.path(), "cursor");
    write(&path, r#"{"hooks": {"stop": [{"command": "s"}]}}"#);
    register_session_hook(tmp.path(), "cursor", &hook("my-hook", ENTRY)).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("{\n  \"version\": 1,"));
    assert_eq!(
        read(&path),
        json!({"version": 1, "hooks": {"stop": [{"command": "s"}], "sessionStart": [ours()]}})
    );
}

#[test]
fn other_hooks_are_kept_and_the_cycle_reports_each_change() {
    let tmp = TempDir::new().unwrap();
    let path = settings(tmp.path(), "cursor");
    let initial = r#"{
  "version": 1,
  "hooks": {
    "sessionStart": [{"command": "./init.sh"}],
    "afterFileEdit": [{"command": "./format.sh", "matcher": "Write"}]
  },
  "z": true
}
"#;
    write(&path, initial);
    let original = read(&path);
    let h = hook("my-hook", ENTRY);
    assert_eq!(
        register_session_hook(tmp.path(), "cursor", &h).unwrap(),
        HookChange::Added
    );
    assert_eq!(
        read(&path)["hooks"]["sessionStart"],
        json!([{"command": "./init.sh"}, ours()])
    );
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.find("\"sessionStart\"").unwrap() < text.find("\"afterFileEdit\"").unwrap());
    assert!(text.find("\"hooks\"").unwrap() < text.find("\"z\"").unwrap());
    assert_eq!(
        register_session_hook(tmp.path(), "cursor", &h).unwrap(),
        HookChange::Unchanged
    );
    assert_eq!(
        register_session_hook(tmp.path(), "cursor", &hook("my-hook", "tool2 --id my-hook"))
            .unwrap(),
        HookChange::Updated
    );
    assert_eq!(
        read(&path)["hooks"]["sessionStart"][1],
        json!({"command": "tool2 --id my-hook", "timeout": 10})
    );
    assert_eq!(
        unregister_session_hook(tmp.path(), "cursor", "my-hook").unwrap(),
        HookChange::Removed
    );
    assert_eq!(read(&path), original);
    assert_eq!(
        unregister_session_hook(tmp.path(), "cursor", "my-hook").unwrap(),
        HookChange::Unchanged
    );
}

#[test]
fn duplicates_collapse_and_emptied_lists_are_removed() {
    let tmp = TempDir::new().unwrap();
    let path = settings(tmp.path(), "cursor");
    write(
        &path,
        r#"{"version": 1, "hooks": {"sessionStart": [
            {"command": "a --id my-hook"}, {"command": "keep"}, {"command": "b my-hook"}
        ]}}"#,
    );
    assert_eq!(
        register_session_hook(tmp.path(), "cursor", &hook("my-hook", "a --id my-hook")).unwrap(),
        HookChange::Updated
    );
    assert_eq!(
        read(&path),
        json!({"version": 1, "hooks": {"sessionStart": [
            {"command": "a --id my-hook"}, {"command": "keep"}
        ]}})
    );

    write(
        &path,
        r#"{"version": 1, "hooks": {"sessionStart": [{"command": "a my-hook"}]}}"#,
    );
    unregister_session_hook(tmp.path(), "cursor", "my-hook").unwrap();
    assert_eq!(read(&path), json!({"version": 1}));
}

#[test]
fn other_versions_and_wrong_shapes_are_refused_and_untouched() {
    for (text, at) in [
        (r#"{"version": 2}"#, "version"),
        (r#"{"version": "1"}"#, "version"),
        (r#"{"version": 1, "hooks": []}"#, "hooks"),
        (
            r#"{"version": 1, "hooks": {"sessionStart": {}}}"#,
            "hooks.sessionStart",
        ),
        (
            r#"{"version": 1, "hooks": {"sessionStart": ["x"]}}"#,
            "hooks.sessionStart[0]",
        ),
    ] {
        let tmp = TempDir::new().unwrap();
        let path = settings(tmp.path(), "cursor");
        write(&path, text);
        match register_session_hook(tmp.path(), "cursor", &hook("my-hook", ENTRY)) {
            Err(SessionHookError::UnexpectedType { at: got, .. }) => assert_eq!(got, at),
            other => panic!("{text}: {other:?}"),
        }
        assert!(unregister_session_hook(tmp.path(), "cursor", "my-hook").is_err());
        assert!(has_session_hook(tmp.path(), "cursor", "my-hook").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }
}

#[test]
fn admin_entry_and_file() {
    let entry = admin_hook_entry("cursor", &hook("my-hook", ENTRY)).unwrap();
    assert_eq!(entry.format, AdminHookFormat::Json);
    assert_eq!(
        entry.text,
        "{\n  \"version\": 1,\n  \"hooks\": {\n    \"sessionStart\": [\n      {\n        \
         \"command\": \"tool --id my-hook\",\n        \"timeout\": 10\n      }\n    ]\n  }\n}\n"
    );
    let file = admin_hook_file("cursor").unwrap();
    if cfg!(target_os = "linux") {
        assert_eq!(file, PathBuf::from("/etc/cursor/hooks.json"));
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            file,
            PathBuf::from("/Library/Application Support/Cursor/hooks.json")
        );
    } else if cfg!(windows) {
        assert_eq!(file, PathBuf::from(r"C:\ProgramData\Cursor\hooks.json"));
    }
}
