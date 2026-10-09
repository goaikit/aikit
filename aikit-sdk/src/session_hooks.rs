//! Session-start hooks: run a command whenever an agent session starts.
//!
//! For a tool that wants to run a command whenever an agent session starts,
//! this module records, per agent, whether the agent supports such a hook and
//! where it is configured, and offers:
//!
//! - [`register_session_hook`] / [`unregister_session_hook`] /
//!   [`has_session_hook`]: manage one hook in the agent's per-user settings
//!   file, merging into whatever the file already holds;
//! - [`admin_hook_file`] / [`admin_hook_entry`] / [`admin_hook_present`]: the
//!   machine-wide settings file an administrator manages, the fragment to
//!   place in it, and whether it is already there (this module never writes
//!   administrator files);
//! - [`format_notice`]: the exact text a hook prints on standard output to
//!   show the user a one-line notice.
//!
//! Only agents whose hook format has been verified are in the table
//! ([`session_hook_agents`]); every other key is unsupported. Both supported
//! agents keep hooks in a JSON settings file under
//! `hooks.SessionStart[].hooks[]`, each group carrying a `matcher`.
//!
//! # Identifying a hook
//!
//! A [`SessionHook`] has a stable `id` chosen by the caller. The `id` must be
//! made of ASCII letters, digits, `-` and `_`, and the hook's `command` must
//! contain it as a whole word (not directly preceded or followed by one of
//! those characters), for example `mytool on-session-start --id mytool-hook`.
//! An existing entry is recognised as the caller's:
//!
//! - for `gemini`, by its `name` field, which this module sets to the `id`;
//! - for `claude`, whose hook entries carry no free-form name, by the `id`
//!   appearing as a whole word in the entry's `command`.
//!
//! Choose a distinctive `id`: any entry matching it is treated as the
//! caller's and may be updated or removed.
//!
//! # Writing safety
//!
//! A settings file is only rewritten when it parses as a JSON object whose
//! `hooks` (if present) is an object, whose `hooks.SessionStart` (if present)
//! is an array of objects, and whose groups' `hooks` (if present) are arrays.
//! Anything else is refused with an error and the file is left untouched.
//! Every other key, group and hook is kept, in its original order. The file is
//! written to a temporary file in the same folder and renamed into place,
//! pretty-printed with two-space indentation and a trailing newline; the
//! folder is created if missing. When the settings file is a symbolic link,
//! the link is resolved and its target is replaced atomically, so the link
//! itself stays a link; a dangling link is refused.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use ordered::{Json, Obj};

/// What one agent supports for session-start hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHookSupport {
    /// The agent's catalog key (e.g. `claude`).
    pub key: &'static str,
    /// The per-user settings file holding the hook, relative to the home
    /// directory (e.g. `.claude/settings.json`).
    pub user_settings: PathBuf,
    /// A note to show the user after registering, when the agent needs the
    /// user to act before a hook changed outside it takes effect.
    pub approval_note: Option<&'static str>,
}

/// One session-start hook, identified by `id` (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHook {
    /// Stable marker identifying the caller's hook. ASCII letters, digits,
    /// `-` and `_` only; must appear as a whole word in `command`.
    pub id: String,
    /// The command line the agent runs when a session starts.
    pub command: String,
}

/// What [`register_session_hook`] or [`unregister_session_hook`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookChange {
    /// The hook was not there and was added.
    Added,
    /// The hook was there with a different command, which was replaced.
    Updated,
    /// The hook was removed.
    Removed,
    /// Nothing needed to change; the file was not written.
    Unchanged,
}

