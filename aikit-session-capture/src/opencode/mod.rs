//! OpenCode adapter: parses `~/.opencode/opencode.db` (SQLite).
//!
//! See spec 010 §12.3. **Different shape from Claude/Codex** — OpenCode
//! persists state in SQLite, not JSONL. The adapter queries the DB rather
//! than streaming bytes; `from_offset` is a `time_updated` watermark, NOT a
//! byte offset. The trait signature tolerates this because `from_offset:
//! u64` is opaque.
//!
//! Watch paths (cross-mount union, mirrors `opencode/adapter.go:1323`):
//!   - sst/opencode CLI (canonical): `~/.opencode/`
//!   - XDG_DATA fallback: `~/.local/share/opencode/`
//!   - Desktop variant (per-OS): `~/AppData/Roaming/ai.opencode.desktop/`
//!     (Windows), `~/Library/Application Support/ai.opencode.desktop/`
//!     (macOS), `~/.config/ai.opencode.desktop/` (Linux)

mod db;
mod mirror;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use rusqlite::params;

#[allow(unused_imports)]
use crate::adapter::ParseWarning; // retained for future ForeignMountRetry emission
use crate::adapter::{Adapter, AdapterError, ParseResult};
use crate::homes::HomeResolver;
use crate::models::{ActionKind, ActionStatus, CaptureSource, TokenEvent, ToolEvent, ToolKind};
use crate::scrub::SecretScrubber;

pub struct OpenCodeAdapter {
    scrubber: SecretScrubber,
    homes: Vec<crate::homes::HomeRoot>,
    override_roots: Option<Vec<PathBuf>>,
}

impl OpenCodeAdapter {
    pub fn new() -> Self {
        Self {
            scrubber: SecretScrubber::default(),
            homes: crate::homes::DefaultHomeResolver.homes(),
            override_roots: None,
        }
    }

    pub fn with(scrubber: SecretScrubber, homes: Vec<crate::homes::HomeRoot>) -> Self {
        Self {
            scrubber,
            homes,
            override_roots: None,
        }
    }

    pub fn with_override_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.override_roots = Some(roots);
        self
    }
}

impl Default for OpenCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Adapter for OpenCodeAdapter {
    fn kind(&self) -> ToolKind {
        ToolKind::OpenCode
    }

    fn watch_paths(&self) -> Vec<PathBuf> {
        if let Some(roots) = &self.override_roots {
            return roots.clone();
        }
        // Crossmount-resolved per-home expansion. Mirrors observer's
        // `defaultRoots()` (opencode/adapter.go:1323).
        let mut roots = Vec::new();
        for h in &self.homes {
            roots.push(h.path.join(".opencode"));
            roots.push(h.path.join(".local").join("share").join("opencode"));
            match h.os {
                crate::homes::HomeOs::Windows => {
                    roots.push(
                        h.path
                            .join("AppData")
                            .join("Roaming")
                            .join("ai.opencode.desktop"),
                    );
                }
                crate::homes::HomeOs::Darwin => {
                    roots.push(
                        h.path
                            .join("Library")
                            .join("Application Support")
                            .join("ai.opencode.desktop"),
                    );
                }
                crate::homes::HomeOs::Linux => {
                    roots.push(h.path.join(".config").join("ai.opencode.desktop"));
                }
            }
        }
        roots
    }

    fn is_session_file(&self, path: &Path) -> bool {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        let lower = name.to_ascii_lowercase();
        if lower != "opencode.db" && lower != "opencode.db-wal" {
            return false;
        }
        self.watch_paths().iter().any(|root| path.starts_with(root))
    }

