//! Session-start hooks: run a command whenever an agent session starts.
//!
//! For a tool that wants to run a command whenever an agent session starts,
//! this module records, per agent, whether the agent supports such a hook and
//! where it is configured, and offers:
//!
//! - [`register_session_hook`] / [`unregister_session_hook`] /
//!   [`has_session_hook`]: manage one hook in the agent's per-user settings,
//!   merging into whatever they already hold;
//! - [`admin_hook_file`] / [`admin_hook_entry`] / [`admin_hook_present`]: the
//!   machine-wide settings file an administrator manages, the fragment to
//!   place in it, and whether it is already there (this module never writes
//!   administrator files);
//! - [`format_notice`]: the exact text a hook prints on standard output to
//!   show the user a one-line notice.
//!
//! Only agents whose hook format has been verified are in the table
//! ([`session_hook_agents`]); every other key is unsupported. Agents hold a
//! hook in one of three ways:
//!
//! - **grouped** (`claude`, `gemini`, `codex`): a JSON settings file with the
//!   entry under `hooks.SessionStart[].hooks[]`, each group carrying a
//!   `matcher`;
//! - **flat** (`cursor`): a JSON settings file with the entry directly under
//!   `hooks.sessionStart[]` and `"version": 1` at the top level;
//! - **owned file** (`pi`): a TypeScript extension file named after the hook
//!   id, which this module renders whole from a fixed template and never
//!   merges (ADR 0025).
//!
//! # Identifying a hook
//!
//! A [`SessionHook`] has a stable `id` chosen by the caller. The `id` must be
//! made of ASCII letters, digits, `-` and `_`, and the hook's `command` must
//! contain it as a whole word (not directly preceded or followed by one of
//! those characters), for example `mytool on-session-start --id mytool-hook`.
//! A hook built with [`SessionHook::from_argv`] must have the `id` as one of
//! its arguments. An existing hook is recognised as the caller's:
//!
//! - for `gemini`, by its `name` field, which this module sets to the `id`;
//! - for `claude`, `codex` and `cursor`, whose hook entries carry no
//!   free-form name, by the `id` appearing as a whole word in the entry's
//!   `command`;
//! - for `pi`, by the file `<id>.ts` starting with the line
//!   `// aikit session hook <id>; generated, do not edit`.
//!
//! Choose a distinctive `id`: any entry matching it is treated as the
//! caller's and may be updated or removed.
//!
//! # Writing safety
//!
//! A JSON settings file is only rewritten when it parses as a JSON object
//! whose `hooks` (if present) is an object, whose event list (if present) is
//! an array of objects, and whose groups' `hooks` (if present) are arrays; for
//! `cursor`, a top-level `version` other than `1` is refused too. Anything else
//! is refused with an error and the file is left untouched. Every other key,
//! group and hook is kept, in its original order. The file is written to a
//! temporary file in the same folder and renamed into place, pretty-printed
//! with two-space indentation and a trailing newline; the folder is created
//! if missing. When the settings file is a symbolic link, the link is
//! resolved and its target is replaced atomically, so the link itself stays a
//! link; a dangling link is refused.
//!
//! For `codex`, registering is refused when `~/.codex/config.toml` already
//! defines hooks, because Codex warns when one layer keeps hooks in two files.
//! The trust records Codex itself keeps there under `hooks.state` don't
//! count. For `pi`, a file at the hook's path that does not start with the
//! hook's marker line is never overwritten or deleted.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::json;

mod ordered;
mod pi;
mod settings;
mod toml_file;

use settings::{exists, read_settings, write_settings};

/// What one agent supports for session-start hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHookSupport {
    /// The agent's catalog key (e.g. `claude`).
    pub key: &'static str,
    /// The per-user settings file holding the hook, relative to the home
    /// directory (e.g. `.claude/settings.json`). For an agent that keeps each
    /// hook in a file of its own (`pi`), the folder holding those files.
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
    /// The program and its arguments, when the hook was built with
    /// [`SessionHook::from_argv`]. Agents that run a program rather than a
    /// shell command (`pi`) need it.
    pub argv: Option<Vec<String>>,
}

