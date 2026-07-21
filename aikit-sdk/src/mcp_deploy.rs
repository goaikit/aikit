//! Merge MCP server definitions into agent-specific config files (JSON or TOML).
//!
//! **Windows:** VS Code user MCP uses `%APPDATA%\\Code\\User\\mcp.json`, or
//! `<userprofile>\\AppData\\Roaming\\Code\\User\\mcp.json` if `APPDATA` is unset.
//! OpenCode user config uses [`dirs::config_dir`] when available, else the same Roaming fallback.
//!
//! **Tests:** When this crate is built with `cfg(test)`, `AIKIT_MCP_TEST_HOME` overrides the home
//! directory used for global path resolution (not used in normal library builds).
//!
//! Supported targets (see [`mcp_supported_agents`]):
//! - **cursor**: `.cursor/mcp.json` / `~/.cursor/mcp.json` — `mcpServers`
//! - **claude**: `.mcp.json` / `~/.claude.json` — `mcpServers`
//! - **gemini**: `.gemini/settings.json` / `~/.gemini/settings.json` — `mcpServers` inside settings
//! - **copilot**: `.vscode/mcp.json` / VS Code user `mcp.json` — `servers` (VS Code shape)
//! - **opencode**: `opencode.json` / user config — root `mcp` map
//! - **codex**: `.codex/config.toml` / `~/.codex/config.toml` — `[mcp_servers.NAME]`

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use toml::Value as TomlValue;

/// Where to write the MCP config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpScope {
    /// Repository or project directory (agent-specific relative path).
    Project,
    /// User home (agent-specific path under the home directory).
    Global,
}

/// Stdio or HTTP MCP transport (product-specific JSON/TOML mapping).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpServerTransport {
    /// `command` / `args` / optional `env`.
    Stdio {
        command: String,
        args: Vec<String>,
        env: Option<HashMap<String, String>>,
    },
    /// Remote Streamable HTTP / SSE URL and optional `headers`.
    Http {
        url: String,
        headers: Option<HashMap<String, String>>,
    },
}

/// Target file and agent for [`add_mcp_server`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddMcpServerOptions {
    pub agent_key: String,
    pub scope: McpScope,
    /// Used when `scope` is [`McpScope::Project`]; ignored for [`McpScope::Global`] except path resolution.
    pub project_root: PathBuf,
    pub server_name: String,
    pub transport: McpServerTransport,
    /// Replace an existing server entry with the same name.
    pub overwrite: bool,
}

/// Agent keys that support MCP config deployment through this module.
pub const MCP_SUPPORTED_AGENT_KEYS: &[&str] =
    &["cursor", "claude", "gemini", "copilot", "opencode", "codex"];

/// Human-oriented row for [`mcp_supported_agents`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpAgentSupportRow {
    pub agent_key: &'static str,
    pub display_name: &'static str,
    pub project_config_path: &'static str,
    pub global_config_path: &'static str,
}

/// Returns supported agents and the config paths (relative to project root or home).
pub fn mcp_supported_agents() -> Vec<McpAgentSupportRow> {
    vec![
        McpAgentSupportRow {
            agent_key: "cursor",
            display_name: "Cursor",
            project_config_path: ".cursor/mcp.json",
            global_config_path: "~/.cursor/mcp.json",
        },
        McpAgentSupportRow {
            agent_key: "claude",
            display_name: "Claude Code",
            project_config_path: ".mcp.json",
            global_config_path: "~/.claude.json",
        },
        McpAgentSupportRow {
            agent_key: "gemini",
            display_name: "Gemini CLI",
            project_config_path: ".gemini/settings.json",
            global_config_path: "~/.gemini/settings.json",
        },
        McpAgentSupportRow {
            agent_key: "copilot",
            display_name: "VS Code / Copilot MCP",
            project_config_path: ".vscode/mcp.json",
            global_config_path: "Code/User/mcp.json under OS app config (see docs)",
        },
        McpAgentSupportRow {
            agent_key: "opencode",
            display_name: "OpenCode",
            project_config_path: "opencode.json",
            global_config_path: "~/.config/opencode/opencode.json (XDG)",
        },
        McpAgentSupportRow {
            agent_key: "codex",
            display_name: "Codex CLI",
            project_config_path: ".codex/config.toml",
            global_config_path: "~/.codex/config.toml",
        },
    ]
}