    async fn parse_session_file(
        &self,
        path: &Path,
        from_offset: u64,
    ) -> Result<ParseResult, AdapterError> {
        // Stage a mirror for foreign-mount sources (spec §19.3). Native
        // sources short-circuit and open directly.
        let db_path = mirror::stage_mirror_if_foreign(path)?;
        // Open inside spawn_blocking — rusqlite is sync.
        let scrubber = self.scrubber.clone();
        let path_owned = path.to_path_buf();
        let db_path = tokio::task::spawn_blocking(move || -> Result<ParseResult, AdapterError> {
            let conn = db::open_read_only(&db_path)?;
            let mut res = ParseResult {
                new_offset: from_offset,
                ..Default::default()
            };

            // High-water mark → if no new rows, short-circuit.
            let latest = db::latest_watermark(&conn);
            if latest <= from_offset as i64 {
                // No new rows since the last parse. Return the actual
                // high-water mark so the host stores the truth (not the
                // inflated requested offset).
                res.new_offset = latest as u64;
                return Ok(res);
            }

            // Tool events from the `part` table (type='tool').
            res.tool_events.extend(load_tool_events(
                &conn,
                &path_owned,
                from_offset as i64,
                &scrubber,
            )?);

            // User-prompt events from `message` table (role='user').
            res.tool_events.extend(load_user_prompts(
                &conn,
                &path_owned,
                from_offset as i64,
                &scrubber,
            )?);

            // Token events from `message` (role='assistant').
            res.token_events = load_token_events(&conn, &path_owned, from_offset as i64)?;

            res.new_offset = latest as u64;
            Ok(res)
        })
        .await
        .map_err(|e| AdapterError::Other(anyhow::anyhow!("spawn_blocking join: {e}")))??;

        Ok(db_path)
    }
}

// ---------------------------------------------------------------------------
// SQL query helpers — mirror observer's loadToolEvents / loadUserPromptEvents
// / loadTokenEvents (opencode/adapter.go:268, 360, 823).
// ---------------------------------------------------------------------------

fn load_tool_events(
    db: &rusqlite::Connection,
    source_file: &Path,
    from_offset: i64,
    scrubber: &SecretScrubber,
) -> Result<Vec<ToolEvent>, AdapterError> {
    let mut stmt = db
        .prepare(
            "SELECT p.id, p.message_id, p.session_id, \
                    COALESCE(s.directory, ''), p.time_created, p.time_updated, p.data, m.data \
             FROM part p \
             JOIN message m ON m.id = p.message_id \
             LEFT JOIN session s ON s.id = p.session_id \
             WHERE p.time_updated > ?1 \
               AND json_extract(p.data, '$.type') = 'tool' \
             ORDER BY p.time_updated ASC, p.id ASC",
        )
        .map_err(|e| AdapterError::SqliteOpen {
            path: source_file.to_path_buf(),
            source: e,
        })?;
    let rows = stmt
        .query_map(params![from_offset], |row| {
            Ok(PartRow {
                id: row.get(0)?,
                message_id: row.get(1)?,
                session_id: row.get(2)?,
                directory: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                time_created: row.get(4)?,
                time_updated: row.get(5)?,
                data: row.get(6)?,
                message: row.get(7)?,
            })
        })
        .map_err(|e| AdapterError::SqliteOpen {
            path: source_file.to_path_buf(),
            source: e,
        })?;
    let mut out = Vec::new();
    for row in rows {
        let row = row.map_err(|e| AdapterError::SqliteOpen {
            path: source_file.to_path_buf(),
            source: e,
        })?;
        if let Some(ev) = build_tool_event(source_file, &row, scrubber) {
            out.push(ev);
        }
    }
    Ok(out)
}