/// Error from the session-hook functions.
#[derive(Debug)]
pub enum SessionHookError {
    /// The agent has no verified session-start hook support.
    Unsupported(String),
    /// The id is empty or contains characters other than ASCII letters,
    /// digits, `-` and `_`.
    InvalidId(String),
    /// The command does not contain the id as a whole word.
    CommandMissingId {
        /// The hook id.
        id: String,
        /// The command that lacks it.
        command: String,
    },
    /// The settings file is not valid JSON.
    InvalidJson {
        /// The settings file.
        path: PathBuf,
        /// The parse error.
        source: serde_json::Error,
    },
    /// A value in the settings file has a type this module does not expect;
    /// the file is left untouched.
    UnexpectedType {
        /// The settings file.
        path: PathBuf,
        /// Where the value is (e.g. `hooks.SessionStart`).
        at: String,
        /// The type that was expected there.
        expected: &'static str,
    },
    /// The settings file is a symbolic link whose target does not exist.
    DanglingSymlink(PathBuf),
    /// A filesystem operation failed.
    Io(io::Error),
}

impl fmt::Display for SessionHookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionHookError::Unsupported(key) => {
                write!(f, "agent {key:?} has no supported session-start hook")
            }
            SessionHookError::InvalidId(id) => write!(
                f,
                "invalid hook id {id:?}: use ASCII letters, digits, '-' and '_' only"
            ),
            SessionHookError::CommandMissingId { id, command } => write!(
                f,
                "hook command {command:?} does not contain the hook id {id:?} as a whole word"
            ),
            SessionHookError::InvalidJson { path, source } => write!(
                f,
                "settings file is not valid JSON, refusing to modify {}: {source}",
                path.display()
            ),
            SessionHookError::UnexpectedType { path, at, expected } => write!(
                f,
                "unexpected value at {at} in {} (expected {expected}), refusing to modify it",
                path.display()
            ),
            SessionHookError::DanglingSymlink(p) => write!(
                f,
                "settings file is a symbolic link to a missing target: {}",
                p.display()
            ),
            SessionHookError::Io(e) => write!(f, "filesystem error: {e}"),
        }
    }
}

impl Error for SessionHookError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            SessionHookError::InvalidJson { source, .. } => Some(source),
            SessionHookError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for SessionHookError {
    fn from(e: io::Error) -> Self {
        SessionHookError::Io(e)
    }
}

/// How an agent's hook entry is recognised as the caller's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    /// The entry's `name` field equals the id; the entry carries `name`.
    NameField,
    /// The id appears as a whole word in the entry's `command`.
    CommandWord,
}

/// One row of the session-hook table.
struct HookRow {
    key: &'static str,
    user_settings: &'static str,
    admin_linux: &'static str,
    admin_macos: &'static str,
    admin_windows: &'static str,
    /// The `matcher` of the group a new hook is added in.
    matcher: &'static str,
    marker: Marker,
    approval_note: Option<&'static str>,
}

/// Agents with verified session-start hook support, in catalog order.
const HOOK_ROWS: &[HookRow] = &[
    HookRow {
        key: "claude",
        user_settings: ".claude/settings.json",
        admin_linux: "/etc/claude-code/managed-settings.json",
        admin_macos: "/Library/Application Support/ClaudeCode/managed-settings.json",
        admin_windows: r"C:\Program Files\ClaudeCode\managed-settings.json",
        matcher: "",
        marker: Marker::CommandWord,
        approval_note: Some(
            "The agent may ask you to review the new hook in its hooks menu (/hooks) \
             before it takes effect.",
        ),
    },
    HookRow {
        key: "gemini",
        user_settings: ".gemini/settings.json",
        admin_linux: "/etc/gemini-cli/settings.json",
        admin_macos: "/Library/Application Support/GeminiCli/settings.json",
        admin_windows: r"C:\ProgramData\gemini-cli\settings.json",
        matcher: "*",
        marker: Marker::NameField,
        approval_note: None,
    },
];

fn find_row(key: &str) -> Option<&'static HookRow> {
    HOOK_ROWS.iter().find(|r| r.key == key)
}

fn supported_row(key: &str) -> Result<&'static HookRow, SessionHookError> {
    find_row(key).ok_or_else(|| SessionHookError::Unsupported(key.to_string()))
}