/// Normalize CLI aliases to catalog keys.
///
/// ADR 0015: `cursor` is now the single canonical catalog key (no more
/// `cursor` → `cursor-agent` translation); `vscode` remains a genuine alias
/// for `copilot`'s MCP target.
pub fn normalize_mcp_agent_key(key: &str) -> &str {
    match key.trim() {
        "vscode" => "copilot",
        other => other,
    }
}

#[derive(Debug)]
pub enum McpDeployError {
    UnknownAgent(String),
    UnsupportedAgent { agent: String, detail: String },
    MissingHome,
    Io(io::Error),
    Json(serde_json::Error),
    TomlParse(String),
    TomlSerialize(String),
    AlreadyExists { name: String },
    InvalidEnvPair(String),
    InvalidConfig(String),
}

impl fmt::Display for McpDeployError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            McpDeployError::UnknownAgent(k) => write!(f, "unknown agent key: {}", k),
            McpDeployError::UnsupportedAgent { agent, detail } => {
                write!(
                    f,
                    "agent '{}' does not support MCP deploy via aikit: {}",
                    agent, detail
                )
            }
            McpDeployError::MissingHome => write!(f, "could not resolve user home directory"),
            McpDeployError::Io(e) => write!(f, "{}", e),
            McpDeployError::Json(e) => write!(f, "{}", e),
            McpDeployError::TomlParse(s) => write!(f, "{}", s),
            McpDeployError::TomlSerialize(s) => write!(f, "{}", s),
            McpDeployError::AlreadyExists { name } => write!(
                f,
                "MCP server '{}' already exists (pass overwrite=true to replace)",
                name
            ),
            McpDeployError::InvalidEnvPair(s) => {
                write!(f, "invalid --env value (expected KEY=value): {}", s)
            }
            McpDeployError::InvalidConfig(s) => write!(f, "{}", s),
        }
    }
}

impl Error for McpDeployError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            McpDeployError::Io(e) => Some(e),
            McpDeployError::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for McpDeployError {
    fn from(e: io::Error) -> Self {
        McpDeployError::Io(e)
    }
}

impl From<serde_json::Error> for McpDeployError {
    fn from(e: serde_json::Error) -> Self {
        McpDeployError::Json(e)
    }
}

fn home_dir_for_mcp() -> Result<PathBuf, McpDeployError> {
    #[cfg(test)]
    if let Ok(h) = std::env::var("AIKIT_MCP_TEST_HOME") {
        if !h.is_empty() {
            return Ok(PathBuf::from(h));
        }
    }
    dirs::home_dir().ok_or(McpDeployError::MissingHome)
}

/// VS Code stable user `mcp.json` (not Cursor). Windows: `%APPDATA%\Code\User\mcp.json`, or
/// `<home>\AppData\Roaming\...` when `APPDATA` is unset (e.g. minimal CI).
fn vscode_user_mcp_path(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Code/User/mcp.json")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Roaming"))
            .join("Code")
            .join("User")
            .join("mcp.json")
    } else {
        home.join(".config/Code/User/mcp.json")
    }
}

