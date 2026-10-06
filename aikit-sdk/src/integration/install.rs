//! Owned hook configuration. A durable prepare/write/commit journal makes
//! interrupted configuration changes resumable without guessing or overwriting
//! external edits. This does not establish that a native agent ran the hooks.
use crate::config_json::{parse_object, JsonObjectError};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Debug)]
pub enum IntegrationError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    Json(serde_json::Error),
    UnknownAgent(String),
    Unsupported(String),
    Invalid(String),
    Conflict(String),
    NotFound,
    RecoveryRequired(String),
}
impl std::fmt::Display for IntegrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Sql(e) => e.fmt(f),
            Self::Json(e) => e.fmt(f),
            Self::UnknownAgent(key) => write!(f, "unknown agent: {key}"),
            Self::Unsupported(detail) => write!(f, "unsupported integration: {detail}"),
            Self::Invalid(detail) => write!(f, "invalid integration: {detail}"),
            Self::Conflict(detail) => write!(f, "configuration conflict: {detail}"),
            Self::NotFound => write!(f, "integration record not found"),
            Self::RecoveryRequired(id) => write!(
                f,
                "resume prepared installation plan {id} before making another change"
            ),
        }
    }
}
impl std::error::Error for IntegrationError {}
impl From<std::io::Error> for IntegrationError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<rusqlite::Error> for IntegrationError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
impl From<serde_json::Error> for IntegrationError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    SessionStarted,
    InputSubmitted,
    BeforeTool,
    AfterTool,
    ToolFailed,
    CompletionProposed,
    SessionEnded,
}
impl HookEvent {
    pub(super) fn claude_name(self) -> &'static str {
        match self {
            Self::SessionStarted => "SessionStart",
            Self::InputSubmitted => "UserPromptSubmit",
            Self::BeforeTool => "PreToolUse",
            Self::AfterTool => "PostToolUse",
            Self::ToolFailed => "PostToolUseFailure",
            Self::CompletionProposed => "Stop",
            Self::SessionEnded => "SessionEnd",
        }
    }
}

/// An executable and exact argument vector. No shell serialization. The current
/// Claude exec-form adapter requires a native executable on Windows, not a shim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookCommand {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallSpec {
    pub application_id: String,
    /// Existing AIKit catalog key, e.g. `claude`. No parallel provider registry.
    pub agent_key: String,
    pub workspace: PathBuf,
    /// Use credential references in arguments; do not place credential values here.
    pub handler: HookCommand,
    pub events: Vec<HookEvent>,
    pub timeout_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installation {
    pub id: String,
    pub spec: InstallSpec,
    pub config_path: PathBuf,
}