/// Returns what the agent `key` supports for session-start hooks, or `None`
/// when it has no verified support.
pub fn session_hook_support(key: &str) -> Option<SessionHookSupport> {
    find_row(key).map(|r| SessionHookSupport {
        key: r.key,
        user_settings: PathBuf::from(r.user_settings),
        approval_note: r.approval_note,
    })
}

/// Returns the keys of every agent with session-start hook support.
pub fn session_hook_agents() -> Vec<&'static str> {
    HOOK_ROWS.iter().map(|r| r.key).collect()
}

/// Adds the hook to the agent's per-user settings file under `home`, or
/// replaces the command of the caller's existing entry (see the module docs
/// for how it is recognised and for the writing rules).
///
/// A new entry goes in a new group appended to `hooks.SessionStart`. When
/// several entries match, the first is updated and the others are removed.
/// Returns [`HookChange::Unchanged`] without writing when the hook is already
/// there with the same command.
pub fn register_session_hook(
    home: &Path,
    key: &str,
    hook: &SessionHook,
) -> Result<HookChange, SessionHookError> {
    let row = supported_row(key)?;
    validate(hook)?;
    let path = home.join(row.user_settings);
    let mut root = read_settings(&path)?;
    let change = add_to(&mut root, row, hook, &path)?;
    if change != HookChange::Unchanged {
        write_settings(&path, &root)?;
    }
    Ok(change)
}

/// Removes the caller's entries with `id` from the agent's per-user settings
/// file under `home`, touching nothing else.
///
/// A group left empty by the removal is removed; `hooks.SessionStart` and then
/// `hooks` are removed only when the removal emptied them. A missing file or
/// no matching entry gives [`HookChange::Unchanged`] without writing.
pub fn unregister_session_hook(
    home: &Path,
    key: &str,
    id: &str,
) -> Result<HookChange, SessionHookError> {
    let row = supported_row(key)?;
    validate_id(id)?;
    let path = home.join(row.user_settings);
    if !exists(&path)? {
        return Ok(HookChange::Unchanged);
    }
    let mut root = read_settings(&path)?;
    let change = remove_from(&mut root, row, id, &path)?;
    if change != HookChange::Unchanged {
        write_settings(&path, &root)?;
    }
    Ok(change)
}

/// Whether the agent's per-user settings file under `home` holds an entry
/// with `id`. A missing file gives `false`; an invalid one is an error.
pub fn has_session_hook(home: &Path, key: &str, id: &str) -> Result<bool, SessionHookError> {
    let row = supported_row(key)?;
    validate_id(id)?;
    let path = home.join(row.user_settings);
    if !exists(&path)? {
        return Ok(false);
    }
    let root = read_settings(&path)?;
    contains(&root, row, id, &path)
}

/// The agent's machine-wide settings file an administrator manages on the
/// current platform, or `None` when the agent is unsupported (or the
/// platform is not Linux, macOS or Windows).
pub fn admin_hook_file(key: &str) -> Option<PathBuf> {
    let row = find_row(key)?;
    let path = if cfg!(target_os = "macos") {
        row.admin_macos
    } else if cfg!(windows) {
        row.admin_windows
    } else if cfg!(target_os = "linux") {
        row.admin_linux
    } else {
        return None;
    };
    Some(PathBuf::from(path))
}

/// The JSON an administrator places in [`admin_hook_file`] to install the
/// hook machine-wide: an object holding only `hooks`, e.g.
/// `{"hooks": {"SessionStart": [{"matcher": "", "hooks": [...]}]}}`, to be
/// merged into that file's existing content. Nothing is written.
pub fn admin_hook_entry(key: &str, hook: &SessionHook) -> Result<Value, SessionHookError> {
    let row = supported_row(key)?;
    validate(hook)?;
    let mut hooks = Obj::default();
    hooks.insert("SessionStart", Json::Array(vec![new_group(row, hook)]));
    let mut root = Obj::default();
    root.insert("hooks", Json::Object(hooks));
    Ok(serde_json::to_value(Json::Object(root)).expect("plain JSON always converts"))
}