/// OpenCode user `opencode.json`. Uses [`dirs::config_dir`] when set; otherwise Roaming (Windows)
/// or `~/.config` (Unix).
fn opencode_user_config_path(home: &Path) -> PathBuf {
    dirs::config_dir()
        .map(|p| p.join("opencode").join("opencode.json"))
        .unwrap_or_else(|| {
            if cfg!(target_os = "windows") {
                std::env::var_os("APPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join("AppData").join("Roaming"))
                    .join("opencode")
                    .join("opencode.json")
            } else {
                home.join(".config").join("opencode").join("opencode.json")
            }
        })
}

/// Resolves the config file path for MCP merge for a supported agent.
pub fn mcp_config_path(
    agent_key: &str,
    scope: McpScope,
    project_root: &Path,
) -> Result<PathBuf, McpDeployError> {
    let key = normalize_mcp_agent_key(agent_key);
    crate::validate_agent_key(key)
        .map_err(|_| McpDeployError::UnknownAgent(agent_key.to_string()))?;

    if !MCP_SUPPORTED_AGENT_KEYS.contains(&key) {
        let supported = MCP_SUPPORTED_AGENT_KEYS.join(", ");
        return Err(McpDeployError::UnsupportedAgent {
            agent: agent_key.to_string(),
            detail: format!("only [{}] are implemented", supported),
        });
    }

    let home = home_dir_for_mcp()?;
    let path = match (key, scope) {
        ("cursor", McpScope::Project) => project_root.join(".cursor/mcp.json"),
        ("cursor", McpScope::Global) => home.join(".cursor/mcp.json"),
        ("claude", McpScope::Project) => project_root.join(".mcp.json"),
        ("claude", McpScope::Global) => home.join(".claude.json"),
        ("gemini", McpScope::Project) => project_root.join(".gemini/settings.json"),
        ("gemini", McpScope::Global) => home.join(".gemini/settings.json"),
        ("copilot", McpScope::Project) => project_root.join(".vscode/mcp.json"),
        ("copilot", McpScope::Global) => vscode_user_mcp_path(&home),
        ("opencode", McpScope::Project) => project_root.join("opencode.json"),
        ("opencode", McpScope::Global) => opencode_user_config_path(&home),
        ("codex", McpScope::Project) => project_root.join(".codex/config.toml"),
        ("codex", McpScope::Global) => home.join(".codex/config.toml"),
        _ => {
            return Err(McpDeployError::UnsupportedAgent {
                agent: agent_key.to_string(),
                detail: "internal mapping error".to_string(),
            })
        }
    };
    Ok(path)
}

/// Cursor / Claude / Gemini `mcpServers` entry (no `type` field).
fn transport_to_mcp_servers_json(t: &McpServerTransport) -> Value {
    match t {
        McpServerTransport::Stdio { command, args, env } => {
            let mut m = Map::new();
            m.insert("command".to_string(), json!(command));
            m.insert("args".to_string(), json!(args));
            if let Some(e) = env {
                let env_obj: Map<String, Value> =
                    e.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
                m.insert("env".to_string(), Value::Object(env_obj));
            }
            Value::Object(m)
        }
        McpServerTransport::Http { url, headers } => {
            let mut m = Map::new();
            m.insert("url".to_string(), json!(url));
            if let Some(h) = headers {
                let h_obj: Map<String, Value> =
                    h.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
                m.insert("headers".to_string(), Value::Object(h_obj));
            }
            Value::Object(m)
        }
    }
}

/// VS Code `.vscode/mcp.json` server entry ([docs](https://code.visualstudio.com/docs/copilot/reference/mcp-configuration)).
fn transport_to_vscode_server_json(t: &McpServerTransport) -> Value {
    match t {
        McpServerTransport::Stdio { command, args, env } => {
            let mut m = Map::new();
            m.insert("type".to_string(), json!("stdio"));
            m.insert("command".to_string(), json!(command));
            m.insert("args".to_string(), json!(args));
            if let Some(e) = env {
                let env_obj: Map<String, Value> =
                    e.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
                m.insert("env".to_string(), Value::Object(env_obj));
            }
            Value::Object(m)
        }
        McpServerTransport::Http { url, headers } => {
            let mut m = Map::new();
            m.insert("type".to_string(), json!("http"));
            m.insert("url".to_string(), json!(url));
            if let Some(h) = headers {
                let h_obj: Map<String, Value> =
                    h.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
                m.insert("headers".to_string(), Value::Object(h_obj));
            }
            Value::Object(m)
        }
    }
}

/// OpenCode `mcp.<name>` entry ([docs](https://dev.opencode.ai/docs/mcp-servers)).
fn transport_to_opencode_server_json(t: &McpServerTransport) -> Value {
    match t {
        McpServerTransport::Stdio { command, args, env } => {
            let mut cmd: Vec<Value> = vec![json!(command)];
            cmd.extend(args.iter().map(|s| json!(s)));
            let mut m = Map::new();
            m.insert("type".to_string(), json!("local"));
            m.insert("command".to_string(), Value::Array(cmd));
            m.insert("enabled".to_string(), json!(true));
            if let Some(e) = env {
                let env_obj: Map<String, Value> =
                    e.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
                m.insert("environment".to_string(), Value::Object(env_obj));
            }
            Value::Object(m)
        }
        McpServerTransport::Http { url, headers } => {
            let mut m = Map::new();
            m.insert("type".to_string(), json!("remote"));
            m.insert("url".to_string(), json!(url));
            m.insert("enabled".to_string(), json!(true));
            if let Some(h) = headers {
                let h_obj: Map<String, Value> =
                    h.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
                m.insert("headers".to_string(), Value::Object(h_obj));
            }
            Value::Object(m)
        }
    }
}

fn codex_server_table(t: &McpServerTransport) -> toml::map::Map<String, TomlValue> {
    let mut tbl = toml::map::Map::new();
    match t {
        McpServerTransport::Stdio { command, args, env } => {
            tbl.insert("command".to_string(), TomlValue::String(command.clone()));
            if !args.is_empty() {
                tbl.insert(
                    "args".to_string(),
                    TomlValue::Array(args.iter().cloned().map(TomlValue::String).collect()),
                );
            }
            if let Some(e) = env {
                let mut et = toml::map::Map::new();
                for (k, v) in e {
                    et.insert(k.clone(), TomlValue::String(v.clone()));
                }
                tbl.insert("env".to_string(), TomlValue::Table(et));
            }
        }
        McpServerTransport::Http { url, headers } => {
            tbl.insert("url".to_string(), TomlValue::String(url.clone()));
            if let Some(h) = headers {
                let mut ht = toml::map::Map::new();
                for (k, v) in h {
                    ht.insert(k.clone(), TomlValue::String(v.clone()));
                }
                tbl.insert("http_headers".to_string(), TomlValue::Table(ht));
            }
        }
    }
    tbl
}

fn read_or_empty_json_object(path: &Path) -> Result<Value, McpDeployError> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let raw = fs::read_to_string(path)?;
    let v: Value = serde_json::from_str(&raw)?;
    if !v.is_object() {
        return Err(McpDeployError::InvalidConfig(
            "config file must contain a JSON object at the root".to_string(),
        ));
    }
    Ok(v)
}

