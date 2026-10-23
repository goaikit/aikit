//! Owned hook configuration. A durable prepare/write/commit journal makes
//! interrupted configuration changes resumable without guessing or overwriting
//! external edits. This does not establish that a native agent ran the hooks.
use crate::config_json::{parse_object, JsonObjectError};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

// Called only inside a transaction. The initializer and explicit owner upgrade
// use the same additive migration; retained payloads and evidence are untouched.
fn migrate_message_tables(connection: &Connection) -> Result<(), IntegrationError> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS outgoing_messages(installation_id TEXT NOT NULL, operation_id TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(installation_id,operation_id));
        CREATE TABLE IF NOT EXISTS message_observations(cursor INTEGER PRIMARY KEY AUTOINCREMENT, installation_id TEXT NOT NULL, operation_id TEXT NOT NULL, event_id TEXT NOT NULL, body TEXT NOT NULL, UNIQUE(installation_id,event_id));
        CREATE INDEX IF NOT EXISTS message_observations_by_operation ON message_observations(installation_id,operation_id,cursor);
        CREATE TABLE IF NOT EXISTS message_handoffs(installation_id TEXT NOT NULL, operation_id TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(installation_id,operation_id));
        CREATE TABLE IF NOT EXISTS message_append_checkpoints(installation_id TEXT NOT NULL, operation_id TEXT NOT NULL, native_row_id TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(installation_id,operation_id), UNIQUE(installation_id,native_row_id));
        CREATE TABLE IF NOT EXISTS binding_admission_floors(installation_id TEXT PRIMARY KEY NOT NULL, cursor INTEGER NOT NULL);
        INSERT INTO binding_admission_floors SELECT DISTINCT json_extract(reference,'$.installation_id'), (SELECT coalesce(max(sequence),0) FROM hook_invocations) FROM session_bindings WHERE detached=1 ON CONFLICT(installation_id) DO NOTHING;
        PRAGMA user_version=15;")?;
    connection.prepare("SELECT installation_id,operation_id,native_row_id,body FROM message_append_checkpoints LIMIT 0")?;
    Ok(())
}

#[path = "install_completion.rs"]
mod completion;
pub use completion::{CompletionBlockingPolicy, SettingChange};
#[path = "install_agent_tools.rs"]
mod agent_tools_install;
use agent_tools_install::{
    check_agent_tool, owned_agent_tool, prune_removed_tool_directories, tool_file_changes,
};

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
    StaleSession,
    Detached,
    SessionEnded,
    RequirementsUnmet(super::CapabilityRequirementsError),
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
            Self::StaleSession => {
                write!(f, "session observation or installation has been replaced")
            }
            Self::Detached => write!(f, "application binding is detached"),
            Self::SessionEnded => write!(f, "native session end was observed"),
            Self::RequirementsUnmet(error) => error.fmt(f),
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
    /// Native turn failed. Notification only; never an accepted completion.
    CompletionFailed,
    /// Owned module observation, not a settings hook or a policy decision.
    NativeTurnStarted,
    /// Native outcome evidence; never inferred from Stop Allow or process exit.
    NativeTurnFinished,
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
            Self::CompletionFailed => "StopFailure",
            Self::NativeTurnStarted => "AIKitNativeTurnStarted",
            Self::NativeTurnFinished => "AIKitNativeTurnFinished",
            Self::SessionEnded => "SessionEnd",
        }
    }
}

/// An executable and exact argument vector. Adapters serialize their native
/// command format; Windows requires a native executable, not a command shim.
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
    #[serde(default)]
    pub completion_policy: CompletionBlockingPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentToolFileChange {
    pub path: PathBuf,
    pub before_fingerprint: String,
    pub after_fingerprint: String,
}