/// Safe preview: deliberately excludes unrelated configuration contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPlan {
    pub id: String,
    pub installation_id: String,
    pub config_path: PathBuf,
    pub events: Vec<HookEvent>,
    pub removal: bool,
    pub before_fingerprint: String,
    pub after_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InstallationStatus {
    Absent,
    /// Configuration exists as installed; execution/effective policy is unqualified.
    Configured {
        installation: Installation,
    },
    Drifted {
        installation: Installation,
        detail: String,
    },
    RecoveryRequired {
        plan_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OwnedHook {
    event: String,
    entry: Value,
    event_existed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Receipt {
    installation: Installation,
    hooks_existed: bool,
    owned: Vec<OwnedHook>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Pending {
    preview: InstallPlan,
    before: String,
    after: Vec<u8>,
    previous_receipt: Option<String>,
    next_receipt: Option<Receipt>,
    workspace: PathBuf,
}

/// Reusable integration entry point. Construction opens metadata only; it never
/// spawns an agent or server. Use a private state directory outside the worktree.
/// Journal rows may contain existing config values while an operation is pending.
pub struct IntegrationService {
    state: PathBuf,
}

impl IntegrationService {
    pub fn open(state: impl AsRef<Path>) -> Result<Self, IntegrationError> {
        if !state.as_ref().exists() {
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(state.as_ref())?;
        }
        reject_link(state.as_ref())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(state.as_ref())?.permissions().mode() & 0o077 != 0 {
                return Err(IntegrationError::Invalid(
                    "integration state directory must be private (mode 0700)".into(),
                ));
            }
        }
        let service = Self {
            state: fs::canonicalize(state)?,
        };
        let connection = service.connection()?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 2 {
            return Err(IntegrationError::Invalid("newer integration schema".into()));
        }
        if version < 2 {
            let _lock = service.state_lock()?;
            connection.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS installations(id TEXT PRIMARY KEY, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS install_plans(id TEXT PRIMARY KEY, installation_id TEXT NOT NULL, phase TEXT NOT NULL, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS hook_invocations(sequence INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE, installation_id TEXT NOT NULL, session_id TEXT NOT NULL, body TEXT NOT NULL, decision TEXT);
            CREATE INDEX IF NOT EXISTS hooks_by_installation ON hook_invocations(installation_id,sequence);
            PRAGMA user_version=2; COMMIT;")?;
        }
        Ok(service)
    }

    fn state_lock(&self) -> Result<fs::File, IntegrationError> {
        lock(&self.state.join("integration.lock"))
    }
    pub(super) fn connection(&self) -> Result<Connection, IntegrationError> {
        let path = self.state.join("integration.db");
        reject_link(&path)?;
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        Ok(connection)
    }

    /// Installation and update follow the same path. Existing owned entries must
    /// still match their receipt; unrelated settings and hooks are retained.
    pub fn plan_install(&self, mut spec: InstallSpec) -> Result<InstallPlan, IntegrationError> {
        validate_spec(&mut spec)?;
        if self.state.starts_with(&spec.workspace) {
            return Err(IntegrationError::Invalid(
                "integration state must be outside the reviewed workspace".into(),
            ));
        }
        let config_path = config_path(&spec)?;
        let id = installation_id(&spec);
        let _lock = self.state_lock()?;
        let connection = self.connection()?;
        reject_pending(&connection, &id)?;
        reject_prepared_config(&connection, &config_path, None)?;
        let previous = receipt_json(&connection, &id)?;
        let before = read_config(&spec.workspace, &config_path)?;
        let mut document = document(before.as_deref())?;
        if let Some(raw) = &previous {
            remove_owned(&mut document, &serde_json::from_str(raw)?)?;
        }
        if document.get("disableAllHooks").and_then(Value::as_bool) == Some(true) {
            return Err(IntegrationError::Conflict(
                "hooks are disabled in this configuration".into(),
            ));
        }
        let installation = Installation {
            id,
            spec,
            config_path,
        };
        let receipt = add_owned(&mut document, installation)?;
        self.save_plan(
            &connection,
            before,
            document,
            previous,
            Some(receipt.clone()),
            &receipt.installation,
        )
    }

    pub fn plan_remove(&self, id: &str) -> Result<InstallPlan, IntegrationError> {
        let _lock = self.state_lock()?;
        let connection = self.connection()?;
        reject_pending(&connection, id)?;
        let raw = receipt_json(&connection, id)?.ok_or(IntegrationError::NotFound)?;
        let receipt: Receipt = serde_json::from_str(&raw)?;
        reject_prepared_config(&connection, &receipt.installation.config_path, None)?;
        let before = read_config(
            &receipt.installation.spec.workspace,
            &receipt.installation.config_path,
        )?;
        let mut document = document(before.as_deref())?;
        remove_owned(&mut document, &receipt)?;
        self.save_plan(
            &connection,
            before,
            document,
            Some(raw),
            None,
            &receipt.installation,
        )
    }

    fn save_plan(
        &self,
        connection: &Connection,
        before: Option<Vec<u8>>,
        document: Value,
        previous_receipt: Option<String>,
        next_receipt: Option<Receipt>,
        installation: &Installation,
    ) -> Result<InstallPlan, IntegrationError> {
        let mut after = serde_json::to_vec_pretty(&document)?;
        after.push(b'\n');
        if after.len() as u64 > MAX_CONFIG_BYTES {
            return Err(IntegrationError::Invalid(
                "resulting config exceeds 1 MiB".into(),
            ));
        }
        let preview = InstallPlan {
            id: uuid::Uuid::new_v4().to_string(),
            installation_id: installation.id.clone(),
            config_path: installation.config_path.clone(),
            events: installation.spec.events.clone(),
            removal: next_receipt.is_none(),
            before_fingerprint: fingerprint(before.as_deref()),
            after_fingerprint: fingerprint(Some(&after)),
        };
        let pending = Pending {
            preview: preview.clone(),
            before: preview.before_fingerprint.clone(),
            after,
            previous_receipt,
            next_receipt,
            workspace: installation.spec.workspace.clone(),
        };
        connection.execute(
            "INSERT INTO install_plans VALUES (?1,?2,'planned',?3)",
            params![
                preview.id,
                preview.installation_id,
                serde_json::to_string(&pending)?
            ],
        )?;
        Ok(preview)
    }

    /// Apply an opaque persisted plan ID. A stale or edited preview cannot change
    /// the executable, target or desired bytes. Repeating this call is idempotent.
    /// Cooperating writers use the config lock; arbitrary external editors can
    /// still race a filesystem rename and must be coordinated by the owner.
    pub fn apply_install(&self, plan_id: &str) -> Result<InstallationStatus, IntegrationError> {
        self.apply_inner(plan_id, false)
    }

    fn apply_inner(
        &self,
        plan_id: &str,
        interrupt_after_write: bool,
    ) -> Result<InstallationStatus, IntegrationError> {
        let _lock = self.state_lock()?;
        let mut connection = self.connection()?;
        let (phase, body): (String, String) = connection
            .query_row(
                "SELECT phase,body FROM install_plans WHERE id=?1",
                [plan_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(IntegrationError::NotFound)?;
        let pending: Pending = serde_json::from_str(&body)?;
        if phase == "applied" {
            return self.status_inner(&connection, &pending.preview.installation_id);
        }
        let config = &pending.preview.config_path;
        ensure_config_parent(&pending.workspace, config)?;
        let _config_lock = lock(&config.with_extension("aikit.lock"))?;
        reject_prepared_config(&connection, config, Some(plan_id))?;
        if let Some(other) = prepared_plan(&connection, &pending.preview.installation_id)? {
            if other != plan_id {
                return Err(IntegrationError::RecoveryRequired(other));
            }
        }
        if receipt_json(&connection, &pending.preview.installation_id)? != pending.previous_receipt
        {
            return Err(IntegrationError::Conflict(
                "installation changed since planning".into(),
            ));
        }
        let current = read_config(&pending.workspace, config)?;
        let current_hash = fingerprint(current.as_deref());
        let already_written =
            phase == "prepared" && current_hash == pending.preview.after_fingerprint;
        if !already_written && current_hash != pending.before {
            return Err(IntegrationError::Conflict(
                "configuration changed since planning; no files were replaced".into(),
            ));
        }
        connection.execute(
            "UPDATE install_plans SET phase='prepared' WHERE id=?1",
            [plan_id],
        )?;
        if !already_written {
            replace_config(&pending.workspace, config, &pending.before, &pending.after)?;
        }
        // Fault injection exercises the actual recovery boundary; not public API.
        if interrupt_after_write {
            return Err(IntegrationError::RecoveryRequired(plan_id.into()));
        }
        let tx = connection.transaction()?;
        if let Some(receipt) = &pending.next_receipt {
            tx.execute("INSERT INTO installations VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body", params![receipt.installation.id, serde_json::to_string(receipt)?])?;
        } else {
            tx.execute(
                "DELETE FROM installations WHERE id=?1",
                [&pending.preview.installation_id],
            )?;
        }
        let mut completed = pending.clone();
        completed.after.clear();
        completed.previous_receipt = None;
        completed.next_receipt = None;
        tx.execute(
            "UPDATE install_plans SET phase='applied',body=?2 WHERE id=?1",
            params![plan_id, serde_json::to_string(&completed)?],
        )?;
        tx.commit()?;
        self.status_inner(&connection, &pending.preview.installation_id)
    }

    pub fn installation_status(&self, id: &str) -> Result<InstallationStatus, IntegrationError> {
        let _lock = self.state_lock()?;
        self.status_inner(&self.connection()?, id)
    }

    /// Resolve a configured installation without a circular ID-in-handler-args
    /// dependency. These are trusted process configuration values, never fields
    /// copied from a native hook payload.
    pub fn find_installation(
        &self,
        application_id: &str,
        agent_key: &str,
        workspace: &Path,
    ) -> Result<Installation, IntegrationError> {
        if crate::agent(agent_key).is_none() {
            return Err(IntegrationError::UnknownAgent(agent_key.into()));
        }
        let workspace = fs::canonicalize(workspace)?;
        self.installed(&installation_key(agent_key, application_id, &workspace))
    }

    pub(super) fn installed(&self, id: &str) -> Result<Installation, IntegrationError> {
        let connection = self.connection()?;
        reject_pending(&connection, id)?;
        let raw = receipt_json(&connection, id)?.ok_or(IntegrationError::NotFound)?;
        let receipt: Receipt = serde_json::from_str(&raw)?;
        Ok(receipt.installation)
    }

    fn status_inner(
        &self,
        connection: &Connection,
        id: &str,
    ) -> Result<InstallationStatus, IntegrationError> {
        if let Some(plan_id) = prepared_plan(connection, id)? {
            return Ok(InstallationStatus::RecoveryRequired { plan_id });
        }
        let Some(raw) = receipt_json(connection, id)? else {
            return Ok(InstallationStatus::Absent);
        };
        let receipt: Receipt = serde_json::from_str(&raw)?;
        let current = read_config(
            &receipt.installation.spec.workspace,
            &receipt.installation.config_path,
        )?;
        let check = document(current.as_deref()).and_then(|mut value| {
            if value.get("disableAllHooks").and_then(Value::as_bool) == Some(true) {
                return Err(IntegrationError::Conflict("hooks are disabled".into()));
            }
            remove_owned(&mut value, &receipt)
        });
        match check {
            Ok(()) => Ok(InstallationStatus::Configured {
                installation: receipt.installation,
            }),
            Err(error) => Ok(InstallationStatus::Drifted {
                installation: receipt.installation,
                detail: error.to_string(),
            }),
        }
    }
}

fn validate_spec(spec: &mut InstallSpec) -> Result<(), IntegrationError> {
    if crate::agent(&spec.agent_key).is_none() {
        return Err(IntegrationError::UnknownAgent(spec.agent_key.clone()));
    }
    if spec.agent_key != "claude" {
        return Err(IntegrationError::Unsupported(format!(
            "owned hooks for {} are not implemented",
            spec.agent_key
        )));
    }
    if spec.application_id.is_empty()
        || spec.application_id.len() > 128
        || !spec
            .application_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(IntegrationError::Invalid(
            "application ID must use 1-128 ASCII letters, digits, dot, underscore or hyphen".into(),
        ));
    }
    spec.workspace = fs::canonicalize(&spec.workspace)?;
    if !spec.workspace.is_dir() {
        return Err(IntegrationError::Invalid(
            "workspace must be a directory".into(),
        ));
    }
    if !spec.handler.executable.is_absolute() || !spec.handler.executable.is_file() {
        return Err(IntegrationError::Invalid(
            "handler must be an existing absolute executable path".into(),
        ));
    }
    if cfg!(windows)
        && !spec
            .handler
            .executable
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("exe"))
    {
        return Err(IntegrationError::Unsupported(
            "Windows exec-form hooks require a native .exe".into(),
        ));
    }
    if !spec
        .handler
        .executable
        .to_str()
        .is_some_and(|value| !value.contains("${"))
        || spec.handler.arguments.len() > 128
        || spec
            .handler
            .arguments
            .iter()
            .any(|arg| arg.len() > 8192 || arg.contains('\0') || arg.contains("${"))
    {
        return Err(IntegrationError::Invalid("use a UTF-8 executable and at most 128 bounded arguments without NUL or native path placeholders; pass resolved values".into()));
    }
    if spec.events.is_empty()
        || spec.events.iter().copied().collect::<BTreeSet<_>>().len() != spec.events.len()
        || !(1..=60).contains(&spec.timeout_seconds)
    {
        return Err(IntegrationError::Invalid(
            "select distinct hooks and a timeout of 1-60 seconds".into(),
        ));
    }
    spec.events.sort();
    Ok(())
}

fn config_path(spec: &InstallSpec) -> Result<PathBuf, IntegrationError> {
    match spec.agent_key.as_str() {
        "claude" => Ok(spec.workspace.join(".claude").join("settings.local.json")),
        _ => Err(IntegrationError::Unsupported(
            "hook configuration layout".into(),
        )),
    }
}
fn installation_id(spec: &InstallSpec) -> String {
    installation_key(&spec.agent_key, &spec.application_id, &spec.workspace)
}
fn installation_key(agent_key: &str, application_id: &str, workspace: &Path) -> String {
    let key = serde_json::to_vec(&(agent_key, application_id, workspace))
        .expect("paths and strings serialize");
    format!("{:x}", Sha256::digest(key))
}
fn receipt_json(connection: &Connection, id: &str) -> Result<Option<String>, IntegrationError> {
    Ok(connection
        .query_row("SELECT body FROM installations WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?)
}
fn prepared_plan(connection: &Connection, id: &str) -> Result<Option<String>, IntegrationError> {
    Ok(connection
        .query_row(
            "SELECT id FROM install_plans WHERE installation_id=?1 AND phase='prepared'",
            [id],
            |r| r.get(0),
        )
        .optional()?)
}
fn reject_pending(connection: &Connection, id: &str) -> Result<(), IntegrationError> {
    if let Some(plan) = prepared_plan(connection, id)? {
        Err(IntegrationError::RecoveryRequired(plan))
    } else {
        Ok(())
    }
}
fn reject_prepared_config(
    connection: &Connection,
    path: &Path,
    own_plan: Option<&str>,
) -> Result<(), IntegrationError> {
    let mut query =
        connection.prepare("SELECT id,body FROM install_plans WHERE phase='prepared'")?;
    let rows = query.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (id, body) = row?;
        let pending: Pending = serde_json::from_str(&body)?;
        if Some(id.as_str()) != own_plan && pending.preview.config_path == path {
            return Err(IntegrationError::RecoveryRequired(id));
        }
    }
    Ok(())
}
fn document(bytes: Option<&[u8]>) -> Result<Value, IntegrationError> {
    let Some(bytes) = bytes else {
        return Ok(json!({}));
    };
    parse_object(bytes).map_err(|e| match e {
        JsonObjectError::Json(e) => IntegrationError::Json(e),
        JsonObjectError::NotObject => {
            IntegrationError::Invalid("config must contain a JSON object".into())
        }
    })
}
fn add_owned(
    document: &mut Value,
    installation: Installation,
) -> Result<Receipt, IntegrationError> {
    let root = document.as_object_mut().expect("validated object");
    let hooks_existed = root.contains_key("hooks");
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| IntegrationError::Invalid("hooks must be an object".into()))?;
    let mut owned = Vec::new();
    for event in &installation.spec.events {
        let event = event.claude_name();
        let event_existed = hooks.contains_key(event);
        let entries = hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| IntegrationError::Invalid(format!("{event} hooks must be an array")))?;
        let entry = json!({"hooks":[{"type":"command","command":installation.spec.handler.executable,"args":installation.spec.handler.arguments,"timeout":installation.spec.timeout_seconds}]});
        if entries.iter().any(|existing| existing == &entry) {
            return Err(IntegrationError::Conflict(format!(
                "unowned identical {event} hook already exists"
            )));
        }
        entries.push(entry.clone());
        owned.push(OwnedHook {
            event: event.into(),
            entry,
            event_existed,
        });
    }
    Ok(Receipt {
        installation,
        hooks_existed,
        owned,
    })
}
fn remove_owned(document: &mut Value, receipt: &Receipt) -> Result<(), IntegrationError> {
    let hooks = document
        .get_mut("hooks")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| IntegrationError::Conflict("owned hooks are missing".into()))?;
    for owned in &receipt.owned {
        let entries = hooks
            .get_mut(&owned.event)
            .and_then(Value::as_array_mut)
            .ok_or_else(|| IntegrationError::Conflict(format!("{} hooks changed", owned.event)))?;
        let matches: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| (e == &owned.entry).then_some(i))
            .collect();
        if matches.len() != 1 {
            return Err(IntegrationError::Conflict(format!(
                "owned {} hook was edited, removed or duplicated",
                owned.event
            )));
        }
        entries.remove(matches[0]);
        if entries.is_empty() && !owned.event_existed {
            hooks.remove(&owned.event);
        }
    }
    if hooks.is_empty() && !receipt.hooks_existed {
        document.as_object_mut().expect("object").remove("hooks");
    }
    Ok(())
}
fn fingerprint(bytes: Option<&[u8]>) -> String {
    match bytes {
        Some(bytes) => format!("{:x}", Sha256::digest(bytes)),
        None => "absent".into(),
    }
}
fn reject_link(path: &Path) -> Result<(), IntegrationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(IntegrationError::Conflict(
            format!("symlink at {}", path.display()),
        )),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
fn check_config_path(workspace: &Path, path: &Path) -> Result<(), IntegrationError> {
    reject_link(workspace)?;
    if fs::canonicalize(workspace)? != workspace {
        return Err(IntegrationError::Conflict(
            "workspace identity changed".into(),
        ));
    }
    let relative = path
        .strip_prefix(workspace)
        .map_err(|_| IntegrationError::Invalid("config outside workspace".into()))?;
    let mut current = workspace.to_owned();
    for part in relative.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err(IntegrationError::Invalid(
                "invalid configuration path".into(),
            ));
        }
        current.push(part);
        reject_link(&current)?;
    }
    Ok(())
}
fn ensure_config_parent(workspace: &Path, path: &Path) -> Result<(), IntegrationError> {
    check_config_path(workspace, path)?;
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| IntegrationError::Invalid("missing config parent".into()))?,
    )?;
    check_config_path(workspace, path)
}
fn read_config(workspace: &Path, path: &Path) -> Result<Option<Vec<u8>>, IntegrationError> {
    check_config_path(workspace, path)?;
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(IntegrationError::Invalid("config exceeds 1 MiB".into()));
    }
    Ok(Some(bytes))
}
fn lock(path: &Path) -> Result<fs::File, IntegrationError> {
    reject_link(path)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    fs2::FileExt::try_lock_exclusive(&file).map_err(|_| {
        IntegrationError::Conflict("another integration writer holds the lock".into())
    })?;
    Ok(file)
}
fn replace_config(
    workspace: &Path,
    path: &Path,
    before: &str,
    bytes: &[u8],
) -> Result<(), IntegrationError> {
    let parent = path
        .parent()
        .ok_or_else(|| IntegrationError::Invalid("missing config parent".into()))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    if let Ok(metadata) = fs::metadata(path) {
        temp.as_file().set_permissions(metadata.permissions())?;
    }
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    if fingerprint(read_config(workspace, path)?.as_deref()) != before {
        return Err(IntegrationError::Conflict(
            "config changed before replacement".into(),
        ));
    }
    temp.persist(path)
        .map_err(|error| IntegrationError::Io(error.error))?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