fn merge_json_bucket(
    mut root: Value,
    bucket_key: &str,
    server_name: &str,
    server_json: Value,
    overwrite: bool,
) -> Result<Value, McpDeployError> {
    let obj = root
        .as_object_mut()
        .ok_or_else(|| McpDeployError::InvalidConfig("root must be a JSON object".to_string()))?;

    let servers = obj
        .entry(bucket_key.to_string())
        .or_insert_with(|| json!({}));
    let map = servers.as_object_mut().ok_or_else(|| {
        McpDeployError::InvalidConfig(format!("'{}' must be a JSON object", bucket_key))
    })?;

    if map.contains_key(server_name) && !overwrite {
        return Err(McpDeployError::AlreadyExists {
            name: server_name.to_string(),
        });
    }
    map.insert(server_name.to_string(), server_json);
    Ok(root)
}

fn merge_opencode_mcp(
    mut root: Value,
    server_name: &str,
    server_json: Value,
    overwrite: bool,
) -> Result<Value, McpDeployError> {
    let obj = root
        .as_object_mut()
        .ok_or_else(|| McpDeployError::InvalidConfig("root must be a JSON object".to_string()))?;

    let mcp = obj.entry("mcp".to_string()).or_insert_with(|| json!({}));
    let map = mcp
        .as_object_mut()
        .ok_or_else(|| McpDeployError::InvalidConfig("'mcp' must be a JSON object".to_string()))?;

    if map.contains_key(server_name) && !overwrite {
        return Err(McpDeployError::AlreadyExists {
            name: server_name.to_string(),
        });
    }
    map.insert(server_name.to_string(), server_json);
    Ok(root)
}