impl SessionHook {
    /// A hook that runs the shell command `command`.
    pub fn new(id: impl Into<String>, command: impl Into<String>) -> Self {
        SessionHook {
            id: id.into(),
            command: command.into(),
            argv: None,
        }
    }

    /// A hook that runs `program` with `args`. `command` is derived from them,
    /// quoted for a POSIX shell (for `cmd.exe` on Windows). One of `args` must
    /// equal `id`.
    pub fn from_argv<I, S>(id: impl Into<String>, program: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut argv = vec![program.into()];
        argv.extend(args.into_iter().map(Into::into));
        SessionHook {
            id: id.into(),
            command: shell_command(&argv),
            argv: Some(argv),
        }
    }
}

/// `argv` as one command line for the platform's shell.
fn shell_command(argv: &[String]) -> String {
    let quote = if cfg!(windows) {
        quote_cmd
    } else {
        quote_posix
    };
    argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")
}

/// Quotes `arg` for a POSIX shell, leaving it bare when it needs no quoting.
fn quote_posix(arg: &str) -> String {
    let bare = |c: char| c.is_ascii_alphanumeric() || "_-./:@%+,".contains(c);
    if !arg.is_empty() && arg.chars().all(bare) {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', r"'\''"))
}

/// Quotes `arg` for `cmd.exe`, leaving it bare when it needs no quoting.
fn quote_cmd(arg: &str) -> String {
    let special = |c: char| c.is_whitespace() || "\"&|<>^()%!,;=".contains(c);
    if !arg.is_empty() && !arg.chars().any(special) {
        return arg.to_string();
    }
    format!("\"{}\"", arg.replace('"', "\"\""))
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

/// The format of an [`AdminHookEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminHookFormat {
    /// A JSON object to merge into the administrator file.
    Json,
    /// TOML tables to add to the administrator file.
    Toml,
}

/// What an administrator places in [`admin_hook_file`] to install a hook
/// machine-wide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminHookEntry {
    /// The format of `text`, which is the administrator file's format.
    pub format: AdminHookFormat,
    /// The fragment, ending with a newline.
    pub text: String,
}

