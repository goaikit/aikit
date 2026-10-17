use super::*;

const GOLDEN: &str = include_str!("../testdata/pi-extension.ts");

fn pi_hook(id: &str, last: &str) -> SessionHook {
    SessionHook::from_argv(
        id,
        "/opt/my tool/bin/tool",
        ["managed", "apply", "--hook", id, last],
    )
}

fn ext(home: &Path, id: &str) -> PathBuf {
    settings(home, "pi").join(format!("{id}.ts"))
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn renders_the_golden_file() {
    let tmp = TempDir::new().unwrap();
    assert_eq!(
        settings(tmp.path(), "pi"),
        tmp.path().join(".pi/agent/extensions")
    );
    assert_eq!(
        register_session_hook(tmp.path(), "pi", &pi_hook("my-hook", "say \"hi\"\n")).unwrap(),
        HookChange::Added
    );
    assert_eq!(
        fs::read_to_string(ext(tmp.path(), "my-hook")).unwrap(),
        GOLDEN
    );
    assert!(has_session_hook(tmp.path(), "pi", "my-hook").unwrap());
    assert_eq!(names(&settings(tmp.path(), "pi")), ["my-hook.ts"]);
}

#[test]
fn the_cycle_reports_each_change_and_leaves_no_temp_files() {
    let tmp = TempDir::new().unwrap();
    let dir = settings(tmp.path(), "pi");
    write(&dir.join("other.ts"), "export default function () {}\n");
    let h = pi_hook("my-hook", "a");
    assert_eq!(
        register_session_hook(tmp.path(), "pi", &h).unwrap(),
        HookChange::Added
    );
    assert_eq!(
        register_session_hook(tmp.path(), "pi", &h).unwrap(),
        HookChange::Unchanged
    );
    assert_eq!(
        register_session_hook(tmp.path(), "pi", &pi_hook("my-hook", "b")).unwrap(),
        HookChange::Updated
    );
    assert!(fs::read_to_string(ext(tmp.path(), "my-hook"))
        .unwrap()
        .contains(r#""my-hook", "b"]"#));
    assert_eq!(names(&dir), ["my-hook.ts", "other.ts"]);
    assert_eq!(
        unregister_session_hook(tmp.path(), "pi", "my-hook").unwrap(),
        HookChange::Removed
    );
    assert!(!has_session_hook(tmp.path(), "pi", "my-hook").unwrap());
    assert_eq!(
        unregister_session_hook(tmp.path(), "pi", "my-hook").unwrap(),
        HookChange::Unchanged
    );
    assert_eq!(names(&dir), ["other.ts"]);
}

#[test]
fn an_edited_file_with_our_marker_is_replaced() {
    let tmp = TempDir::new().unwrap();
    let path = ext(tmp.path(), "my-hook");
    write(
        &path,
        "// aikit session hook my-hook; generated, do not edit\r\nedited\r\n",
    );
    assert!(has_session_hook(tmp.path(), "pi", "my-hook").unwrap());
    assert_eq!(
        register_session_hook(tmp.path(), "pi", &pi_hook("my-hook", "say \"hi\"\n")).unwrap(),
        HookChange::Updated
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), GOLDEN);
}

#[test]
fn a_file_without_our_marker_is_never_overwritten_or_deleted() {
    for text in [
        "export default function () {}\n",
        "",
        "// aikit session hook other-hook; generated, do not edit\n",
        "\n// aikit session hook my-hook; generated, do not edit\n",
    ] {
        let tmp = TempDir::new().unwrap();
        let path = ext(tmp.path(), "my-hook");
        write(&path, text);
        match register_session_hook(tmp.path(), "pi", &pi_hook("my-hook", "a")) {
            Err(SessionHookError::NotOurs(p)) => assert_eq!(p, path),
            other => panic!("{text:?}: {other:?}"),
        }
        assert!(!has_session_hook(tmp.path(), "pi", "my-hook").unwrap());
        assert_eq!(
            unregister_session_hook(tmp.path(), "pi", "my-hook").unwrap(),
            HookChange::Unchanged
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }
}

#[test]
fn other_ids_are_left_alone() {
    let tmp = TempDir::new().unwrap();
    register_session_hook(tmp.path(), "pi", &pi_hook("other-hook", "a")).unwrap();
    let before = fs::read_to_string(ext(tmp.path(), "other-hook")).unwrap();
    assert_eq!(
        unregister_session_hook(tmp.path(), "pi", "my-hook").unwrap(),
        HookChange::Unchanged
    );
    assert!(!has_session_hook(tmp.path(), "pi", "my-hook").unwrap());
    assert_eq!(
        fs::read_to_string(ext(tmp.path(), "other-hook")).unwrap(),
        before
    );
}

#[test]
fn a_hook_without_argv_is_refused() {
    let tmp = TempDir::new().unwrap();
    assert!(matches!(
        register_session_hook(tmp.path(), "pi", &hook("my-hook", "tool --id my-hook")),
        Err(SessionHookError::NeedsArgv(id)) if id == "my-hook"
    ));
    assert!(!settings(tmp.path(), "pi").exists());
}

#[cfg(unix)]
#[test]
fn a_dangling_symlink_is_refused() {
    let tmp = TempDir::new().unwrap();
    let path = ext(tmp.path(), "my-hook");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(tmp.path().join("missing.ts"), &path).unwrap();
    assert!(matches!(
        register_session_hook(tmp.path(), "pi", &pi_hook("my-hook", "a")),
        Err(SessionHookError::DanglingSymlink(_))
    ));
}

#[test]
fn no_admin_location() {
    assert!(admin_hook_file("pi").is_none());
    assert!(matches!(
        admin_hook_entry("pi", &pi_hook("my-hook", "a")),
        Err(SessionHookError::Unsupported(_))
    ));
}

/// The golden file parses as TypeScript. Skipped when bun is not installed;
/// CI installs it.
#[test]
fn golden_file_parses_with_bun() {
    let Ok(out) = std::process::Command::new("bun")
        .args(["build", "--no-bundle"])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/session_hooks/testdata/pi-extension.ts"
        ))
        .output()
    else {
        eprintln!("bun not found; skipping the parse check");
        return;
    };
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
