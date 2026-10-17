//! The TOML side of Codex: the `config.toml` that may already hold hooks, and
//! the administrator's `requirements.toml`. Nothing here writes a file.

use std::fs;
use std::io;
use std::path::Path;

use super::ordered::Json;
use super::{settings, HookRow, Layout, SessionHook, SessionHookError};

/// Refuses with [`SessionHookError::OtherHookFile`] when the TOML file at
/// `path` defines hooks: a `hooks` key holding anything but Codex's own trust
/// records under `hooks.state`, which `/hooks` writes there (checked on Codex
/// 0.163.0). A missing file defines none.
pub(super) fn refuse_other_hooks(path: &Path) -> Result<(), SessionHookError> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let table: toml::Table =
        toml::from_str(&text).map_err(|source| SessionHookError::InvalidToml {
            path: path.to_path_buf(),
            source,
        })?;
    let defines_hooks = match table.get("hooks") {
        None => false,
        Some(toml::Value::Table(hooks)) => hooks.keys().any(|k| k != "state"),
        Some(_) => true,
    };
    if defines_hooks {
        return Err(SessionHookError::OtherHookFile(path.to_path_buf()));
    }
    Ok(())
}

fn quoted(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// The `[[hooks.<event>]]` fragment holding only this hook, for a `Grouped`
/// row.
pub(super) fn fragment(row: &HookRow, hook: &SessionHook) -> String {
    let Layout::Grouped { matcher } = row.layout else {
        unreachable!("only grouped rows have a TOML administrator file");
    };
    let event = row.event;
    let mut out = format!(
        "[[hooks.{event}]]\nmatcher = {}\n\n[[hooks.{event}.hooks]]\ntype = \"command\"\ncommand = {}\n",
        quoted(matcher),
        quoted(&hook.command),
    );
    if let Some(timeout) = row.timeout {
        out.push_str(&format!("timeout = {timeout}\n"));
    }
    out
}

/// Whether the TOML file at `path` holds the caller's entry, in the row's
/// shape. A missing or unreadable file, or one that is not valid TOML, gives
/// `false`.
pub(super) fn contains(path: &Path, row: &HookRow, id: &str) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(table) = toml::from_str::<toml::Table>(&text) else {
        return false;
    };
    let Ok(root) = serde_json::to_value(&table).and_then(serde_json::from_value::<Json>) else {
        return false;
    };
    settings::contains(&root, row, id, path).unwrap_or(false)
}