/// Safe preview: deliberately excludes unrelated configuration contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlan {
    pub id: String,
    pub installation_id: String,
    pub config_path: PathBuf,
    pub events: Vec<HookEvent>,
    pub removal: bool,
    pub before_fingerprint: String,
    pub after_fingerprint: String,
    /// Exact related setting changes only; no unrelated configuration is exposed.
    #[serde(default)]
    pub settings: Vec<SettingChange>,
    #[serde(default)]
    pub completion_policy: CompletionBlockingPolicy,
    /// Owned plugin files only, with no instructions or handler arguments.
    #[serde(default)]
    pub files: Vec<AgentToolFileChange>,
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
    #[serde(default)]
    inserted_version: bool,
    /// Complete ownership of a generated extension file, not merged JSON.
    #[serde(default)]
    source_fingerprint: Option<String>,
    #[serde(default)]
    completion: Option<completion::OwnedCompletion>,
    #[serde(default)]
    agent_tool: Option<OwnedAgentTool>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OwnedAgentTool {
    relative_root: PathBuf,
    files: BTreeMap<String, String>,
    #[serde(default)]
    invocation_env_key: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PendingToolFile {
    path: PathBuf,
    before: String,
    after: Option<Vec<u8>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Pending {
    preview: InstallPlan,
    before: String,
    after: Vec<u8>,
    previous_receipt: Option<String>,
    next_receipt: Option<Receipt>,
    workspace: PathBuf,
    #[serde(default)]
    delete: bool,
    #[serde(default)]
    tool_files: Vec<PendingToolFile>,
    /// New coordinated replacements must stay detached through apply/recovery.
    #[serde(default)]
    require_detached: bool,
    #[serde(default)]
    update_request_sha256: Option<String>,
}

/// Reusable integration entry point. Construction opens metadata only; it never
/// spawns an agent or server. Use a private state directory outside the worktree.
/// Journal rows may contain existing config values while an operation is pending.
pub struct IntegrationService {
    state: PathBuf,
    allow_create: bool,
}

/// Hold across a short consumer registration commit only. This pins the exact
/// applied plan revision against cooperating installation writers. Do not await,
/// invoke native I/O or call SDK writers/status while holding it.
#[must_use = "hold through the consumer commit"]
pub struct AppliedInstallationGuard {
    installation: Installation,
    _lock: InstallationLock,
}

impl AppliedInstallationGuard {
    pub fn installation(&self) -> &Installation {
        &self.installation
    }
}

/// Local metadata migration only. This neither updates installed hooks nor
/// qualifies the provider, its effective configuration or any native session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateUpgradeReport {
    pub previous_schema: u32,
    pub current_schema: u32,
    pub changed: bool,
}

fn validate_upgrade_schema(tx: &Connection) -> Result<u32, IntegrationError> {
    let previous_schema: u32 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if !(9..=15).contains(&previous_schema) {
        return Err(IntegrationError::Invalid(
            "explicit integration upgrade supports existing schemas 9 through 15 only".into(),
        ));
    }
    // Do not turn a claimed legacy version with missing core history into
    // a fresh journal. Preparing these reads verifies the expected columns
    // without parsing, modifying or dropping retained application payloads.
    for query in [
            "SELECT id,body FROM installations LIMIT 0",
            "SELECT id,installation_id,phase,body FROM install_plans LIMIT 0",
            "SELECT sequence,id,installation_id,session_id,body,decision,installation_revision FROM hook_invocations LIMIT 0",
            "SELECT installation_id,revision FROM installation_revisions LIMIT 0",
            "SELECT id,reference,detached FROM session_bindings LIMIT 0",
        ] {
            tx.prepare(query)?;
        }
    if previous_schema >= 10 {
        tx.prepare("SELECT installation_id,operation_id,body FROM outgoing_messages LIMIT 0")?;
        tx.prepare("SELECT cursor,installation_id,operation_id,event_id,body FROM message_observations LIMIT 0")?;
    }
    if previous_schema >= 11 {
        tx.prepare("SELECT installation_id,operation_id,body FROM message_handoffs LIMIT 0")?;
    }
    if previous_schema >= 12 {
        tx.prepare("SELECT installation_id,operation_id,native_row_id,body FROM message_append_checkpoints LIMIT 0")?;
    }
    if previous_schema == 15 {
        tx.prepare("SELECT installation_id,cursor FROM binding_admission_floors LIMIT 0")?;
    }
    Ok(previous_schema)
}

impl IntegrationService {
    pub fn open(state: impl AsRef<Path>) -> Result<Self, IntegrationError> {
        Self::open_inner(state.as_ref(), true)
    }

    /// Open an existing integration journal without replacing missing state.
    /// Requires the current schema; owner-controlled `open` may initialize or
    /// upgrade first. Every later connection also disables SQLite file creation.
    pub fn open_existing(state: impl AsRef<Path>) -> Result<Self, IntegrationError> {
        Self::open_inner(state.as_ref(), false)
    }

    fn open_inner(state: &Path, allow_create: bool) -> Result<Self, IntegrationError> {
        let service = Self::open_state(state, allow_create)?;
        let connection = service.connection()?;
        service.initialize_schema(&connection, allow_create)?;
        Ok(service)
    }

    fn open_state(state: &Path, allow_create: bool) -> Result<Self, IntegrationError> {
        if !state.exists() {
            if !allow_create {
                return Err(IntegrationError::NotFound);
            }
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(state)?;
        }
        reject_link(state)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(state)?.permissions().mode() & 0o077 != 0 {
                return Err(IntegrationError::Invalid(
                    "integration state directory must be private (mode 0700)".into(),
                ));
            }
        }
        if !allow_create {
            let database = state.join("integration.db");
            reject_link(&database)?;
            let metadata = match fs::metadata(&database) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Err(IntegrationError::NotFound);
                }
                other => other?,
            };
            if !metadata.is_file() || metadata.len() == 0 {
                return Err(IntegrationError::Invalid(
                    "existing integration journal is empty or not a file".into(),
                ));
            }
        }
        Ok(Self {
            state: fs::canonicalize(state)?,
            allow_create,
        })
    }

    /// Explicitly upgrade an existing schema 9-14 journal to the current schema.
    /// Never creates missing state, repairs malformed tables, changes native
    /// configuration, detaches a binding or submits a message. Existing attached
    /// bindings and unapplied installation plans must first be resolved by their
    /// owner. Use upgrade_existing_detaching for exact scoped legacy consent.
    /// The installation lock and one SQLite
    /// transaction cover preflight and the additive migration together.
    ///
    /// Current-schema calls are idempotent and do not need quiescence because
    /// they do not migrate anything. Older schemas need a separate legacy
    /// upgrade procedure; `open` retains its existing initializer semantics.
    pub fn upgrade_existing(
        state: impl AsRef<Path>,
    ) -> Result<StateUpgradeReport, IntegrationError> {
        Self::upgrade_state_inner(state.as_ref(), None)
    }

    fn upgrade_state_inner(
        state: &Path,
        selected: Option<(&str, &[String])>,
    ) -> Result<StateUpgradeReport, IntegrationError> {
        let service = Self::open_state(state, false)?;
        let _guard = service.state_lock()?;
        let mut connection = service.connection()?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let previous_schema = validate_upgrade_schema(&tx)?;
        if let Some((installation, ids)) = selected {
            for id in ids {
                let (body, detached): (String, bool) = tx
                    .query_row(
                        "SELECT reference,detached FROM session_bindings WHERE id=?1",
                        [id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?
                    .ok_or(IntegrationError::NotFound)?;
                let reference: super::SessionRef = serde_json::from_str(&body)?;
                if reference.installation_id != installation {
                    return Err(IntegrationError::Conflict(
                        "selected binding belongs to another installation".into(),
                    ));
                }
                if previous_schema == 15 && !detached {
                    return Err(IntegrationError::Conflict("integration schema is current; use ordinary owner detachment for attached handles".into()));
                }
                if previous_schema != 15 {
                    tx.execute("UPDATE session_bindings SET detached=1 WHERE id=?1", [id])?;
                }
            }
        }
        if previous_schema != 15 {
            let attached: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM session_bindings WHERE detached=0)",
                [],
                |row| row.get(0),
            )?;
            if attached {
                return Err(IntegrationError::Conflict(
                    "detach recorded session bindings with the compatible runtime before upgrading integration state; no agent was stopped".into(),
                ));
            }
            let pending: Option<String> = tx
                .query_row(
                    "SELECT id FROM install_plans WHERE phase != 'applied' ORDER BY id LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(plan) = pending {
                return Err(IntegrationError::RecoveryRequired(plan));
            }
            migrate_message_tables(&tx)?;
        }
        tx.commit()?;
        Ok(StateUpgradeReport {
            previous_schema,
            current_schema: 15,
            changed: previous_schema != 15,
        })
    }

    /// Read retained schemas 9-15 without initialization or migration. The
    /// bounded page exposes recorded correlation only, not active configuration
    /// or native liveness. Use it to select exact IDs for upgrade_existing_detaching.
    pub fn attached_bindings_for_upgrade(
        state: impl AsRef<Path>,
        application_id: &str,
        agent_key: &str,
        workspace: &Path,
        after_id: Option<&str>,
        limit: u32,
    ) -> Result<super::RecordedBindingsPage, IntegrationError> {
        if limit == 0
            || limit > 500
            || after_id.is_some_and(|id| id.len() > 256 || id.contains('\0'))
        {
            return Err(IntegrationError::Invalid(
                "binding page requires a valid cursor and limit 1-500".into(),
            ));
        }
        let installation = super::owner_sessions::scope(application_id, agent_key, workspace)?;
        let service = Self::open_state(state.as_ref(), false)?;
        let _guard = service.state_lock()?;
        let mut connection = Connection::open_with_flags(
            service.state.join("integration.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)?;
        validate_upgrade_schema(&tx)?;
        let mut query = tx.prepare("SELECT id,reference FROM session_bindings WHERE detached=0 AND json_extract(reference,'$.installation_id')=?1 AND id>?2 ORDER BY id LIMIT ?3")?;
        let mut records = Vec::new();
        for row in query.query_map(params![installation, after_id.unwrap_or(""), limit], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (id, body) = row?;
            let reference = serde_json::from_str(&body)?;
            let status = super::sessions::reference_status(&tx, &reference)?;
            records.push(super::RecordedBinding {
                id,
                reference,
                status,
            });
        }
        Ok(super::RecordedBindingsPage {
            next_id: records.last().map(|r| r.id.clone()),
            records,
        })
    }

    /// Detach exact owner-selected scoped handles and migrate in one existing
    /// SQLite transaction. Other attached handles or pending plans refuse the
    /// entire operation. Rollback retains old schema and every detachment bit.
    /// No hooks, native process or messages change. Close old runtimes first.
    /// Current-schema replay succeeds only for IDs already detached; use the
    /// ordinary lifecycle API for new current-schema detachments.
    pub fn upgrade_existing_detaching(
        state: impl AsRef<Path>,
        application_id: &str,
        agent_key: &str,
        workspace: &Path,
        binding_ids: &[String],
    ) -> Result<StateUpgradeReport, IntegrationError> {
        if binding_ids.is_empty()
            || binding_ids.len() > 500
            || binding_ids
                .iter()
                .any(|id| id.is_empty() || id.len() > 256 || id.contains('\0'))
            || binding_ids.iter().collect::<BTreeSet<_>>().len() != binding_ids.len()
        {
            return Err(IntegrationError::Invalid(
                "select 1-500 distinct recorded binding IDs".into(),
            ));
        }
        let installation = super::owner_sessions::scope(application_id, agent_key, workspace)?;
        Self::upgrade_state_inner(state.as_ref(), Some((&installation, binding_ids)))
    }

    fn initialize_schema(
        &self,
        connection: &Connection,
        allow_create: bool,
    ) -> Result<(), IntegrationError> {
        // Version 15 prevents rebinding observations from before owner detachment.
        // Version 14 fences caller-owned update identity against older receipt writers.
        // Version 13 retains mandatory detach guards for coordinated plans.
        // Version 12 adds unqualified native row checkpoints before/after append;
        // version 11 retains raw handoff returns independently of qualified proof.
        // Older readers must not silently drop those journal semantics.
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if !allow_create && version == 0 {
            return Err(IntegrationError::Invalid(
                "existing integration journal has no supported schema".into(),
            ));
        }
        if version > 15 {
            return Err(IntegrationError::Invalid("newer integration schema".into()));
        }
        if !allow_create && version < 15 {
            return Err(IntegrationError::Invalid(
                "existing integration journal requires an explicit schema upgrade".into(),
            ));
        }
        if version < 15 {
            let _lock = self.state_lock()?;
            // Recheck under the migration lock: another opener may have migrated.
            let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
            if version > 15 {
                return Err(IntegrationError::Invalid("newer integration schema".into()));
            }
            if version < 3 {
                connection.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS installations(id TEXT PRIMARY KEY, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS install_plans(id TEXT PRIMARY KEY, installation_id TEXT NOT NULL, phase TEXT NOT NULL, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS hook_invocations(sequence INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE, installation_id TEXT NOT NULL, session_id TEXT NOT NULL, body TEXT NOT NULL, decision TEXT);
            CREATE INDEX IF NOT EXISTS hooks_by_installation ON hook_invocations(installation_id,sequence);
            ALTER TABLE hook_invocations ADD COLUMN installation_revision TEXT;
            CREATE TABLE installation_revisions(installation_id TEXT PRIMARY KEY, revision TEXT NOT NULL);
            INSERT INTO installation_revisions SELECT id,lower(hex(randomblob(16))) FROM installations;
            CREATE TABLE session_bindings(id TEXT PRIMARY KEY, reference TEXT NOT NULL, detached INTEGER NOT NULL DEFAULT 0);
            CREATE UNIQUE INDEX active_session_binding ON session_bindings(reference) WHERE detached=0;
            PRAGMA user_version=9; COMMIT;")?;
            }
            // Version 3+ already has bindings/revisions. Retain every row;
            // the outgoing-message migration is additive.
            connection.execute_batch("BEGIN IMMEDIATE;")?;
            if let Err(error) = migrate_message_tables(connection) {
                let _ = connection.execute_batch("ROLLBACK;");
                return Err(error);
            }
            connection.execute_batch("COMMIT;")?;
        }
        Ok(())
    }

    pub(super) fn state_lock(&self) -> Result<InstallationLock, IntegrationError> {
        self.state_lock_with_wait(Duration::ZERO)
    }
    pub(super) fn state_lock_with_wait(
        &self,
        budget: Duration,
    ) -> Result<InstallationLock, IntegrationError> {
        lock_with_wait(&self.state.join("integration.lock"), budget)
    }
    // Retaining an already-returned native result may overlap its ensuing
    // Turn-start notification. Wait only for this local journal writer; never
    // retry the native call, semantic validation or a transaction body.
    pub(super) fn handoff_record_lock(&self) -> Result<InstallationLock, IntegrationError> {
        self.state_lock_with_wait(Duration::from_millis(250))
    }
    pub(super) fn connection(&self) -> Result<Connection, IntegrationError> {
        let path = self.state.join("integration.db");
        reject_link(&path)?;
        let flags = if self.allow_create {
            rusqlite::OpenFlags::default()
        } else {
            rusqlite::OpenFlags::default() & !rusqlite::OpenFlags::SQLITE_OPEN_CREATE
        };
        let connection = Connection::open_with_flags(path, flags)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        Ok(connection)
    }

    /// Installation and update follow the same path. Existing owned entries must
    /// still match their receipt; unrelated settings and hooks are retained.
    pub fn plan_install(&self, spec: InstallSpec) -> Result<InstallPlan, IntegrationError> {
        self.plan_install_inner(spec, None, None, None, false)
    }

    /// Request native completion-loop configuration through the existing owned
    /// prepare/apply/remove path. A plan is not evidence of effective native
    /// settings, repeated blocking, or accepted completion. Unsupported adapters
    /// fail explicitly; no managed session or weaker fallback is substituted.
    pub fn plan_install_with_completion_policy(
        &self,
        spec: InstallSpec,
        completion_policy: CompletionBlockingPolicy,
    ) -> Result<InstallPlan, IntegrationError> {
        self.plan_install_inner(spec, Some(completion_policy), None, None, false)
    }

    /// Replace an existing hook installation and its generated callable in one
    /// persisted plan. All recorded bindings for this installation must be
    /// explicitly detached, both at planning and at apply/recovery. This never
    /// stops the owner's agent or grants readiness. The optional message bridge
    /// remains an explicit, unqualified opt-in.
    pub fn plan_update_with_agent_tool(
        &self,
        spec: InstallSpec,
        completion_policy: CompletionBlockingPolicy,
        tool: super::AgentToolSpec,
        message_handler: Option<HookCommand>,
    ) -> Result<InstallPlan, IntegrationError> {
        self.plan_install_inner(
            spec,
            Some(completion_policy),
            Some((tool, message_handler)),
            None,
            false,
        )
    }

    /// Consumer-owned operation ID is also the plan ID. Exact canonical request
    /// retries return the original planned/prepared/applied preview, including
    /// after a lost reply; changed requests or scope conflict. No files are applied.
    pub fn plan_update_with_agent_tool_id(
        &self,
        operation_id: &str,
        spec: InstallSpec,
        completion_policy: CompletionBlockingPolicy,
        tool: super::AgentToolSpec,
        message_handler: Option<HookCommand>,
    ) -> Result<InstallPlan, IntegrationError> {
        validate_operation_id(operation_id)?;
        self.plan_install_inner(
            spec,
            Some(completion_policy),
            Some((tool, message_handler)),
            Some(operation_id),
            false,
        )
    }

    /// Return the canonical installation identity without creating a plan or
    /// modifying configuration. Use this identity in a generated callable spec.
    /// This validates the recipe, not native readiness or effective policy.
    pub fn installation_identity(&self, spec: &InstallSpec) -> Result<String, IntegrationError> {
        let mut spec = spec.clone();
        validate_spec(&mut spec)?;
        if self.state.starts_with(&spec.workspace) {
            return Err(IntegrationError::Invalid(
                "integration state must be outside the reviewed workspace".into(),
            ));
        }
        Ok(installation_id(&spec))
    }

    /// Plan the first hooks, completion policy, callable and instructions as one
    /// recoverable installation. A new operation refuses an existing installation;
    /// exact retries return the original preview even after apply or a lost reply.
    /// No files are applied and no native readiness or activation is granted.
    pub fn plan_initial_with_agent_tool_id(
        &self,
        operation_id: &str,
        spec: InstallSpec,
        completion_policy: CompletionBlockingPolicy,
        tool: super::AgentToolSpec,
        message_handler: Option<HookCommand>,
    ) -> Result<InstallPlan, IntegrationError> {
        validate_operation_id(operation_id)?;
        self.plan_install_inner(
            spec,
            Some(completion_policy),
            Some((tool, message_handler)),
            Some(operation_id),
            true,
        )
    }

    fn plan_install_inner(
        &self,
        mut spec: InstallSpec,
        requested_policy: Option<CompletionBlockingPolicy>,
        bundle: Option<(super::AgentToolSpec, Option<HookCommand>)>,
        operation_id: Option<&str>,
        initial_bundle: bool,
    ) -> Result<InstallPlan, IntegrationError> {
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
        let request_hash = operation_id
            .map(|_| {
                // Preserve the existing update request digest for durable retries.
                let bytes = if initial_bundle {
                    serde_json::to_vec(&(&spec, requested_policy, &bundle, "initial"))
                } else {
                    serde_json::to_vec(&(&spec, requested_policy, &bundle))
                }?;
                Ok::<_, serde_json::Error>(fingerprint(Some(&bytes)))
            })
            .transpose()?;
        if let Some(operation_id) = operation_id {
            let existing: Option<String> = connection
                .query_row(
                    "SELECT body FROM install_plans WHERE id=?1",
                    [operation_id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(existing) = existing {
                let pending: Pending = serde_json::from_str(&existing)?;
                if pending.preview.id != operation_id
                    || pending.preview.installation_id != id
                    || pending.update_request_sha256 != request_hash
                {
                    return Err(IntegrationError::Conflict(
                        "installation operation ID already has another request".into(),
                    ));
                }
                return Ok(pending.preview);
            }
        }
        reject_pending(&connection, &id)?;
        reject_prepared_config(&connection, &config_path, None)?;
        let previous = receipt_json(&connection, &id)?;
        let plugin = if let Some((tool, message_handler)) = &bundle {
            if initial_bundle && previous.is_some() {
                return Err(IntegrationError::Conflict(
                    "initial installation already exists; use the update operation".into(),
                ));
            }
            if !initial_bundle && previous.is_none() {
                return Err(IntegrationError::NotFound);
            }
            require_detached_bindings(&connection, &id)?;
            if tool.application_id != spec.application_id || tool.installation_id != id {
                return Err(IntegrationError::Invalid(
                    "tool identity must match its installation".into(),
                ));
            }
            if message_handler.is_some() && !spec.events.contains(&HookEvent::InputSubmitted) {
                return Err(IntegrationError::Invalid(
                    "message bridge requires input observations".into(),
                ));
            }
            super::agent_tools::validate_completion_events(tool, &spec.events)?;
            Some(super::agent_tools::render_with_message_handler(
                &spec.agent_key,
                tool,
                Some(&spec.handler),
                message_handler.as_ref(),
            )?)
        } else {
            None
        };
        let retained_tool = previous
            .as_deref()
            .map(serde_json::from_str::<Receipt>)
            .transpose()?
            .and_then(|receipt| receipt.agent_tool);
        if let Some(tool) = &retained_tool {
            check_agent_tool(&spec.workspace, tool)?;
            let old: Receipt =
                serde_json::from_str(previous.as_deref().expect("retained receipt"))?;
            if bundle.is_none()
                && tool.invocation_env_key.is_some()
                && old.installation.spec.handler != spec.handler
            {
                return Err(IntegrationError::Conflict("owned native startup handler changed; remove/reinstall the integration while idle before replacing its command".into()));
            }
        }
        // Legacy callers updating an installation retain its explicit policy.
        // Choose under the metadata lock so concurrent plans cannot downgrade it.
        let completion_policy = match requested_policy {
            Some(policy) => policy,
            None => previous
                .as_deref()
                .map(serde_json::from_str::<Receipt>)
                .transpose()?
                .map(|receipt| receipt.installation.completion_policy)
                .unwrap_or_default(),
        };
        completion::validate(&spec, completion_policy)?;
        let before = read_config(&spec.workspace, &config_path)?;
        if spec.agent_key == "pi" {
            if let Some(raw) = &previous {
                check_source(before.as_deref(), &serde_json::from_str(raw)?)?;
            } else if before.is_some() {
                return Err(IntegrationError::Conflict(
                    "unowned Pi extension already exists".into(),
                ));
            }
            let installation = Installation {
                id,
                spec,
                config_path,
                completion_policy,
            };
            let after = super::pi::extension(&installation)?;
            let receipt = Receipt {
                installation: installation.clone(),
                hooks_existed: false,
                owned: Vec::new(),
                inserted_version: false,
                source_fingerprint: Some(fingerprint(Some(&after))),
                completion: None,
                agent_tool: None,
            };
            return self.save_bytes_plan(
                &connection,
                before,
                Some(after),
                previous,
                Some(receipt),
                &installation,
            );
        }
        let mut document = document(before.as_deref())?;
        let mut retained_completion = None;
        let mut retained_ownership = None;
        if let Some(raw) = &previous {
            let receipt: Receipt = serde_json::from_str(raw)?;
            if completion_policy == CompletionBlockingPolicy::RequireUnbounded
                && receipt.completion.is_some()
            {
                strip_owned(&mut document, &receipt, false)?;
                retained_completion = receipt.completion.clone();
                retained_ownership = Some(receipt);
            } else {
                remove_owned(&mut document, &receipt)?;
            }
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
            completion_policy,
        };
        let completion = match retained_completion {
            Some(retained) => Some(retained),
            None => completion::install(&mut document, completion_policy)?,
        };
        let mut receipt = add_owned(&mut document, installation)?;
        if let Some(previous) = retained_ownership {
            receipt.hooks_existed = previous.hooks_existed;
            for hook in &mut receipt.owned {
                if let Some(old) = previous.owned.iter().find(|old| old.event == hook.event) {
                    hook.event_existed = old.event_existed;
                }
            }
        }
        receipt.completion = completion;
        receipt.agent_tool = plugin.as_ref().map(owned_agent_tool).or(retained_tool);
        if let Some(plugin) = &plugin {
            return self.save_bytes_plan_with_tool(
                &connection,
                before,
                Some(serde_json::to_vec_pretty(&document)?),
                previous,
                Some(receipt.clone()),
                &receipt.installation,
                Some(plugin),
                true,
                operation_id.zip(request_hash),
            );
        }
        self.save_plan(
            &connection,
            before,
            document,
            previous,
            Some(receipt.clone()),
            &receipt.installation,
        )
    }

    /// Reuse an unstarted removal's original ID only for the exact same recipe.
    /// A lost planning reply must not leave another unfinished removal behind.
    /// Changed configuration requires inspection, never silent retargeting.
    pub fn plan_remove(&self, id: &str) -> Result<InstallPlan, IntegrationError> {
        let _lock = self.state_lock()?;
        let connection = self.connection()?;
        reject_pending(&connection, id)?;
        let raw = receipt_json(&connection, id)?.ok_or(IntegrationError::NotFound)?;
        let receipt: Receipt = serde_json::from_str(&raw)?;
        if let Some(tool) = &receipt.agent_tool {
            check_agent_tool(&receipt.installation.spec.workspace, tool)?;
        }
        reject_prepared_config(&connection, &receipt.installation.config_path, None)?;
        let before = read_config(
            &receipt.installation.spec.workspace,
            &receipt.installation.config_path,
        )?;
        if receipt.source_fingerprint.is_some() {
            check_source(before.as_deref(), &receipt)?;
            return self.save_bytes_plan(
                &connection,
                before,
                None,
                Some(raw),
                None,
                &receipt.installation,
            );
        }
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
        self.save_bytes_plan(
            connection,
            before,
            Some(after),
            previous_receipt,
            next_receipt,
            installation,
        )
    }

    fn save_bytes_plan(
        &self,
        connection: &Connection,
        before: Option<Vec<u8>>,
        after: Option<Vec<u8>>,
        previous_receipt: Option<String>,
        next_receipt: Option<Receipt>,
        installation: &Installation,
    ) -> Result<InstallPlan, IntegrationError> {
        self.save_bytes_plan_with_tool(
            connection,
            before,
            after,
            previous_receipt,
            next_receipt,
            installation,
            None,
            false,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn save_bytes_plan_with_tool(
        &self,
        connection: &Connection,
        before: Option<Vec<u8>>,
        after: Option<Vec<u8>>,
        previous_receipt: Option<String>,
        next_receipt: Option<Receipt>,
        installation: &Installation,
        plugin: Option<&super::AgentToolPlugin>,
        require_detached: bool,
        request_identity: Option<(&str, String)>,
    ) -> Result<InstallPlan, IntegrationError> {
        if after
            .as_ref()
            .is_some_and(|bytes| bytes.len() as u64 > MAX_CONFIG_BYTES)
        {
            return Err(IntegrationError::Invalid(
                "resulting config exceeds 1 MiB".into(),
            ));
        }
        let tool_files = tool_file_changes(
            &installation.spec.workspace,
            previous_receipt.as_deref(),
            next_receipt.as_ref(),
            plugin,
        )?;
        let preview = InstallPlan {
            id: request_identity
                .as_ref()
                .map(|(id, _)| (*id).to_owned())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            installation_id: installation.id.clone(),
            config_path: installation.config_path.clone(),
            events: installation.spec.events.clone(),
            removal: next_receipt.is_none(),
            before_fingerprint: fingerprint(before.as_deref()),
            after_fingerprint: fingerprint(after.as_deref()),
            settings: if installation.spec.agent_key == "claude" {
                completion::changes(before.as_deref(), after.as_deref())?
            } else {
                Vec::new()
            },
            completion_policy: installation.completion_policy,
            files: tool_files
                .iter()
                .map(|file| AgentToolFileChange {
                    path: file.path.clone(),
                    before_fingerprint: file.before.clone(),
                    after_fingerprint: fingerprint(file.after.as_deref()),
                })
                .collect(),
        };
        let pending = Pending {
            preview: preview.clone(),
            before: preview.before_fingerprint.clone(),
            delete: after.is_none(),
            after: after.unwrap_or_default(),
            previous_receipt,
            next_receipt,
            workspace: installation.spec.workspace.clone(),
            tool_files,
            require_detached,
            update_request_sha256: request_identity.map(|(_, hash)| hash),
        };
        if preview.removal {
            let mut statement = connection.prepare(
                "SELECT id,body FROM install_plans WHERE installation_id=?1 AND phase='planned' ORDER BY id",
            )?;
            let rows = statement.query_map([&preview.installation_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut retained = None;
            for row in rows {
                let (id, body) = row?;
                let previous: Pending = serde_json::from_str(&body)?;
                if !previous.preview.removal {
                    continue;
                }
                if retained.is_some() {
                    return Err(IntegrationError::Conflict(
                        "multiple unstarted removal plans require inspection; no plan was replaced"
                            .into(),
                    ));
                }
                let mut comparable = previous.preview.clone();
                comparable.id = preview.id.clone();
                if previous.preview.id != id
                    || comparable != preview
                    || previous.before != pending.before
                    || previous.after != pending.after
                    || previous.delete != pending.delete
                    || previous.previous_receipt != pending.previous_receipt
                    || previous.next_receipt.is_some()
                    || previous.workspace != pending.workspace
                    || previous.tool_files != pending.tool_files
                    || previous.require_detached != pending.require_detached
                    || previous.update_request_sha256 != pending.update_request_sha256
                {
                    return Err(IntegrationError::Conflict(format!(
                        "unstarted removal plan {id} differs from the current request; preserve it and inspect configuration"
                    )));
                }
                retained = Some(previous.preview);
            }
            if let Some(previous) = retained {
                return Ok(previous);
            }
        }
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
        self.apply_with_tool_fault(plan_id, interrupt_after_write, None)
    }

    fn apply_with_tool_fault(
        &self,
        plan_id: &str,
        interrupt_after_write: bool,
        interrupt_after_tool: Option<usize>,
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
        if pending.require_detached {
            require_detached_bindings(&connection, &pending.preview.installation_id)?;
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
        if phase == "planned" {
            let previous = pending
                .previous_receipt
                .as_deref()
                .map(serde_json::from_str::<Receipt>)
                .transpose()?;
            if previous
                .as_ref()
                .and_then(|receipt| receipt.agent_tool.as_ref())
                .is_none()
            {
                if let Some(tool) = pending
                    .next_receipt
                    .as_ref()
                    .and_then(|receipt| receipt.agent_tool.as_ref())
                {
                    let root = pending.workspace.join(&tool.relative_root);
                    check_config_path(&pending.workspace, &root)?;
                    if root.try_exists()? {
                        return Err(IntegrationError::Conflict(
                            "unowned tool directory appeared after planning".into(),
                        ));
                    }
                }
            }
        }
        if let Some(tool) = pending
            .next_receipt
            .as_ref()
            .and_then(|receipt| receipt.agent_tool.as_ref())
        {
            for (name, expected) in &tool.files {
                let path = pending.workspace.join(&tool.relative_root).join(name);
                if !pending.tool_files.iter().any(|file| file.path == path)
                    && fingerprint(read_config(&pending.workspace, &path)?.as_deref()) != *expected
                {
                    return Err(IntegrationError::Conflict(
                        "unchanged owned tool file drifted after planning".into(),
                    ));
                }
            }
        }
        // Validate every file before the first write. Only a prepared plan may
        // accept its own exact after bytes when recovering a partial application.
        for file in &pending.tool_files {
            let hash = fingerprint(read_config(&pending.workspace, &file.path)?.as_deref());
            if hash != file.before
                && !(phase == "prepared" && hash == fingerprint(file.after.as_deref()))
            {
                return Err(IntegrationError::Conflict(
                    "owned tool file changed since planning".into(),
                ));
            }
        }
        connection.execute(
            "UPDATE install_plans SET phase='prepared' WHERE id=?1",
            [plan_id],
        )?;
        // Install/update module files before publishing the manifest. Removal
        // withdraws the manifest first. Hooks refuse a prepared generation.
        for (index, file) in pending.tool_files.iter().enumerate() {
            let hash = fingerprint(read_config(&pending.workspace, &file.path)?.as_deref());
            if hash != fingerprint(file.after.as_deref()) {
                ensure_config_parent(&pending.workspace, &file.path)?;
                if let Some(bytes) = &file.after {
                    replace_config(&pending.workspace, &file.path, &file.before, bytes)?;
                } else {
                    // Recheck immediately before removing the exact owned file.
                    if fingerprint(read_config(&pending.workspace, &file.path)?.as_deref())
                        != file.before
                    {
                        return Err(IntegrationError::Conflict(
                            "owned tool file changed before removal".into(),
                        ));
                    }
                    fs::remove_file(&file.path)?;
                    #[cfg(unix)]
                    fs::File::open(file.path.parent().expect("validated parent"))?.sync_all()?;
                }
            }
            if interrupt_after_tool == Some(index + 1) {
                return Err(IntegrationError::RecoveryRequired(plan_id.into()));
            }
        }
        prune_removed_tool_directories(
            &pending.workspace,
            pending.previous_receipt.as_deref(),
            pending.next_receipt.as_ref(),
        )?;
        if !already_written {
            if pending.delete {
                if fingerprint(read_config(&pending.workspace, config)?.as_deref())
                    != pending.before
                {
                    return Err(IntegrationError::Conflict(
                        "extension changed before removal".into(),
                    ));
                }
                fs::remove_file(config)?;
                #[cfg(unix)]
                fs::File::open(config.parent().expect("validated parent"))?.sync_all()?;
            } else {
                replace_config(&pending.workspace, config, &pending.before, &pending.after)?;
            }
        }
        // Fault injection exercises the actual recovery boundary; not public API.
        if interrupt_after_write {
            return Err(IntegrationError::RecoveryRequired(plan_id.into()));
        }
        let tx = connection.transaction()?;
        if let Some(receipt) = &pending.next_receipt {
            tx.execute("INSERT INTO installations VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body", params![receipt.installation.id, serde_json::to_string(receipt)?])?;
            tx.execute("INSERT INTO installation_revisions VALUES (?1,?2) ON CONFLICT(installation_id) DO UPDATE SET revision=excluded.revision", params![receipt.installation.id, plan_id])?;
        } else {
            tx.execute(
                "DELETE FROM installations WHERE id=?1",
                [&pending.preview.installation_id],
            )?;
            tx.execute(
                "DELETE FROM installation_revisions WHERE installation_id=?1",
                [&pending.preview.installation_id],
            )?;
        }
        let mut completed = pending.clone();
        completed.after.clear();
        completed.previous_receipt = None;
        completed.next_receipt = None;
        completed.tool_files.clear();
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

    /// Validate the exact applied revision and keep it pinned for a consumer's
    /// short metadata commit. A superseded plan cannot authorize that commit.
    pub fn applied_installation_guard(
        &self,
        plan_id: &str,
    ) -> Result<AppliedInstallationGuard, IntegrationError> {
        let guard = self.state_lock()?;
        let connection = self.connection()?;
        let (phase, body): (String, String) = connection
            .query_row(
                "SELECT phase,body FROM install_plans WHERE id=?1",
                [plan_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(IntegrationError::NotFound)?;
        let pending: Pending = serde_json::from_str(&body)?;
        let revision: Option<String> = connection
            .query_row(
                "SELECT revision FROM installation_revisions WHERE installation_id=?1",
                [&pending.preview.installation_id],
                |r| r.get(0),
            )
            .optional()?;
        if phase != "applied" || pending.preview.removal || revision.as_deref() != Some(plan_id) {
            return Err(IntegrationError::Conflict(
                "installation plan is not the current applied revision".into(),
            ));
        }
        if pending.require_detached {
            require_detached_bindings(&connection, &pending.preview.installation_id)?;
        }
        let InstallationStatus::Configured { installation } =
            self.status_inner(&connection, &pending.preview.installation_id)?
        else {
            return Err(IntegrationError::Conflict(
                "applied installation has drifted".into(),
            ));
        };
        Ok(AppliedInstallationGuard {
            installation,
            _lock: guard,
        })
    }

    /// Cancel only a caller-ID update that has never begun applying. Missing
    /// intent is harmless; prepared/applied or foreign plans are never removed.
    /// This deletes an unapplied preview only, without touching provider files.
    pub fn discard_planned_update(
        &self,
        operation_id: &str,
        application_id: &str,
        agent_key: &str,
        workspace: &Path,
    ) -> Result<(), IntegrationError> {
        if crate::agent(agent_key).is_none() {
            return Err(IntegrationError::UnknownAgent(agent_key.into()));
        }
        let workspace = fs::canonicalize(workspace)?;
        let id = installation_key(agent_key, application_id, &workspace);
        let _guard = self.state_lock()?;
        let connection = self.connection()?;
        let current: Option<String> = connection
            .query_row(
                "SELECT revision FROM installation_revisions WHERE installation_id=?1",
                [&id],
                |r| r.get(0),
            )
            .optional()?;
        if current.as_deref() == Some(operation_id) {
            return Err(IntegrationError::Conflict(
                "applied update cannot be discarded".into(),
            ));
        }
        let existing: Option<(String, String)> = connection
            .query_row(
                "SELECT phase,body FROM install_plans WHERE id=?1",
                [operation_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((phase, body)) = existing else {
            return Ok(());
        };
        let pending: Pending = serde_json::from_str(&body)?;
        if phase != "planned"
            || pending.preview.id != operation_id
            || pending.preview.installation_id != id
            || pending.workspace != workspace
            || pending.update_request_sha256.is_none()
        {
            return Err(IntegrationError::Conflict(
                "only an unstarted owned update can be discarded".into(),
            ));
        }
        connection.execute(
            "DELETE FROM install_plans WHERE id=?1 AND phase='planned'",
            [operation_id],
        )?;
        Ok(())
    }

    /// Inspect an interrupted apply for one owner-configured installation.
    /// Returns only a prepared plan, never an unapplied preview or another
    /// application's plan. This is a read, not authorization to apply it: the
    /// consumer must inspect `removal` and retain its own lifecycle guards.
    /// `apply_install` rechecks fingerprints and preserves conflicting edits.
    pub fn pending_installation_plan(
        &self,
        application_id: &str,
        agent_key: &str,
        workspace: &Path,
    ) -> Result<Option<InstallPlan>, IntegrationError> {
        if crate::agent(agent_key).is_none() {
            return Err(IntegrationError::UnknownAgent(agent_key.into()));
        }
        let workspace = fs::canonicalize(workspace)?;
        let installation_id = installation_key(agent_key, application_id, &workspace);
        let _lock = self.state_lock()?;
        let connection = self.connection()?;
        let Some(id) = prepared_plan(&connection, &installation_id)? else {
            return Ok(None);
        };
        let body: String = connection.query_row(
            "SELECT body FROM install_plans WHERE id=?1 AND installation_id=?2 AND phase='prepared'",
            params![id, installation_id], |row| row.get(0),
        )?;
        let pending: Pending = serde_json::from_str(&body)?;
        if pending.preview.id != id
            || pending.preview.installation_id != installation_id
            || pending.workspace != workspace
            || pending.preview.removal != pending.next_receipt.is_none()
        {
            return Err(IntegrationError::Conflict(
                "pending installation scope does not match".into(),
            ));
        }
        Ok(Some(pending.preview))
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

    pub(super) fn agent_tool_environment_key(
        &self,
        id: &str,
    ) -> Result<Option<String>, IntegrationError> {
        let raw = receipt_json(&self.connection()?, id)?.ok_or(IntegrationError::NotFound)?;
        let receipt: Receipt = serde_json::from_str(&raw)?;
        Ok(receipt.agent_tool.and_then(|tool| tool.invocation_env_key))
    }

    pub(super) fn status_inner(
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
        let check = if receipt.source_fingerprint.is_some() {
            check_source(current.as_deref(), &receipt)
        } else {
            document(current.as_deref()).and_then(|mut value| {
                if value.get("disableAllHooks").and_then(Value::as_bool) == Some(true) {
                    return Err(IntegrationError::Conflict("hooks are disabled".into()));
                }
                strip_owned(&mut value, &receipt, false)
            })
        };
        let check = check.and_then(|()| match &receipt.agent_tool {
            Some(tool) => check_agent_tool(&receipt.installation.spec.workspace, tool),
            None => Ok(()),
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

fn validate_operation_id(operation_id: &str) -> Result<(), IntegrationError> {
    if operation_id.is_empty()
        || operation_id.len() > 256
        || !operation_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(IntegrationError::Invalid(
            "invalid installation operation ID".into(),
        ));
    }
    Ok(())
}

fn validate_spec(spec: &mut InstallSpec) -> Result<(), IntegrationError> {
    if crate::agent(&spec.agent_key).is_none() {
        return Err(IntegrationError::UnknownAgent(spec.agent_key.clone()));
    }
    if !matches!(
        spec.agent_key.as_str(),
        "claude" | "cursor" | "codex" | "pi"
    ) {
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
    super::command::validate_handler(&spec.handler)?;
    if spec.events.iter().any(|event| {
        matches!(
            event,
            HookEvent::NativeTurnStarted | HookEvent::NativeTurnFinished
        )
    }) {
        return Err(IntegrationError::Invalid(
            "native Turn observations require the owned module, not settings hooks".into(),
        ));
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
    if spec.agent_key == "pi" && !spec.events.contains(&HookEvent::SessionStarted) {
        return Err(IntegrationError::Invalid(
            "Pi invocation-scoped hooks require SessionStarted".into(),
        ));
    }
    if spec.agent_key == "cursor" {
        for event in &spec.events {
            super::cursor::event_name(*event)?;
        }
    }
    if spec.agent_key == "codex" {
        for event in &spec.events {
            super::codex::event_name(*event)?;
        }
        if spec.events.contains(&HookEvent::SessionEnded) && spec.timeout_seconds > 3 {
            return Err(IntegrationError::Unsupported(
                "Codex SessionEnd requires a timeout of at most 3 seconds".into(),
            ));
        }
    }
    Ok(())
}

fn config_path(spec: &InstallSpec) -> Result<PathBuf, IntegrationError> {
    match spec.agent_key.as_str() {
        "claude" => Ok(spec.workspace.join(".claude").join("settings.local.json")),
        "cursor" => Ok(spec.workspace.join(".cursor").join("hooks.json")),
        "codex" => Ok(spec.workspace.join(".codex").join("hooks.json")),
        "pi" => Ok(spec
            .workspace
            .join(".pi")
            .join("extensions")
            .join(format!("aikit-{}.js", installation_id(spec)))),
        _ => Err(IntegrationError::Unsupported(
            "hook configuration layout".into(),
        )),
    }
}
fn installation_id(spec: &InstallSpec) -> String {
    installation_key(&spec.agent_key, &spec.application_id, &spec.workspace)
}
pub(super) fn installation_key(agent_key: &str, application_id: &str, workspace: &Path) -> String {
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
fn require_detached_bindings(connection: &Connection, id: &str) -> Result<(), IntegrationError> {
    let mut query =
        connection.prepare("SELECT reference FROM session_bindings WHERE detached=0")?;
    for row in query.query_map([], |row| row.get::<_, String>(0))? {
        let reference: super::SessionRef = serde_json::from_str(&row?)?;
        if reference.installation_id == id {
            return Err(IntegrationError::Conflict("detach recorded bindings before coordinated installation update; no agent was stopped".into()));
        }
    }
    Ok(())
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
    let inserted_version = if installation.spec.agent_key == "cursor" {
        match root.get("version") {
            Some(value) if value.as_u64() == Some(1) => false,
            Some(_) => {
                return Err(IntegrationError::Conflict(
                    "unsupported Cursor config version".into(),
                ))
            }
            None => {
                root.insert("version".into(), json!(1));
                true
            }
        }
    } else {
        false
    };
    let hooks_existed = root.contains_key("hooks");
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| IntegrationError::Invalid("hooks must be an object".into()))?;
    let mut owned = Vec::new();
    for event in &installation.spec.events {
        let event = if installation.spec.agent_key == "cursor" {
            super::cursor::event_name(*event)?
        } else if installation.spec.agent_key == "codex" {
            super::codex::event_name(*event)?
        } else {
            event.claude_name()
        };
        let event_existed = hooks.contains_key(event);
        let entries = hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| IntegrationError::Invalid(format!("{event} hooks must be an array")))?;
        let entry = if installation.spec.agent_key == "cursor" {
            json!({"command":super::command::command(&installation.spec.handler)?,"timeout":installation.spec.timeout_seconds,"failClosed":true})
        } else if installation.spec.agent_key == "codex" {
            json!({"hooks":[{"type":"command","command":super::command::command(&installation.spec.handler)?,"timeout":installation.spec.timeout_seconds}]})
        } else {
            json!({"hooks":[{"type":"command","command":installation.spec.handler.executable,"args":installation.spec.handler.arguments,"timeout":installation.spec.timeout_seconds}]})
        };
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
        inserted_version,
        source_fingerprint: None,
        completion: None,
        agent_tool: None,
    })
}

fn check_source(current: Option<&[u8]>, receipt: &Receipt) -> Result<(), IntegrationError> {
    if receipt.source_fingerprint.as_deref() != Some(fingerprint(current).as_str()) {
        return Err(IntegrationError::Conflict(
            "owned extension was removed or edited".into(),
        ));
    }
    Ok(())
}
fn remove_owned(document: &mut Value, receipt: &Receipt) -> Result<(), IntegrationError> {
    strip_owned(document, receipt, true)
}
fn strip_owned(
    document: &mut Value,
    receipt: &Receipt,
    restore_completion: bool,
) -> Result<(), IntegrationError> {
    if receipt.installation.spec.agent_key == "cursor"
        && document.get("version").and_then(Value::as_u64) != Some(1)
    {
        return Err(IntegrationError::Conflict(
            "Cursor config version changed".into(),
        ));
    }
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
    // Preserve the required version if another application's hooks remain.
    if receipt.inserted_version && document.get("hooks").is_none() {
        document.as_object_mut().expect("object").remove("version");
    }
    if let Some(completion) = &receipt.completion {
        if restore_completion {
            completion::remove(document, completion)?;
        } else {
            completion::check(document)?;
        }
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
// Explicitly unlock before closing: a concurrent Unix fork can inherit the
// open file description until exec, extending a close-only lock's lifetime.
pub(super) struct InstallationLock(fs::File);

impl Drop for InstallationLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

fn lock(path: &Path) -> Result<InstallationLock, IntegrationError> {
    lock_with_wait(path, Duration::ZERO)
}

fn lock_with_wait(path: &Path, budget: Duration) -> Result<InstallationLock, IntegrationError> {
    reject_link(path)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    let started = std::time::Instant::now();
    loop {
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => break,
            Err(error) => {
                let contention = fs2::lock_contended_error();
                let busy = error.kind() == std::io::ErrorKind::WouldBlock
                    || (error.raw_os_error().is_some()
                        && error.raw_os_error() == contention.raw_os_error());
                if !busy || started.elapsed() >= budget {
                    return Err(IntegrationError::Conflict(
                        "another integration writer holds the lock".into(),
                    ));
                }
                std::thread::sleep(
                    Duration::from_millis(5).min(budget.saturating_sub(started.elapsed())),
                );
            }
        }
    }
    Ok(InstallationLock(file))
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