fn build_tool_event(
    source_file: &Path,
    row: &PartRow,
    scrubber: &SecretScrubber,
) -> Option<ToolEvent> {
    let msg: MessageData = serde_json::from_str(&row.message).ok()?;
    let part: ToolPartData = serde_json::from_str(&row.data).ok()?;
    if part.r#type != "tool" {
        return None;
    }
    let (kind, target, success, err_msg) = map_tool(&part);
    let when_ms = if part.state.time.start > 0 {
        Some(part.state.time.start)
    } else if msg.time.created > 0 {
        Some(msg.time.created)
    } else {
        row.time_created
    };
    let duration_ms = if part.state.time.start > 0 && part.state.time.end > part.state.time.start {
        Some((part.state.time.end - part.state.time.start) as u64)
    } else {
        None
    };
    let raw_input = part
        .state
        .input
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_default();
    let scrubbed_input = if raw_input.is_empty() {
        None
    } else {
        Some(scrubber.scrub(&raw_input))
    };
    let output = if !part.state.output.is_empty() {
        part.state.output.clone()
    } else {
        part.state.metadata.output.clone()
    };
    let scrubbed_output = if output.is_empty() {
        None
    } else {
        Some(scrubber.scrub(&output))
    };
    Some(ToolEvent {
        source_event_id: format!("opencode:part:{}", row.id),
        source_file: source_file.to_path_buf(),
        session_id: row.session_id.clone(),
        tool: ToolKind::OpenCode,
        kind,
        target: target.map(|t| truncate_str(&scrubber.scrub(&t), 200).to_string()),
        input: scrubbed_input,
        output: scrubbed_output,
        status: if success {
            ActionStatus::Success
        } else {
            ActionStatus::Failure
        },
        error_message: err_msg.map(|m| truncate_str(&m, 500).to_string()),
        started_at_ms: when_ms,
        duration_ms,
        git_root: if msg.path.cwd.is_empty() {
            if row.directory.is_empty() {
                None
            } else {
                Some(PathBuf::from(&row.directory))
            }
        } else {
            Some(PathBuf::from(&msg.path.cwd))
        },
        metadata: serde_json::json!({
            "tool": part.tool,
            "model": msg.model.model_id,
            "variant": msg.variant,
        }),
    })
}

fn map_tool(part: &ToolPartData) -> (ActionKind, Option<String>, bool, Option<String>) {
    let input: ToolInput = part
        .state
        .input
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let tool_lower = part.tool.to_ascii_lowercase();
    let mut success =
        part.state.status.is_empty() || part.state.status.eq_ignore_ascii_case("completed");
    let mut err_msg = None;
    let kind = match tool_lower.as_str() {
        "bash" | "shell" | "command" | "powershell" | "pwsh" | "cmd" | "cmd.exe" => {
            if part.state.metadata.exit != 0 {
                success = false;
                err_msg = if !part.state.output.is_empty() {
                    Some(part.state.output.clone())
                } else {
                    Some(part.state.metadata.output.clone())
                };
            }
            ActionKind::Bash
        }
        "read" | "cat" | "view" => ActionKind::Read,
        "write" | "create" => ActionKind::Write,
        "edit" | "patch" | "replace" | "multiedit" | "applypatch" | "apply_patch" => {
            ActionKind::Edit
        }
        "grep" | "search" | "rg" => ActionKind::Search,
        "glob" | "find" | "ls" => ActionKind::Glob,
        "webfetch" | "fetch" | "http" => ActionKind::WebFetch,
        "websearch" => ActionKind::WebSearch,
        "task" | "agent" | "subagent" => ActionKind::Subagent,
        "todoread" | "todowrite" | "todo" => ActionKind::Plan,
        n if n.contains("mcp") => ActionKind::Mcp,
        _ => ActionKind::Other,
    };
    let target = input
        .command
        .clone()
        .or_else(|| input.file_path.clone())
        .or_else(|| part.state.metadata.filepath.clone())
        .or_else(|| part.state.title.clone())
        .or_else(|| Some(part.tool.clone()).filter(|s| !s.is_empty()));
    (kind, target, success, err_msg)
}