/// Error from the session-hook functions.
#[derive(Debug)]
pub enum SessionHookError {
    /// The agent has no verified session-start hook support, or (for
    /// [`admin_hook_entry`]) no administrator-level location.
    Unsupported(String),
    /// The id is empty or contains characters other than ASCII letters,
    /// digits, `-` and `_`.
    InvalidId(String),
    /// The command does not contain the id as a whole word, or a hook built
    /// from arguments has no argument equal to the id.
    CommandMissingId {
        /// The hook id.
        id: String,
        /// The command that lacks it.
        command: String,
    },
    /// The agent runs a program with arguments, and the hook (with this id)
    /// was not built with [`SessionHook::from_argv`].
    NeedsArgv(String),
    /// The settings file is not valid JSON.
    InvalidJson {
        /// The settings file.
        path: PathBuf,
        /// The parse error.
        source: serde_json::Error,
    },
    /// A TOML file that must be checked is not valid TOML.
    InvalidToml {
        /// The TOML file.
        path: PathBuf,
        /// The parse error.
        source: toml::de::Error,
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
    /// The agent already keeps hooks in this other file, so nothing was
    /// written.
    OtherHookFile(PathBuf),
    /// A file at the hook's path was not written by this module; it is left
    /// as it is.
    NotOurs(PathBuf),
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
            SessionHookError::NeedsArgv(id) => write!(
                f,
                "hook {id:?} needs a program and arguments for this agent, not only a command"
            ),
            SessionHookError::InvalidJson { path, source } => write!(
                f,
                "settings file is not valid JSON, refusing to modify {}: {source}",
                path.display()
            ),
            SessionHookError::InvalidToml { path, source } => write!(
                f,
                "{} is not valid TOML, refusing to add a hook: {source}",
                path.display()
            ),
            SessionHookError::UnexpectedType { path, at, expected } => write!(
                f,
                "unexpected value at {at} in {} (expected {expected}), refusing to modify it",
                path.display()
            ),
            SessionHookError::OtherHookFile(p) => write!(
                f,
                "hooks are already configured in {}, refusing to add a hook in a second file",
                p.display()
            ),
            SessionHookError::NotOurs(p) => write!(
                f,
                "{} was not written by aikit, refusing to replace it",
                p.display()
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
            SessionHookError::InvalidToml { source, .. } => Some(source),
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

/// How an agent's hook is recognised as the caller's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    /// The entry's `name` field equals the id; the entry carries `name`.
    NameField,
    /// The id appears as a whole word in the entry's `command`.
    CommandWord,
    /// The file named after the id starts with the marker line.
    FirstLine,
}

/// How an agent holds a hook (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// In a group with this `matcher` under `hooks.<event>`.
    Grouped { matcher: &'static str },
    /// Directly under `hooks.<event>`, with `"version": 1` at the top level.
    Flat,
    /// In a file of its own, in the folder `user_settings`.
    OwnedFile,
}

/// What a hook prints to show a notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Notice {
    /// `{"systemMessage": text}`, shown to the user.
    SystemMessage,
    /// `{"additional_context": text}`, which reaches the model only.
    AdditionalContext,
    /// The text itself, which the extension shows to the user.
    PlainText,
}

/// The administrator-level settings file of one agent.
struct AdminFiles {
    linux: &'static str,
    macos: &'static str,
    windows: &'static str,
    format: AdminHookFormat,
}

/// One row of the session-hook table.
struct HookRow {
    key: &'static str,
    user_settings: &'static str,
    layout: Layout,
    /// The key under `hooks` holding session-start hooks.
    event: &'static str,
    marker: Marker,
    /// The `timeout` in seconds set on a new entry, when the agent takes one.
    timeout: Option<u32>,
    approval_note: Option<&'static str>,
    admin: Option<AdminFiles>,
    notice: Notice,
    /// Another per-user file, relative to the home directory, where the
    /// agent may already keep hooks (TOML); registering is refused when it
    /// holds a `hooks` table.
    other_hook_file: Option<&'static str>,
}

/// Agents with verified session-start hook support, in catalog order.
const HOOK_ROWS: &[HookRow] = &[
    HookRow {
        key: "claude",
        user_settings: ".claude/settings.json",
        layout: Layout::Grouped { matcher: "" },
        event: "SessionStart",
        marker: Marker::CommandWord,
        timeout: None,
        approval_note: Some(
            "The agent may ask you to review the new hook in its hooks menu (/hooks) \
             before it takes effect.",
        ),
        admin: Some(AdminFiles {
            linux: "/etc/claude-code/managed-settings.json",
            macos: "/Library/Application Support/ClaudeCode/managed-settings.json",
            windows: r"C:\Program Files\ClaudeCode\managed-settings.json",
            format: AdminHookFormat::Json,
        }),
        notice: Notice::SystemMessage,
        other_hook_file: None,
    },
    HookRow {
        key: "gemini",
        user_settings: ".gemini/settings.json",
        layout: Layout::Grouped { matcher: "*" },
        event: "SessionStart",
        marker: Marker::NameField,
        timeout: None,
        approval_note: None,
        admin: Some(AdminFiles {
            linux: "/etc/gemini-cli/settings.json",
            macos: "/Library/Application Support/GeminiCli/settings.json",
            windows: r"C:\ProgramData\gemini-cli\settings.json",
            format: AdminHookFormat::Json,
        }),
        notice: Notice::SystemMessage,
        other_hook_file: None,
    },
    // Codex CLI, verified on 0.163.0. The hook runs on `startup` and
    // `resume`, not on `clear`, `compact` or `fork`, when the session's first
    // turn starts; reattaching to a session the app-server still holds is no
    // resume. A new or changed user hook runs only once trusted in `/hooks`,
    // which records the trust under `hooks.state` in `config.toml`. The
    // `systemMessage` shows in the transcript as a `Hook` line.
    HookRow {
        key: "codex",
        user_settings: ".codex/hooks.json",
        layout: Layout::Grouped {
            matcher: "startup|resume",
        },
        event: "SessionStart",
        marker: Marker::CommandWord,
        timeout: Some(10),
        approval_note: Some(
            "Codex runs the new hook only after you trust it in `/hooks`. \
             It asks again whenever the hook changes.",
        ),
        admin: Some(AdminFiles {
            linux: "/etc/codex/requirements.toml",
            macos: "/etc/codex/requirements.toml",
            windows: r"C:\ProgramData\OpenAI\Codex\requirements.toml",
            format: AdminHookFormat::Toml,
        }),
        notice: Notice::SystemMessage,
        other_hook_file: Some(".codex/config.toml"),
    },
    // Cursor. `sessionStart` runs on each new conversation and is not
    // awaited; the notice reaches the model, not the user.
    HookRow {
        key: "cursor",
        user_settings: ".cursor/hooks.json",
        layout: Layout::Flat,
        event: "sessionStart",
        marker: Marker::CommandWord,
        timeout: Some(10),
        approval_note: None,
        admin: Some(AdminFiles {
            linux: "/etc/cursor/hooks.json",
            macos: "/Library/Application Support/Cursor/hooks.json",
            windows: r"C:\ProgramData\Cursor\hooks.json",
            format: AdminHookFormat::Json,
        }),
        notice: Notice::AdditionalContext,
        other_hook_file: None,
    },
    // Pi, verified on 1.3.0. The extension runs the hook for the
    // `session_start` reasons `startup`, `new` and `resume`, not `reload` or
    // `fork`; the notice shows as a warning in the chat. Pi has no
    // administrator-level location. Installs that move the agent folder
    // with `PI_CODING_AGENT_DIR` are not covered.
    HookRow {
        key: "pi",
        user_settings: ".pi/agent/extensions",
        layout: Layout::OwnedFile,
        event: "session_start",
        marker: Marker::FirstLine,
        timeout: None,
        approval_note: None,
        admin: None,
        notice: Notice::PlainText,
        other_hook_file: None,
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

/// Adds the hook to the agent's per-user settings under `home`, or replaces
/// the command of the caller's existing entry (see the module docs for how it
/// is recognised and for the writing rules).
///
/// In a grouped layout, a new entry goes in a new group appended to the event
/// list; in a flat one, the entry itself is appended. When several entries
/// match, the first is updated and the others are removed. For `pi`, the
/// extension file is written whole, and the hook must be built with
/// [`SessionHook::from_argv`]. Returns [`HookChange::Unchanged`] without
/// writing when the hook is already there with the same command.
pub fn register_session_hook(
    home: &Path,
    key: &str,
    hook: &SessionHook,
) -> Result<HookChange, SessionHookError> {
    let row = supported_row(key)?;
    validate(hook)?;
    if let Some(other) = row.other_hook_file {
        toml_file::refuse_other_hooks(&home.join(other))?;
    }
    let path = home.join(row.user_settings);
    if row.layout == Layout::OwnedFile {
        return pi::register(&path, hook);
    }
    let mut root = read_settings(&path)?;
    let change = settings::add_to(&mut root, row, hook, &path)?;
    if change != HookChange::Unchanged {
        write_settings(&path, &root)?;
    }
    Ok(change)
}

/// Removes the caller's entries with `id` from the agent's per-user settings
/// under `home`, touching nothing else.
///
/// A group left empty by the removal is removed; the event list and then
/// `hooks` are removed only when the removal emptied them. For `pi`, the
/// extension file is deleted only when it starts with the marker line of
/// `id`. A missing file or no matching entry gives [`HookChange::Unchanged`]
/// without writing.
pub fn unregister_session_hook(
    home: &Path,
    key: &str,
    id: &str,
) -> Result<HookChange, SessionHookError> {
    let row = supported_row(key)?;
    validate_id(id)?;
    let path = home.join(row.user_settings);
    if row.layout == Layout::OwnedFile {
        return pi::unregister(&path, id);
    }
    if !exists(&path)? {
        return Ok(HookChange::Unchanged);
    }
    let mut root = read_settings(&path)?;
    let change = settings::remove_from(&mut root, row, id, &path)?;
    if change != HookChange::Unchanged {
        write_settings(&path, &root)?;
    }
    Ok(change)
}

/// Whether the agent's per-user settings under `home` hold an entry with
/// `id`. A missing file gives `false`; an invalid one is an error.
pub fn has_session_hook(home: &Path, key: &str, id: &str) -> Result<bool, SessionHookError> {
    let row = supported_row(key)?;
    validate_id(id)?;
    let path = home.join(row.user_settings);
    if row.layout == Layout::OwnedFile {
        return pi::has(&path, id);
    }
    if !exists(&path)? {
        return Ok(false);
    }
    let root = read_settings(&path)?;
    settings::contains(&root, row, id, &path)
}

/// The agent's machine-wide settings file an administrator manages on the
/// current platform, or `None` when the agent is unsupported, has no
/// administrator-level location (`pi`), or the platform is not Linux, macOS
/// or Windows.
pub fn admin_hook_file(key: &str) -> Option<PathBuf> {
    let admin = find_row(key)?.admin.as_ref()?;
    let path = if cfg!(target_os = "macos") {
        admin.macos
    } else if cfg!(windows) {
        admin.windows
    } else if cfg!(target_os = "linux") {
        admin.linux
    } else {
        return None;
    };
    Some(PathBuf::from(path))
}

/// What an administrator places in [`admin_hook_file`] to install the hook
/// machine-wide, to be merged into that file's existing content. Nothing is
/// written.
///
/// For a JSON file, an object holding only `hooks` (and, for `cursor`,
/// `version`), e.g.
/// `{"hooks": {"SessionStart": [{"matcher": "", "hooks": [...]}]}}`. For
/// Codex's `requirements.toml`, the `[[hooks.SessionStart]]` tables. An agent
/// without an administrator-level location (`pi`) gives
/// [`SessionHookError::Unsupported`].
pub fn admin_hook_entry(key: &str, hook: &SessionHook) -> Result<AdminHookEntry, SessionHookError> {
    let row = supported_row(key)?;
    validate(hook)?;
    let admin = row
        .admin
        .as_ref()
        .ok_or_else(|| SessionHookError::Unsupported(key.to_string()))?;
    let text = match admin.format {
        AdminHookFormat::Json => settings::to_text(&settings::fragment(row, hook)),
        AdminHookFormat::Toml => toml_file::fragment(row, hook),
    };
    Ok(AdminHookEntry {
        format: admin.format,
        text,
    })
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
    let Some(admin) = &row.admin else {
        return false;
    };
    if validate_id(id).is_err() || !exists(path).unwrap_or(false) {
        return false;
    }
    match admin.format {
        AdminHookFormat::Json => read_settings(path)
            .and_then(|root| settings::contains(&root, row, id, path))
            .unwrap_or(false),
        AdminHookFormat::Toml => toml_file::contains(path, row, id),
    }
}

/// The exact text a session-start hook of agent `key` prints on standard
/// output to show `text` as a notice, without a trailing newline, or `None`
/// when the agent is unsupported:
///
/// - `claude`, `gemini`, `codex`: one line of JSON,
///   `{"systemMessage":"<text>"}`, shown to the user;
/// - `cursor`: one line of JSON, `{"additional_context":"<text>"}`; Cursor
///   shows the user nothing and passes the text to the model;
/// - `pi`: `text` itself, which the extension shows to the user when Pi has
///   an interface.
pub fn format_notice(key: &str, text: &str) -> Option<String> {
    Some(match find_row(key)?.notice {
        Notice::SystemMessage => json!({ "systemMessage": text }).to_string(),
        Notice::AdditionalContext => json!({ "additional_context": text }).to_string(),
        Notice::PlainText => text.to_string(),
    })
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
    let argv_ok = match &hook.argv {
        Some(argv) => argv.iter().skip(1).any(|a| *a == hook.id),
        None => true,
    };
    if !argv_ok || !has_word(&hook.command, &hook.id) {
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

#[cfg(test)]
mod tests;
