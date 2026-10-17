//! Hooks held in a JSON settings file, in the `Grouped` and `Flat` layouts,
//! and the atomic file writing every layout shares.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::ordered::{Json, Obj};
use super::{has_word, HookChange, HookRow, Layout, Marker, SessionHook, SessionHookError};

fn is_grouped(row: &HookRow) -> bool {
    matches!(row.layout, Layout::Grouped { .. })
}

fn new_entry(row: &HookRow, hook: &SessionHook) -> Json {
    let mut entry = Obj::default();
    if is_grouped(row) {
        entry.insert("type", Json::String("command".into()));
    }
    entry.insert("command", Json::String(hook.command.clone()));
    if let Some(timeout) = row.timeout {
        entry.insert("timeout", Json::Number(timeout.into()));
    }
    if row.marker == Marker::NameField {
        entry.insert("name", Json::String(hook.id.clone()));
    }
    Json::Object(entry)
}

fn new_group(row: &HookRow, hook: &SessionHook, matcher: &str) -> Json {
    let mut group = Obj::default();
    group.insert("matcher", Json::String(matcher.into()));
    group.insert("hooks", Json::Array(vec![new_entry(row, hook)]));
    Json::Object(group)
}

/// A settings object holding only this hook, in the row's shape.
pub(super) fn fragment(row: &HookRow, hook: &SessionHook) -> Json {
    let item = match row.layout {
        Layout::Grouped { matcher } => new_group(row, hook, matcher),
        _ => new_entry(row, hook),
    };
    let mut hooks = Obj::default();
    hooks.insert(row.event, Json::Array(vec![item]));
    let mut root = Obj::default();
    if matches!(row.layout, Layout::Flat) {
        root.insert("version", Json::Number(1.into()));
    }
    root.insert("hooks", Json::Object(hooks));
    Json::Object(root)
}

fn is_ours(entry: &Json, row: &HookRow, id: &str) -> bool {
    match row.marker {
        Marker::NameField => entry.get("name").and_then(Json::as_str) == Some(id),
        Marker::CommandWord => entry
            .get("command")
            .and_then(Json::as_str)
            .is_some_and(|c| has_word(c, id)),
        Marker::FirstLine => false,
    }
}

fn unexpected(path: &Path, at: impl Into<String>, expected: &'static str) -> SessionHookError {
    SessionHookError::UnexpectedType {
        path: path.to_path_buf(),
        at: at.into(),
        expected,
    }
}

fn is_one(value: &Json) -> bool {
    matches!(value, Json::Number(n) if n.as_u64() == Some(1))
}

/// Checks the shape of `root` and returns the list under `hooks.<event>`
/// when present: groups for `Grouped`, entries for `Flat`.
fn event_list<'a>(
    root: &'a mut Json,
    row: &HookRow,
    path: &Path,
) -> Result<Option<&'a mut Vec<Json>>, SessionHookError> {
    let obj = root
        .as_object_mut()
        .ok_or_else(|| unexpected(path, "the top level", "an object"))?;
    if matches!(row.layout, Layout::Flat) && obj.get("version").is_some_and(|v| !is_one(v)) {
        return Err(unexpected(path, "version", "1"));
    }
    let Some(hooks) = obj.get_mut("hooks") else {
        return Ok(None);
    };
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| unexpected(path, "hooks", "an object"))?;
    let Some(list) = hooks.get_mut(row.event) else {
        return Ok(None);
    };
    let at = format!("hooks.{}", row.event);
    let list = list
        .as_array_mut()
        .ok_or_else(|| unexpected(path, at.as_str(), "an array"))?;
    for (i, item) in list.iter().enumerate() {
        let item = item
            .as_object()
            .ok_or_else(|| unexpected(path, format!("{at}[{i}]"), "an object"))?;
        if is_grouped(row) && item.get("hooks").is_some_and(|h| !h.is_array()) {
            return Err(unexpected(path, format!("{at}[{i}].hooks"), "an array"));
        }
    }
    Ok(Some(list))
}

fn group_entries(group: &mut Json) -> Option<&mut Vec<Json>> {
    group.get_mut("hooks").and_then(Json::as_array_mut)
}

/// Updates the first of the caller's entries met so far (across calls, as
/// recorded in `change`) and removes any later duplicate.
fn update_entries(
    entries: &mut Vec<Json>,
    row: &HookRow,
    hook: &SessionHook,
    change: &mut Option<HookChange>,
) {
    let mut i = 0;
    while i < entries.len() {
        if !is_ours(&entries[i], row, &hook.id) {
            i += 1;
        } else if change.is_some() {
            // A duplicate of the entry already handled.
            entries.remove(i);
            *change = Some(HookChange::Updated);
        } else {
            let entry = entries[i].as_object_mut().expect("is_ours needs an object");
            if entry.get("command").and_then(Json::as_str) == Some(hook.command.as_str()) {
                *change = Some(HookChange::Unchanged);
            } else {
                entry.insert("command", Json::String(hook.command.clone()));
                *change = Some(HookChange::Updated);
            }
            i += 1;
        }
    }
}