fn load_user_prompts(
    db: &rusqlite::Connection,
    source_file: &Path,
    from_offset: i64,
    scrubber: &SecretScrubber,
) -> Result<Vec<ToolEvent>, AdapterError> {
    let mut stmt = db
        .prepare(
            "SELECT m.id, m.session_id, COALESCE(s.directory, ''), \
                    m.time_created, m.time_updated, m.data \
             FROM message m \
             LEFT JOIN session s ON s.id = m.session_id \
             WHERE m.time_updated > ?1 \
               AND json_extract(m.data, '$.role') = 'user' \
             ORDER BY m.time_updated ASC, m.id ASC",
        )
        .map_err(|e| AdapterError::SqliteOpen {
            path: source_file.to_path_buf(),
            source: e,
        })?;
    let rows = stmt
        .query_map(params![from_offset], |row| {
            Ok(MessageRow {
                id: row.get(0)?,
                session_id: row.get(1)?,
                directory: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                time_created: row.get(3)?,
                time_updated: row.get(4)?,
                data: row.get(5)?,
            })
        })
        .map_err(|e| AdapterError::SqliteOpen {
            path: source_file.to_path_buf(),
            source: e,
        })?;

    let mut out = Vec::new();
    for row in rows {
        let row = row.map_err(|e| AdapterError::SqliteOpen {
            path: source_file.to_path_buf(),
            source: e,
        })?;
        let msg: MessageData = match serde_json::from_str(&row.data) {
            Ok(m) => m,
            Err(_) => continue,
        };
        // Walk parts to collect text content.
        let text = collect_message_text(db, &row.id, scrubber);
        if text.trim().is_empty() {
            continue;
        }
        let when_ms = if msg.time.created > 0 {
            Some(msg.time.created)
        } else {
            row.time_created
        };
        out.push(ToolEvent {
            source_event_id: format!("opencode:message:{}", row.id),
            source_file: source_file.to_path_buf(),
            session_id: row.session_id.clone(),
            tool: ToolKind::OpenCode,
            kind: ActionKind::Other,
            target: Some(truncate_str(&text, 200).to_string()),
            input: Some(scrubber.scrub(&text)),
            output: None,
            status: ActionStatus::Success,
            error_message: None,
            started_at_ms: when_ms,
            duration_ms: None,
            git_root: if msg.path.cwd.is_empty() && row.directory.is_empty() {
                None
            } else if !msg.path.cwd.is_empty() {
                Some(PathBuf::from(&msg.path.cwd))
            } else {
                Some(PathBuf::from(&row.directory))
            },
            metadata: serde_json::json!({ "kind": "user_prompt" }),
        });
    }
    Ok(out)
}

fn collect_message_text(
    db: &rusqlite::Connection,
    message_id: &str,
    scrubber: &SecretScrubber,
) -> String {
    let Ok(mut stmt) =
        db.prepare("SELECT data FROM part WHERE message_id = ?1 ORDER BY time_created ASC, id ASC")
    else {
        return String::new();
    };
    let Ok(rows) = stmt.query_map([message_id], |row| row.get::<_, String>(0)) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for row in rows.flatten() {
        if let Ok(text_part) = serde_json::from_str::<TextPartData>(&row) {
            if text_part.r#type == "text" && !text_part.text.trim().is_empty() {
                parts.push(text_part.text);
            }
        }
    }
    scrubber.scrub(&parts.join("\n"))
}

fn load_token_events(
    db: &rusqlite::Connection,
    source_file: &Path,
    from_offset: i64,
) -> Result<Vec<TokenEvent>, AdapterError> {
    let mut stmt = db
        .prepare(
            "SELECT m.id, m.session_id, COALESCE(s.directory, ''), \
                    m.time_created, m.time_updated, m.data \
             FROM message m \
             LEFT JOIN session s ON s.id = m.session_id \
             WHERE m.time_updated > ?1 \
               AND json_extract(m.data, '$.role') = 'assistant' \
             ORDER BY m.time_updated ASC, m.id ASC",
        )
        .map_err(|e| AdapterError::SqliteOpen {
            path: source_file.to_path_buf(),
            source: e,
        })?;
    let rows = stmt
        .query_map(params![from_offset], |row| {
            Ok(MessageRow {
                id: row.get(0)?,
                session_id: row.get(1)?,
                directory: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                time_created: row.get(3)?,
                time_updated: row.get(4)?,
                data: row.get(5)?,
            })
        })
        .map_err(|e| AdapterError::SqliteOpen {
            path: source_file.to_path_buf(),
            source: e,
        })?;
    let mut out = Vec::new();
    for row in rows {
        let row = row.map_err(|e| AdapterError::SqliteOpen {
            path: source_file.to_path_buf(),
            source: e,
        })?;
        let msg: MessageData = match serde_json::from_str(&row.data) {
            Ok(m) => m,
            Err(_) => continue,
        };
        // Skip rows with no token data (in-progress turn).
        if msg.tokens.input == 0
            && msg.tokens.output == 0
            && msg.tokens.cache.read == 0
            && msg.tokens.cache.write == 0
            && msg.tokens.reasoning == 0
        {
            continue;
        }
        let when_ms = if msg.time.completed > 0 {
            Some(msg.time.completed)
        } else {
            Some(row.time_updated)
        };
        out.push(TokenEvent {
            source_event_id: format!("opencode:tokens:{}", row.id),
            session_id: row.session_id.clone(),
            tool: ToolKind::OpenCode,
            model: if !msg.model_id.is_empty() {
                Some(msg.model_id.clone())
            } else if !msg.model.model_id.is_empty() {
                Some(msg.model.model_id.clone())
            } else {
                None
            },
            request_id: None,
            input_tokens: Some(msg.tokens.input as u64),
            cache_read_tokens: Some(msg.tokens.cache.read as u64),
            cache_creation_tokens: Some(msg.tokens.cache.write as u64),
            cache_creation_1h_tokens: None, // OpenCode doesn't distinguish 1h tier.
            output_tokens: Some(msg.tokens.output as u64),
            reasoning_tokens: Some(msg.tokens.reasoning as u64),
            captured_at_ms: when_ms.unwrap_or(0),
            captured_via: CaptureSource::Transcript,
        });
    }
    Ok(out)
}

fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

// ---------------------------------------------------------------------------
// Row + JSON types
// ---------------------------------------------------------------------------

struct PartRow {
    id: String,
    #[allow(dead_code)]
    message_id: String,
    session_id: String,
    directory: String,
    time_created: Option<i64>,
    #[allow(dead_code)]
    time_updated: i64,
    data: String,
    message: String,
}

struct MessageRow {
    id: String,
    session_id: String,
    directory: String,
    time_created: Option<i64>,
    #[allow(dead_code)]
    time_updated: i64,
    data: String,
}

#[derive(serde::Deserialize)]
struct MessageData {
    #[serde(default)]
    #[allow(dead_code)]
    role: String,
    #[serde(default, rename = "modelID")]
    model_id: String,
    #[serde(default)]
    model: ModelBlock,
    #[serde(default)]
    path: PathBlock,
    #[serde(default)]
    time: TimeBlock,
    #[serde(default)]
    variant: String,
    #[serde(default)]
    tokens: TokensBlock,
}

#[derive(serde::Deserialize, Default)]
struct ModelBlock {
    #[serde(default, rename = "modelID")]
    model_id: String,
}

#[derive(serde::Deserialize, Default)]
struct PathBlock {
    #[serde(default)]
    cwd: String,
}

#[derive(serde::Deserialize, Default)]
struct TimeBlock {
    #[serde(default)]
    created: i64,
    #[serde(default)]
    completed: i64,
}

#[derive(serde::Deserialize, Default)]
struct TokensBlock {
    #[serde(default)]
    input: i64,
    #[serde(default)]
    output: i64,
    #[serde(default)]
    reasoning: i64,
    #[serde(default)]
    cache: CacheBlock,
}

#[derive(serde::Deserialize, Default)]
struct CacheBlock {
    #[serde(default)]
    read: i64,
    #[serde(default)]
    write: i64,
}

#[derive(serde::Deserialize)]
struct ToolPartData {
    #[serde(rename = "type")]
    r#type: String,
    tool: String,
    #[allow(dead_code)]
    #[serde(default, rename = "callID")]
    call_id: String,
    state: ToolState,
}

#[derive(serde::Deserialize)]
struct ToolState {
    #[serde(default)]
    status: String,
    #[serde(default)]
    input: Option<serde_json::Value>,
    #[serde(default)]
    output: String,
    #[serde(default)]
    metadata: ToolMetadata,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    time: ToolTime,
}

#[derive(serde::Deserialize, Default)]
struct ToolMetadata {
    #[serde(default)]
    output: String,
    #[serde(default)]
    exit: i64,
    #[serde(default)]
    filepath: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct ToolTime {
    #[serde(default)]
    start: i64,
    #[serde(default)]
    end: i64,
}

#[derive(serde::Deserialize, Default)]
struct ToolInput {
    #[serde(default)]
    command: Option<String>,
    #[serde(default, rename = "filePath")]
    file_path: Option<String>,
}

#[derive(serde::Deserialize)]
struct TextPartData {
    #[serde(rename = "type")]
    r#type: String,
    text: String,
}