fn read_or_empty_toml_root(
    path: &Path,
) -> Result<toml::map::Map<String, TomlValue>, McpDeployError> {
    if !path.exists() {
        return Ok(toml::map::Map::new());
    }
    let raw = fs::read_to_string(path)?;
    let v: TomlValue =
        toml::from_str(&raw).map_err(|e| McpDeployError::TomlParse(e.to_string()))?;
    match v {
        TomlValue::Table(t) => Ok(t),
        _ => Err(McpDeployError::InvalidConfig(
            "config.toml root must be a TOML table".to_string(),
        )),
    }
}

fn merge_codex_mcp_servers(
    mut root: toml::map::Map<String, TomlValue>,
    server_name: &str,
    server_tbl: toml::map::Map<String, TomlValue>,
    overwrite: bool,
) -> Result<toml::map::Map<String, TomlValue>, McpDeployError> {
    let servers_val = root
        .entry("mcp_servers".to_string())
        .or_insert_with(|| TomlValue::Table(toml::map::Map::new()));

    let servers_tbl = servers_val.as_table_mut().ok_or_else(|| {
        McpDeployError::InvalidConfig("'mcp_servers' must be a TOML table".to_string())
    })?;

    if servers_tbl.contains_key(server_name) && !overwrite {
        return Err(McpDeployError::AlreadyExists {
            name: server_name.to_string(),
        });
    }
    servers_tbl.insert(server_name.to_string(), TomlValue::Table(server_tbl));
    Ok(root)
}

/// Parses `KEY=value` into a map entry (first `=` separates key and value).
pub fn parse_env_pairs(pairs: &[String]) -> Result<HashMap<String, String>, McpDeployError> {
    let mut m = HashMap::new();
    for p in pairs {
        let (k, v) = p
            .split_once('=')
            .ok_or_else(|| McpDeployError::InvalidEnvPair(p.clone()))?;
        if k.is_empty() {
            return Err(McpDeployError::InvalidEnvPair(p.clone()));
        }
        m.insert(k.to_string(), v.to_string());
    }
    Ok(m)
}

/// Parses `KEY=value` for HTTP headers.
pub fn parse_header_pairs(pairs: &[String]) -> Result<HashMap<String, String>, McpDeployError> {
    parse_env_pairs(pairs)
}