/// Whether the agent's [`admin_hook_file`] holds an entry with `id`. A
/// missing, unreadable or unexpected file gives `false`.
pub fn admin_hook_present(key: &str, id: &str) -> bool {
    admin_hook_file(key).is_some_and(|path| admin_hook_present_in(&path, key, id))
}

fn admin_hook_present_in(path: &Path, key: &str, id: &str) -> bool {
    let Some(row) = find_row(key) else {
        return false;
    };
    validate_id(id).is_ok()
        && exists(path).unwrap_or(false)
        && read_settings(path)
            .and_then(|root| contains(&root, row, id, path))
            .unwrap_or(false)
}

/// The exact text a session-start hook of agent `key` prints on standard
/// output to show `text` to the user as a notice: one line of JSON,
/// `{"systemMessage":"<text>"}`, without a trailing newline. `None` when the
/// agent is unsupported.
pub fn format_notice(key: &str, text: &str) -> Option<String> {
    find_row(key)?;
    Some(json!({ "systemMessage": text }).to_string())
}

fn is_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

fn validate_id(id: &str) -> Result<(), SessionHookError> {
    if id.is_empty() || !id.chars().all(is_id_char) {
        return Err(SessionHookError::InvalidId(id.to_string()));
    }
    Ok(())
}

fn validate(hook: &SessionHook) -> Result<(), SessionHookError> {
    validate_id(&hook.id)?;
    if !has_word(&hook.command, &hook.id) {
        return Err(SessionHookError::CommandMissingId {
            id: hook.id.clone(),
            command: hook.command.clone(),
        });
    }
    Ok(())
}

/// Whether `id` occurs in `text` not directly preceded or followed by an id
/// character.
fn has_word(text: &str, id: &str) -> bool {
    text.match_indices(id).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + id.len()..].chars().next();
        !before.is_some_and(is_id_char) && !after.is_some_and(is_id_char)
    })
}

fn new_entry(row: &HookRow, hook: &SessionHook) -> Json {
    let mut entry = Obj::default();
    entry.insert("type", Json::String("command".into()));
    entry.insert("command", Json::String(hook.command.clone()));
    if row.marker == Marker::NameField {
        entry.insert("name", Json::String(hook.id.clone()));
    }
    Json::Object(entry)
}

fn new_group(row: &HookRow, hook: &SessionHook) -> Json {
    let mut group = Obj::default();
    group.insert("matcher", Json::String(row.matcher.into()));
    group.insert("hooks", Json::Array(vec![new_entry(row, hook)]));
    Json::Object(group)
}

fn is_ours(entry: &Json, row: &HookRow, id: &str) -> bool {
    match row.marker {
        Marker::NameField => entry.get("name").and_then(Json::as_str) == Some(id),
        Marker::CommandWord => entry
            .get("command")
            .and_then(Json::as_str)
            .is_some_and(|c| has_word(c, id)),
    }
}

fn unexpected(path: &Path, at: impl Into<String>, expected: &'static str) -> SessionHookError {
    SessionHookError::UnexpectedType {
        path: path.to_path_buf(),
        at: at.into(),
        expected,
    }
}

/// Checks the shape of `root` and returns `hooks.SessionStart` when present.
fn session_start<'a>(
    root: &'a mut Json,
    path: &Path,
) -> Result<Option<&'a mut Vec<Json>>, SessionHookError> {
    let obj = root
        .as_object_mut()
        .ok_or_else(|| unexpected(path, "the top level", "an object"))?;
    let Some(hooks) = obj.get_mut("hooks") else {
        return Ok(None);
    };
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| unexpected(path, "hooks", "an object"))?;
    let Some(start) = hooks.get_mut("SessionStart") else {
        return Ok(None);
    };
    let start = start
        .as_array_mut()
        .ok_or_else(|| unexpected(path, "hooks.SessionStart", "an array"))?;
    for (i, group) in start.iter().enumerate() {
        let group = group
            .as_object()
            .ok_or_else(|| unexpected(path, format!("hooks.SessionStart[{i}]"), "an object"))?;
        if group.get("hooks").is_some_and(|h| !h.is_array()) {
            return Err(unexpected(
                path,
                format!("hooks.SessionStart[{i}].hooks"),
                "an array",
            ));
        }
    }
    Ok(Some(start))
}