pub(super) fn add_to(
    root: &mut Json,
    row: &HookRow,
    hook: &SessionHook,
    path: &Path,
) -> Result<HookChange, SessionHookError> {
    if event_list(root, row, path)?.is_none() {
        let obj = root.as_object_mut().expect("checked by event_list");
        let hooks = obj.get_or_insert_with("hooks", || Json::Object(Obj::default()));
        hooks
            .as_object_mut()
            .expect("checked by event_list")
            .insert(row.event, Json::Array(Vec::new()));
    }
    let list = event_list(root, row, path)?.expect("just inserted");

    let mut change = None;
    match row.layout {
        Layout::Grouped { matcher } => {
            list.retain_mut(|group| {
                let Some(entries) = group_entries(group) else {
                    return true;
                };
                let before = entries.len();
                update_entries(entries, row, hook, &mut change);
                // Drop a group only when removing duplicates emptied it.
                entries.len() == before || !entries.is_empty()
            });
            if change.is_none() {
                list.push(new_group(row, hook, matcher));
            }
        }
        _ => {
            update_entries(list, row, hook, &mut change);
            if change.is_none() {
                list.push(new_entry(row, hook));
            }
        }
    }
    let change = change.unwrap_or(HookChange::Added);
    if matches!(row.layout, Layout::Flat) && change != HookChange::Unchanged {
        let obj = root.as_object_mut().expect("checked by event_list");
        if obj.get("version").is_none() {
            obj.insert_first("version", Json::Number(1.into()));
        }
    }
    Ok(change)
}

pub(super) fn remove_from(
    root: &mut Json,
    row: &HookRow,
    id: &str,
    path: &Path,
) -> Result<HookChange, SessionHookError> {
    let Some(list) = event_list(root, row, path)? else {
        return Ok(HookChange::Unchanged);
    };
    let mut removed = false;
    if is_grouped(row) {
        list.retain_mut(|group| {
            let Some(entries) = group_entries(group) else {
                return true;
            };
            let before = entries.len();
            entries.retain(|e| !is_ours(e, row, id));
            if entries.len() == before {
                return true;
            }
            removed = true;
            !entries.is_empty()
        });
    } else {
        let before = list.len();
        list.retain(|e| !is_ours(e, row, id));
        removed = list.len() != before;
    }
    if !removed {
        return Ok(HookChange::Unchanged);
    }
    if list.is_empty() {
        let obj = root.as_object_mut().expect("checked by event_list");
        let hooks = obj
            .get_mut("hooks")
            .and_then(Json::as_object_mut)
            .expect("checked by event_list");
        hooks.shift_remove(row.event);
        if hooks.is_empty() {
            obj.shift_remove("hooks");
        }
    }
    Ok(HookChange::Removed)
}

pub(super) fn contains(
    root: &Json,
    row: &HookRow,
    id: &str,
    path: &Path,
) -> Result<bool, SessionHookError> {
    let mut root = root.clone();
    let Some(list) = event_list(&mut root, row, path)? else {
        return Ok(false);
    };
    Ok(if is_grouped(row) {
        list.iter().any(|g| {
            g.get("hooks")
                .and_then(Json::as_array)
                .is_some_and(|es| es.iter().any(|e| is_ours(e, row, id)))
        })
    } else {
        list.iter().any(|e| is_ours(e, row, id))
    })
}

/// Whether anything (including a dangling link) is at `path`.
pub(super) fn exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Reads the settings at `path`: an empty object when the file is missing or
/// blank.
pub(super) fn read_settings(path: &Path) -> Result<Json, SessionHookError> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if exists(path)? {
                return Err(SessionHookError::DanglingSymlink(path.to_path_buf()));
            }
            return Ok(Json::Object(Obj::default()));
        }
        Err(e) => return Err(e.into()),
    };
    if text.trim().is_empty() {
        return Ok(Json::Object(Obj::default()));
    }
    let value: Json =
        serde_json::from_str(&text).map_err(|source| SessionHookError::InvalidJson {
            path: path.to_path_buf(),
            source,
        })?;
    if !value.is_object() {
        return Err(unexpected(path, "the top level", "an object"));
    }
    Ok(value)
}

/// Pretty-prints `root` with two-space indentation and a trailing newline.
pub(super) fn to_text(root: &Json) -> String {
    let mut text = serde_json::to_string_pretty(root).expect("plain JSON always serializes");
    text.push('\n');
    text
}

/// Writes `root` to `path` atomically, through a symbolic link to its target.
pub(super) fn write_settings(path: &Path, root: &Json) -> Result<(), SessionHookError> {
    write_text(path, &to_text(root))
}

/// Writes `text` to `path` atomically: to a temporary file in the same folder
/// (named so that it never ends in the target's extension), then renamed into
/// place. A symbolic link is resolved and its target replaced, keeping the
/// target's permissions; a dangling link is refused.
pub(super) fn write_text(path: &Path, text: &str) -> Result<(), SessionHookError> {
    let target = if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        fs::canonicalize(path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => SessionHookError::DanglingSymlink(path.to_path_buf()),
            _ => SessionHookError::Io(e),
        })?
    } else {
        path.to_path_buf()
    };
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    fs::create_dir_all(&dir)?;

    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut tmp = tempfile::Builder::new()
        .prefix(&format!(".{name}.tmp-"))
        .tempfile_in(&dir)?;
    tmp.write_all(text.as_bytes())?;
    tmp.as_file().sync_all()?;
    if let Ok(meta) = fs::metadata(&target) {
        fs::set_permissions(tmp.path(), meta.permissions())?;
    }
    tmp.persist(&target).map_err(|e| e.error)?;
    Ok(())
}
