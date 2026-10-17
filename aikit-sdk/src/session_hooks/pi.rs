//! The `OwnedFile` layout: one TypeScript extension file per hook, rendered
//! whole from a fixed template and never merged (ADR 0025).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::settings::{exists, write_text};
use super::{HookChange, SessionHook, SessionHookError};

/// The first line of every file this module renders.
pub(super) fn marker(id: &str) -> String {
    format!("// aikit session hook {id}; generated, do not edit")
}

/// The extension file of hook `id` in the extensions folder `dir`.
pub(super) fn file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.ts"))
}

fn js_string(s: &str) -> String {
    serde_json::to_string(s).expect("a string always serializes")
}

/// The extension for `hook`, which runs `argv` (program, then arguments).
pub(super) fn render(id: &str, argv: &[String]) -> String {
    let (program, args) = argv.split_first().expect("validated: argv has a program");
    let args = args
        .iter()
        .map(|a| js_string(a))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"{marker}
//
// Runs a program when a Pi session starts, is new or is resumed, and shows
// what it prints to the user as a notice. aikit replaces or removes this file
// as a whole; edits are lost.

const PROGRAM = {program};
const ARGS = [{args}];
const REASONS = ["startup", "new", "resume"];

export default function (pi: any) {{
  pi.on("session_start", async (event: any, ctx: any) => {{
    if (!REASONS.includes(event?.reason)) return;
    try {{
      const result = await pi.exec(PROGRAM, ARGS, {{ timeout: 10000 }});
      const notice = String(result?.stdout ?? "").trim();
      if (notice && ctx?.hasUI) ctx.ui.notify(notice, "warning");
    }} catch {{
      // A missing or failing program never breaks the session.
    }}
  }});
}}
"#,
        marker = marker(id),
        program = js_string(program),
    )
}

/// The file's bytes, or `None` when nothing is there.
fn read(path: &Path) -> Result<Option<Vec<u8>>, SessionHookError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if exists(path)? {
                return Err(SessionHookError::DanglingSymlink(path.to_path_buf()));
            }
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

/// Whether `bytes` start with the marker line of hook `id`.
fn is_marked(bytes: &[u8], id: &str) -> bool {
    let line = bytes.split(|b| *b == b'\n').next().unwrap_or_default();
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    line == marker(id).as_bytes()
}

pub(super) fn register(dir: &Path, hook: &SessionHook) -> Result<HookChange, SessionHookError> {
    let argv = hook
        .argv
        .as_deref()
        .ok_or_else(|| SessionHookError::NeedsArgv(hook.id.clone()))?;
    let path = file(dir, &hook.id);
    let text = render(&hook.id, argv);
    let change = match read(&path)? {
        None => HookChange::Added,
        Some(bytes) if !is_marked(&bytes, &hook.id) => {
            return Err(SessionHookError::NotOurs(path));
        }
        Some(bytes) if bytes == text.as_bytes() => return Ok(HookChange::Unchanged),
        Some(_) => HookChange::Updated,
    };
    write_text(&path, &text)?;
    Ok(change)
}

pub(super) fn unregister(dir: &Path, id: &str) -> Result<HookChange, SessionHookError> {
    let path = file(dir, id);
    match read(&path)? {
        Some(bytes) if is_marked(&bytes, id) => {
            fs::remove_file(&path)?;
            Ok(HookChange::Removed)
        }
        _ => Ok(HookChange::Unchanged),
    }
}

pub(super) fn has(dir: &Path, id: &str) -> Result<bool, SessionHookError> {
    Ok(read(&file(dir, id))?.is_some_and(|bytes| is_marked(&bytes, id)))
}