fn group_entries(group: &mut Json) -> Option<&mut Vec<Json>> {
    group.get_mut("hooks").and_then(Json::as_array_mut)
}

fn add_to(
    root: &mut Json,
    row: &HookRow,
    hook: &SessionHook,
    path: &Path,
) -> Result<HookChange, SessionHookError> {
    if session_start(root, path)?.is_none() {
        let obj = root.as_object_mut().expect("checked by session_start");
        let hooks = obj.get_or_insert_with("hooks", || Json::Object(Obj::default()));
        hooks
            .as_object_mut()
            .expect("checked by session_start")
            .insert("SessionStart", Json::Array(Vec::new()));
    }
    let start = session_start(root, path)?.expect("just inserted");

    let mut change = None;
    start.retain_mut(|group| {
        let Some(entries) = group_entries(group) else {
            return true;
        };
        let before = entries.len();
        let mut i = 0;
        while i < entries.len() {
            if !is_ours(&entries[i], row, &hook.id) {
                i += 1;
            } else if change.is_some() {
                // A duplicate of the entry already handled.
                entries.remove(i);
                change = Some(HookChange::Updated);
            } else {
                let entry = entries[i].as_object_mut().expect("is_ours needs an object");
                if entry.get("command").and_then(Json::as_str) == Some(hook.command.as_str()) {
                    change = Some(HookChange::Unchanged);
                } else {
                    entry.insert("command", Json::String(hook.command.clone()));
                    change = Some(HookChange::Updated);
                }
                i += 1;
            }
        }
        // Drop a group only when removing duplicates emptied it.
        entries.len() == before || !entries.is_empty()
    });
    if let Some(change) = change {
        return Ok(change);
    }
    start.push(new_group(row, hook));
    Ok(HookChange::Added)
}