/// Reads (or creates) the agent config file, merges the server entry, writes back.
///
/// Returns the path written.
pub fn add_mcp_server(opts: AddMcpServerOptions) -> Result<PathBuf, McpDeployError> {
    let key = normalize_mcp_agent_key(&opts.agent_key);
    let path = mcp_config_path(key, opts.scope, &opts.project_root)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    match key {
        "codex" => {
            let mut root = read_or_empty_toml_root(&path)?;
            let server_tbl = codex_server_table(&opts.transport);
            root = merge_codex_mcp_servers(root, &opts.server_name, server_tbl, opts.overwrite)?;
            let out = TomlValue::Table(root);
            let s = toml::to_string_pretty(&out)
                .map_err(|e| McpDeployError::TomlSerialize(e.to_string()))?;
            fs::write(&path, s)?;
        }
        "opencode" => {
            let existing = read_or_empty_json_object(&path)?;
            let server_json = transport_to_opencode_server_json(&opts.transport);
            let merged =
                merge_opencode_mcp(existing, &opts.server_name, server_json, opts.overwrite)?;
            let out = serde_json::to_string_pretty(&merged)?;
            fs::write(&path, out)?;
        }
        "copilot" => {
            let existing = read_or_empty_json_object(&path)?;
            let server_json = transport_to_vscode_server_json(&opts.transport);
            let merged = merge_json_bucket(
                existing,
                "servers",
                &opts.server_name,
                server_json,
                opts.overwrite,
            )?;
            let out = serde_json::to_string_pretty(&merged)?;
            fs::write(&path, out)?;
        }
        "cursor" | "claude" | "gemini" => {
            let existing = read_or_empty_json_object(&path)?;
            let server_json = transport_to_mcp_servers_json(&opts.transport);
            let merged = merge_json_bucket(
                existing,
                "mcpServers",
                &opts.server_name,
                server_json,
                opts.overwrite,
            )?;
            let out = serde_json::to_string_pretty(&merged)?;
            fs::write(&path, out)?;
        }
        _ => {
            return Err(McpDeployError::UnsupportedAgent {
                agent: opts.agent_key.clone(),
                detail: "internal dispatch error".to_string(),
            });
        }
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;

    fn mcp_env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn merge_new_file_cursor_project() {
        let tmp = TempDir::new().unwrap();
        let path = add_mcp_server(AddMcpServerOptions {
            agent_key: "cursor".to_string(),
            scope: McpScope::Project,
            project_root: tmp.path().to_path_buf(),
            server_name: "demo".to_string(),
            transport: McpServerTransport::Http {
                url: "http://127.0.0.1:8730/mcp".to_string(),
                headers: None,
            },
            overwrite: false,
        })
        .unwrap();
        assert_eq!(path, tmp.path().join(".cursor/mcp.json"));
        let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            v["mcpServers"]["demo"]["url"],
            json!("http://127.0.0.1:8730/mcp")
        );
    }

    #[test]
    fn merge_preserves_other_servers_and_keys() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join(".cursor/mcp.json");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(
            &p,
            r#"{"mcpServers":{"old":{"url":"http://old/mcp"}},"x":1}"#,
        )
        .unwrap();
        add_mcp_server(AddMcpServerOptions {
            agent_key: "cursor".to_string(),
            scope: McpScope::Project,
            project_root: tmp.path().to_path_buf(),
            server_name: "new".to_string(),
            transport: McpServerTransport::Stdio {
                command: "npx".to_string(),
                args: vec!["-y".into(), "pkg".into()],
                env: None,
            },
            overwrite: false,
        })
        .unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["x"], json!(1));
        assert_eq!(v["mcpServers"]["old"]["url"], json!("http://old/mcp"));
        assert_eq!(v["mcpServers"]["new"]["command"], json!("npx"));
    }

    #[test]
    fn duplicate_errors_without_overwrite() {
        let tmp = TempDir::new().unwrap();
        add_mcp_server(AddMcpServerOptions {
            agent_key: "claude".to_string(),
            scope: McpScope::Project,
            project_root: tmp.path().to_path_buf(),
            server_name: "s".to_string(),
            transport: McpServerTransport::Http {
                url: "http://a".into(),
                headers: None,
            },
            overwrite: false,
        })
        .unwrap();
        let e = add_mcp_server(AddMcpServerOptions {
            agent_key: "claude".to_string(),
            scope: McpScope::Project,
            project_root: tmp.path().to_path_buf(),
            server_name: "s".to_string(),
            transport: McpServerTransport::Http {
                url: "http://b".into(),
                headers: None,
            },
            overwrite: false,
        })
        .unwrap_err();
        assert!(matches!(e, McpDeployError::AlreadyExists { .. }));
    }

    #[test]
    fn claude_global_merges_top_level_mcp_servers() {
        let _lock = mcp_env_lock();
        let tmp = TempDir::new().unwrap();
        let prev = std::env::var("AIKIT_MCP_TEST_HOME");
        std::env::set_var("AIKIT_MCP_TEST_HOME", tmp.path());
        let fake_claude = tmp.path().join(".claude.json");
        fs::write(
            &fake_claude,
            r#"{"mcpServers":{"keep":{"command":"c"}},"session":"x"}"#,
        )
        .unwrap();
        let path = add_mcp_server(AddMcpServerOptions {
            agent_key: "claude".to_string(),
            scope: McpScope::Global,
            project_root: PathBuf::from("C:\\unused"),
            server_name: "added".to_string(),
            transport: McpServerTransport::Http {
                url: "http://h/mcp".into(),
                headers: None,
            },
            overwrite: false,
        })
        .unwrap();
        assert_eq!(path, fake_claude);
        let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["session"], json!("x"));
        assert_eq!(v["mcpServers"]["keep"]["command"], json!("c"));
        assert_eq!(v["mcpServers"]["added"]["url"], json!("http://h/mcp"));
        match prev {
            Ok(h) => std::env::set_var("AIKIT_MCP_TEST_HOME", h),
            Err(_) => std::env::remove_var("AIKIT_MCP_TEST_HOME"),
        }
    }

    #[test]
    fn copilot_global_path_under_fake_home() {
        let _lock = mcp_env_lock();
        let tmp = TempDir::new().unwrap();
        let prev_home = std::env::var("AIKIT_MCP_TEST_HOME");
        std::env::set_var("AIKIT_MCP_TEST_HOME", tmp.path());
        #[cfg(windows)]
        let prev_appdata = std::env::var("APPDATA").ok();
        #[cfg(windows)]
        std::env::remove_var("APPDATA");

        let p = mcp_config_path("copilot", McpScope::Global, tmp.path()).unwrap();
        let expected = match std::env::consts::OS {
            "windows" => tmp
                .path()
                .join("AppData")
                .join("Roaming")
                .join("Code")
                .join("User")
                .join("mcp.json"),
            "macos" => tmp
                .path()
                .join("Library/Application Support/Code/User/mcp.json"),
            _ => tmp
                .path()
                .join(".config")
                .join("Code")
                .join("User")
                .join("mcp.json"),
        };
        assert_eq!(p, expected);

        #[cfg(windows)]
        match prev_appdata {
            Some(ref v) => std::env::set_var("APPDATA", v),
            None => std::env::remove_var("APPDATA"),
        }
        match prev_home {
            Ok(h) => std::env::set_var("AIKIT_MCP_TEST_HOME", h),
            Err(_) => std::env::remove_var("AIKIT_MCP_TEST_HOME"),
        }
    }

    #[test]
    fn gemini_merges_into_settings_json() {
        let tmp = TempDir::new().unwrap();
        let settings = tmp.path().join(".gemini/settings.json");
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        fs::write(
            &settings,
            r#"{"editor":"x","mcpServers":{"a":{"command":"old"}}}"#,
        )
        .unwrap();
        add_mcp_server(AddMcpServerOptions {
            agent_key: "gemini".to_string(),
            scope: McpScope::Project,
            project_root: tmp.path().to_path_buf(),
            server_name: "b".to_string(),
            transport: McpServerTransport::Http {
                url: "http://g/mcp".into(),
                headers: None,
            },
            overwrite: false,
        })
        .unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(v["editor"], json!("x"));
        assert_eq!(v["mcpServers"]["a"]["command"], json!("old"));
        assert_eq!(v["mcpServers"]["b"]["url"], json!("http://g/mcp"));
    }

    #[test]
    fn copilot_writes_vscode_servers_shape() {
        let tmp = TempDir::new().unwrap();
        add_mcp_server(AddMcpServerOptions {
            agent_key: "copilot".to_string(),
            scope: McpScope::Project,
            project_root: tmp.path().to_path_buf(),
            server_name: "ctx".to_string(),
            transport: McpServerTransport::Http {
                url: "https://mcp.example/mcp".into(),
                headers: None,
            },
            overwrite: false,
        })
        .unwrap();
        let p = tmp.path().join(".vscode/mcp.json");
        let v: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["servers"]["ctx"]["type"], json!("http"));
        assert_eq!(v["servers"]["ctx"]["url"], json!("https://mcp.example/mcp"));
    }

    #[test]
    fn opencode_merges_under_mcp() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("opencode.json"),
            r#"{"mcp":{"old":{"type":"remote","url":"http://x","enabled":true}},"x":1}"#,
        )
        .unwrap();
        add_mcp_server(AddMcpServerOptions {
            agent_key: "opencode".to_string(),
            scope: McpScope::Project,
            project_root: tmp.path().to_path_buf(),
            server_name: "new".to_string(),
            transport: McpServerTransport::Stdio {
                command: "npx".to_string(),
                args: vec!["-y".into(), "pkg".into()],
                env: None,
            },
            overwrite: false,
        })
        .unwrap();
        let v: Value =
            serde_json::from_str(&fs::read_to_string(tmp.path().join("opencode.json")).unwrap())
                .unwrap();
        assert_eq!(v["x"], json!(1));
        assert_eq!(v["mcp"]["new"]["type"], json!("local"));
        assert_eq!(v["mcp"]["new"]["command"][0], json!("npx"));
    }

    #[test]
    fn codex_writes_mcp_servers_toml() {
        let tmp = TempDir::new().unwrap();
        let cfg = tmp.path().join(".codex/config.toml");
        fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        fs::write(&cfg, "[experimental]\nfoo = 1\n").unwrap();
        add_mcp_server(AddMcpServerOptions {
            agent_key: "codex".to_string(),
            scope: McpScope::Project,
            project_root: tmp.path().to_path_buf(),
            server_name: "ctx7".to_string(),
            transport: McpServerTransport::Stdio {
                command: "npx".to_string(),
                args: vec!["-y".into(), "@x/y".into()],
                env: None,
            },
            overwrite: false,
        })
        .unwrap();
        let raw = fs::read_to_string(&cfg).unwrap();
        assert!(raw.contains("[mcp_servers.ctx7]"));
        assert!(raw.contains("command = \"npx\""));
        let v: TomlValue = toml::from_str(&raw).unwrap();
        let root = v.as_table().unwrap();
        assert_eq!(root["experimental"]["foo"], TomlValue::Integer(1));
        let ms = root["mcp_servers"].as_table().unwrap();
        assert_eq!(ms["ctx7"]["command"], TomlValue::String("npx".into()));
    }

    #[test]
    fn normalize_agent_aliases() {
        assert_eq!(normalize_mcp_agent_key("cursor"), "cursor");
        assert_eq!(normalize_mcp_agent_key("vscode"), "copilot");
        assert_eq!(normalize_mcp_agent_key("claude"), "claude");
    }

    #[test]
    fn unknown_agent_key_errors() {
        let tmp = TempDir::new().unwrap();
        let e = mcp_config_path("not-an-agent", McpScope::Project, tmp.path()).unwrap_err();
        assert!(matches!(e, McpDeployError::UnknownAgent(_)));
    }

    #[test]
    fn unsupported_catalog_agent_errors() {
        let tmp = TempDir::new().unwrap();
        let e = mcp_config_path("qwen", McpScope::Project, tmp.path()).unwrap_err();
        assert!(matches!(e, McpDeployError::UnsupportedAgent { .. }));
    }
}