fn remove_from(
    root: &mut Json,
    row: &HookRow,
    id: &str,
    path: &Path,
) -> Result<HookChange, SessionHookError> {
    let Some(start) = session_start(root, path)? else {
        return Ok(HookChange::Unchanged);
    };
    let mut removed = false;
    start.retain_mut(|group| {
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
    if !removed {
        return Ok(HookChange::Unchanged);
    }
    if start.is_empty() {
        let obj = root.as_object_mut().expect("checked by session_start");
        let hooks = obj
            .get_mut("hooks")
            .and_then(Json::as_object_mut)
            .expect("checked by session_start");
        hooks.shift_remove("SessionStart");
        if hooks.is_empty() {
            obj.shift_remove("hooks");
        }
    }
    Ok(HookChange::Removed)
}

fn contains(root: &Json, row: &HookRow, id: &str, path: &Path) -> Result<bool, SessionHookError> {
    let mut root = root.clone();
    let Some(start) = session_start(&mut root, path)? else {
        return Ok(false);
    };
    Ok(start.iter().any(|g| {
        g.get("hooks")
            .and_then(Json::as_array)
            .is_some_and(|es| es.iter().any(|e| is_ours(e, row, id)))
    }))
}

/// Whether anything (including a dangling link) is at `path`.
fn exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Reads the settings at `path`: an empty object when the file is missing or
/// blank.
fn read_settings(path: &Path) -> Result<Json, SessionHookError> {
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

/// Writes `root` to `path` atomically, through a symbolic link to its target.
fn write_settings(path: &Path, root: &Json) -> Result<(), SessionHookError> {
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

    let mut text = serde_json::to_string_pretty(root).map_err(io::Error::other)?;
    text.push('\n');

    let mut tmp = tempfile::Builder::new()
        .prefix(".settings.json.tmp-")
        .tempfile_in(&dir)?;
    tmp.write_all(text.as_bytes())?;
    tmp.as_file().sync_all()?;
    if let Ok(meta) = fs::metadata(&target) {
        fs::set_permissions(tmp.path(), meta.permissions())?;
    }
    tmp.persist(&target).map_err(|e| e.error)?;
    Ok(())
}

/// A JSON value that keeps object keys in the order they were read, so a
/// settings file is rewritten with its keys where the user put them.
mod ordered {
    use std::fmt;

    use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
    use serde::ser::{Serialize, SerializeMap, Serializer};

    #[derive(Debug, Clone, PartialEq)]
    pub(super) enum Json {
        Null,
        Bool(bool),
        Number(serde_json::Number),
        String(String),
        Array(Vec<Json>),
        Object(Obj),
    }

    /// An object's members, in order.
    #[derive(Debug, Clone, Default, PartialEq)]
    pub(super) struct Obj(Vec<(String, Json)>);

    impl Obj {
        pub(super) fn get(&self, key: &str) -> Option<&Json> {
            self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        }

        pub(super) fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
            self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
        }

        /// Replaces the value of `key` in place, or appends it.
        pub(super) fn insert(&mut self, key: &str, value: Json) {
            match self.get_mut(key) {
                Some(v) => *v = value,
                None => self.0.push((key.to_string(), value)),
            }
        }

        pub(super) fn get_or_insert_with(
            &mut self,
            key: &str,
            f: impl FnOnce() -> Json,
        ) -> &mut Json {
            let i = match self.0.iter().position(|(k, _)| k == key) {
                Some(i) => i,
                None => {
                    self.0.push((key.to_string(), f()));
                    self.0.len() - 1
                }
            };
            &mut self.0[i].1
        }

        /// Removes `key`, keeping the other members in order.
        pub(super) fn shift_remove(&mut self, key: &str) {
            self.0.retain(|(k, _)| k != key);
        }

        pub(super) fn is_empty(&self) -> bool {
            self.0.is_empty()
        }
    }

    impl Json {
        pub(super) fn get(&self, key: &str) -> Option<&Json> {
            self.as_object().and_then(|o| o.get(key))
        }

        pub(super) fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
            self.as_object_mut().and_then(|o| o.get_mut(key))
        }

        pub(super) fn as_str(&self) -> Option<&str> {
            match self {
                Json::String(s) => Some(s),
                _ => None,
            }
        }

        pub(super) fn as_array(&self) -> Option<&Vec<Json>> {
            match self {
                Json::Array(a) => Some(a),
                _ => None,
            }
        }

        pub(super) fn as_array_mut(&mut self) -> Option<&mut Vec<Json>> {
            match self {
                Json::Array(a) => Some(a),
                _ => None,
            }
        }

        pub(super) fn as_object(&self) -> Option<&Obj> {
            match self {
                Json::Object(o) => Some(o),
                _ => None,
            }
        }

        pub(super) fn as_object_mut(&mut self) -> Option<&mut Obj> {
            match self {
                Json::Object(o) => Some(o),
                _ => None,
            }
        }

        pub(super) fn is_array(&self) -> bool {
            matches!(self, Json::Array(_))
        }

        pub(super) fn is_object(&self) -> bool {
            matches!(self, Json::Object(_))
        }
    }

    impl Serialize for Json {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            match self {
                Json::Null => s.serialize_unit(),
                Json::Bool(b) => s.serialize_bool(*b),
                Json::Number(n) => n.serialize(s),
                Json::String(v) => s.serialize_str(v),
                Json::Array(a) => a.serialize(s),
                Json::Object(o) => {
                    let mut map = s.serialize_map(Some(o.0.len()))?;
                    for (k, v) in &o.0 {
                        map.serialize_entry(k, v)?;
                    }
                    map.end()
                }
            }
        }
    }

    impl<'de> Deserialize<'de> for Json {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Json, D::Error> {
            d.deserialize_any(JsonVisitor)
        }
    }

    struct JsonVisitor;

    impl<'de> Visitor<'de> for JsonVisitor {
        type Value = Json;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("any JSON value")
        }

        fn visit_unit<E>(self) -> Result<Json, E> {
            Ok(Json::Null)
        }

        fn visit_bool<E>(self, v: bool) -> Result<Json, E> {
            Ok(Json::Bool(v))
        }

        fn visit_i64<E>(self, v: i64) -> Result<Json, E> {
            Ok(Json::Number(v.into()))
        }

        fn visit_u64<E>(self, v: u64) -> Result<Json, E> {
            Ok(Json::Number(v.into()))
        }

        fn visit_f64<E>(self, v: f64) -> Result<Json, E> {
            Ok(serde_json::Number::from_f64(v).map_or(Json::Null, Json::Number))
        }

        fn visit_str<E>(self, v: &str) -> Result<Json, E> {
            Ok(Json::String(v.to_string()))
        }

        fn visit_string<E>(self, v: String) -> Result<Json, E> {
            Ok(Json::String(v))
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
            let mut out = Vec::new();
            while let Some(v) = seq.next_element()? {
                out.push(v);
            }
            Ok(Json::Array(out))
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
            let mut out = Vec::new();
            while let Some((k, v)) = map.next_entry::<String, Json>()? {
                out.push((k, v));
            }
            Ok(Json::Object(Obj(out)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn hook(id: &str, command: &str) -> SessionHook {
        SessionHook {
            id: id.into(),
            command: command.into(),
        }
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
        assert_eq!(session_hook_agents(), vec!["claude", "gemini"]);
        let c = session_hook_support("claude").unwrap();
        assert_eq!(c.user_settings, PathBuf::from(".claude/settings.json"));
        let g = session_hook_support("gemini").unwrap();
        assert_eq!(g.user_settings, PathBuf::from(".gemini/settings.json"));
        assert!(session_hook_support("codex").is_none());

        let tmp = TempDir::new().unwrap();
        let h = hook("my-hook", "tool --id my-hook");
        assert!(matches!(
            register_session_hook(tmp.path(), "codex", &h),
            Err(SessionHookError::Unsupported(_))
        ));
        assert!(matches!(
            unregister_session_hook(tmp.path(), "nope", "my-hook"),
            Err(SessionHookError::Unsupported(_))
        ));
        assert!(has_session_hook(tmp.path(), "nope", "my-hook").is_err());
        assert!(admin_hook_file("codex").is_none());
        assert!(admin_hook_entry("codex", &h).is_err());
        assert!(format_notice("codex", "hi").is_none());
        assert!(!admin_hook_present("codex", "my-hook"));
    }

    #[test]
    fn approval_note_only_for_claude() {
        let note = session_hook_support("claude").unwrap().approval_note;
        assert!(note.unwrap().contains("/hooks"));
        assert!(session_hook_support("gemini")
            .unwrap()
            .approval_note
            .is_none());
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
        assert_eq!(
            admin_hook_entry("claude", &h).unwrap(),
            json!({"hooks": {"SessionStart": [
                {"matcher": "", "hooks": [{"type": "command", "command": "tool --id my-hook"}]}
            ]}})
        );
        assert_eq!(
            admin_hook_entry("gemini", &h).unwrap(),
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
        for key in ["claude", "gemini"] {
            let mut entry = admin_hook_entry(key, &h).unwrap();
            entry["other"] = json!(true);
            fs::write(&path, entry.to_string()).unwrap();
            assert!(admin_hook_present_in(&path, key, "my-hook"), "{key}");
            assert!(!admin_hook_present_in(&path, key, "other-hook"));
            assert!(!admin_hook_present_in(&path, "codex", "my-hook"));
        }
    }

    #[test]
    fn notice_format_per_agent() {
        for key in ["claude", "gemini"] {
            let out = format_notice(key, "Ready: 3 \"items\"\nsynced").unwrap();
            assert!(!out.contains('\n'), "one line");
            let v: Value = serde_json::from_str(&out).unwrap();
            assert_eq!(v, json!({"systemMessage": "Ready: 3 \"items\"\nsynced"}));
        }
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
}
