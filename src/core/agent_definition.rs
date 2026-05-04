//! Session / persisted agent definitions aligned with RFC goaikit/aikit#6.
//!
//! Parses Markdown + YAML front-matter (Claude-style body as system prompt) and
//! ephemeral JSON maps (same fields as Claude `--agents`). VS CodeCopilot extras
//! `user-invocable`, `disable-model-invocation`, and `argument-hint` are not
//! read or surfaced; YAML may still contain them and parsing succeeds.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Which subagents a coordinator may delegate to (VS Code **`agents`** field subset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DelegationAllowlist {
    /// `*` — any available delegate
    All,
    /// `[]` — no delegation from this definition
    None,
    /// Named agents only.
    Names(Vec<String>),
}

impl serde::Serialize for DelegationAllowlist {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        match self {
            DelegationAllowlist::All => s.serialize_str("*"),
            DelegationAllowlist::None => {
                let seq = s.serialize_seq(Some(0))?;
                seq.end()
            }
            DelegationAllowlist::Names(v) => v.serialize(s),
        }
    }
}

/// Normalized definition after parsing and validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub tools: Option<Vec<String>>,
    pub disallowed_tools: Option<Vec<String>>,
    pub model: Option<String>,
    pub delegation: Option<DelegationAllowlist>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    MissingField { field: &'static str },
    InvalidJson(String),
    InvalidYaml(String),
    InvalidMarkdown(String),
    EmptyDocument,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::MissingField { field } => write!(f, "missing required field `{}`", field),
            ParseError::InvalidJson(s) => write!(f, "invalid JSON: {}", s),
            ParseError::InvalidYaml(s) => write!(f, "invalid YAML: {}", s),
            ParseError::InvalidMarkdown(s) => write!(f, "{}", s),
            ParseError::EmptyDocument => write!(f, "empty document"),
        }
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawJsonAgent {
    name: Option<String>,
    description: Option<String>,
    prompt: Option<String>,
    #[serde(default)]
    tools: Option<JsonValue>,
    #[serde(default, alias = "disallowed_tools")]
    disallowed_tools: Option<JsonValue>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    agents: Option<JsonValue>,
}

fn normalize_tool_list(v: Option<JsonValue>) -> Result<Option<Vec<String>>, ParseError> {
    match v {
        None => Ok(None),
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::Array(a)) => {
            let mut out = Vec::with_capacity(a.len());
            for item in a {
                match item {
                    JsonValue::String(s) if !s.is_empty() => out.push(s),
                    JsonValue::String(_) => {}
                    _ => {
                        return Err(ParseError::InvalidJson(
                            "tools entries must be strings".into(),
                        ))
                    }
                }
            }
            Ok(Some(out))
        }
        Some(JsonValue::String(s)) => Ok(Some(split_comma_tools(&s))),
        _ => Err(ParseError::InvalidJson(
            "tools must be a JSON array or string".into(),
        )),
    }
}

fn split_comma_tools(s: &str) -> Vec<String> {
    s.split([',', ';'])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

fn delegation_from_json(v: Option<JsonValue>) -> Result<Option<DelegationAllowlist>, ParseError> {
    match v {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(s)) => {
            if s == "*" {
                Ok(Some(DelegationAllowlist::All))
            } else {
                Err(ParseError::InvalidJson(
                    "agents as string must be '*'".into(),
                ))
            }
        }
        Some(JsonValue::Array(a)) => {
            if a.is_empty() {
                return Ok(Some(DelegationAllowlist::None));
            }
            let mut names = Vec::new();
            for item in a {
                match item {
                    JsonValue::String(s) if !s.is_empty() => names.push(s),
                    JsonValue::String(_) => {}
                    _ => {
                        return Err(ParseError::InvalidJson(
                            "agents list entries must be strings".into(),
                        ))
                    }
                }
            }
            Ok(Some(DelegationAllowlist::Names(names)))
        }
        _ => Err(ParseError::InvalidJson(
            "agents must be '*', [] or a string array".into(),
        )),
    }
}

fn raw_json_to_def(id: &str, raw: RawJsonAgent) -> Result<AgentDefinition, ParseError> {
    let name = raw
        .name
        .unwrap_or_else(|| id.to_string())
        .trim()
        .to_string();
    if name.is_empty() {
        return Err(ParseError::MissingField { field: "name" });
    }
    let description = raw
        .description
        .ok_or(ParseError::MissingField {
            field: "description",
        })?
        .trim()
        .to_string();
    if description.is_empty() {
        return Err(ParseError::MissingField {
            field: "description",
        });
    }
    let prompt = raw.prompt.unwrap_or_default().trim().to_string();

    Ok(AgentDefinition {
        name,
        description,
        prompt,
        tools: normalize_tool_list(raw.tools)?,
        disallowed_tools: normalize_tool_list(raw.disallowed_tools)?,
        model: raw
            .model
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty()),
        delegation: delegation_from_json(raw.agents)?,
    })
}

/// Parse Claude-style `--agents` JSON: `{ "<id>": { ... per-agent fields } }`.
///
/// The returned map is keyed by **outer JSON id** so session merges and overrides behave as in the RFC.
pub fn parse_session_agents_json(s: &str) -> Result<HashMap<String, AgentDefinition>, ParseError> {
    let value: JsonValue =
        serde_json::from_str(s).map_err(|e| ParseError::InvalidJson(e.to_string()))?;
    let map = value.as_object().ok_or_else(|| {
        ParseError::InvalidJson("session-agents JSON must be a single object".into())
    })?;

    let mut out = HashMap::new();
    for (key, agent_val) in map {
        let raw: RawJsonAgent = serde_json::from_value(agent_val.clone())
            .map_err(|e| ParseError::InvalidJson(format!("agent `{}`: {}", key, e)))?;
        let def = raw_json_to_def(key, raw)?;
        out.insert(key.clone(), def);
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
struct YamlFrontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    tools: Option<serde_yaml::Value>,
    #[serde(default, alias = "disallowedTools")]
    disallowed_tools: Option<serde_yaml::Value>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    agents: Option<serde_yaml::Value>,
}

fn tools_from_yaml(v: serde_yaml::Value) -> Result<Vec<String>, ParseError> {
    match v {
        serde_yaml::Value::Sequence(seq) => {
            let mut out = Vec::new();
            for item in seq {
                match item {
                    serde_yaml::Value::String(s) if !s.is_empty() => out.push(s),
                    serde_yaml::Value::Null | serde_yaml::Value::String(_) => {}
                    _ => {
                        return Err(ParseError::InvalidYaml(
                            "tools sequence must contain only strings".into(),
                        ))
                    }
                }
            }
            Ok(out)
        }
        serde_yaml::Value::String(s) => Ok(split_comma_tools(&s)),
        serde_yaml::Value::Bool(_) => Err(ParseError::InvalidYaml("invalid tools value".into())),
        _ => Err(ParseError::InvalidYaml(
            "tools must be a YAML sequence or string".into(),
        )),
    }
}

fn normalize_yaml_tools(v: Option<serde_yaml::Value>) -> Result<Option<Vec<String>>, ParseError> {
    match v {
        None => Ok(None),
        Some(serde_yaml::Value::Null) => Ok(None),
        Some(v) => Ok(Some(tools_from_yaml(v)?)),
    }
}

fn delegation_from_yaml(
    v: Option<serde_yaml::Value>,
) -> Result<Option<DelegationAllowlist>, ParseError> {
    match v {
        None | Some(serde_yaml::Value::Null) => Ok(None),
        Some(serde_yaml::Value::String(s)) => {
            if s == "*" {
                Ok(Some(DelegationAllowlist::All))
            } else {
                Ok(Some(DelegationAllowlist::Names(vec![s])))
            }
        }
        Some(serde_yaml::Value::Sequence(seq)) => {
            if seq.is_empty() {
                return Ok(Some(DelegationAllowlist::None));
            }
            let mut names = Vec::new();
            for item in seq {
                match item {
                    serde_yaml::Value::String(s) if !s.is_empty() => names.push(s),
                    serde_yaml::Value::String(_) => {}
                    _ => {
                        return Err(ParseError::InvalidYaml(
                            "agents sequence must contain only strings".into(),
                        ))
                    }
                }
            }
            Ok(Some(DelegationAllowlist::Names(names)))
        }
        _ => Err(ParseError::InvalidYaml(
            "agents must be '*', a string, or a sequence".into(),
        )),
    }
}

/// Parse Markdown with `---` YAML front-matter; body is the system prompt unless `prompt:` is set in front-matter.
pub fn parse_agent_markdown(content: &str) -> Result<AgentDefinition, ParseError> {
    let content = content.trim_start();
    if content.is_empty() {
        return Err(ParseError::EmptyDocument);
    }

    let (yaml_part, body) = if content.starts_with("---") {
        let after_first = content.strip_prefix("---").unwrap();
        let end = after_first.find("\n---").ok_or_else(|| {
            ParseError::InvalidMarkdown("missing closing --- for YAML frontmatter".into())
        })?;
        let yaml_text = after_first[..end].trim();
        let rest = after_first[end + "\n---".len()..].trim_start();
        (Some(yaml_text), rest)
    } else {
        (None, content)
    };

    let body = body.trim();
    let ym: YamlFrontmatter = match yaml_part {
        None => YamlFrontmatter {
            name: None,
            description: None,
            prompt: None,
            tools: None,
            disallowed_tools: None,
            model: None,
            agents: None,
        },
        Some(txt) => {
            serde_yaml::from_str(txt).map_err(|e| ParseError::InvalidYaml(e.to_string()))?
        }
    };

    let name = ym
        .name
        .clone()
        .ok_or(ParseError::MissingField { field: "name" })?
        .trim()
        .to_string();
    if name.is_empty() {
        return Err(ParseError::MissingField { field: "name" });
    }
    let description = ym
        .description
        .clone()
        .ok_or(ParseError::MissingField {
            field: "description",
        })?
        .trim()
        .to_string();
    if description.is_empty() {
        return Err(ParseError::MissingField {
            field: "description",
        });
    }

    let prompt = ym
        .prompt
        .clone()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| body.to_string());

    if prompt.is_empty() {
        return Err(ParseError::MissingField { field: "prompt" });
    }

    Ok(AgentDefinition {
        name,
        description,
        prompt,
        tools: normalize_yaml_tools(ym.tools)?,
        disallowed_tools: normalize_yaml_tools(ym.disallowed_tools)?,
        model: ym
            .model
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty()),
        delegation: delegation_from_yaml(ym.agents)?,
    })
}

// ──────────────────────────────────────────────────────────────────────────────
// Registry types
// ──────────────────────────────────────────────────────────────────────────────

/// Source label for a persisted or session-injected definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DefinitionSource {
    Builtin,
    Managed,
    User,
    Project,
    Session,
}

impl std::fmt::Display for DefinitionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DefinitionSource::Builtin => write!(f, "builtin"),
            DefinitionSource::Managed => write!(f, "managed"),
            DefinitionSource::User => write!(f, "user"),
            DefinitionSource::Project => write!(f, "project"),
            DefinitionSource::Session => write!(f, "session"),
        }
    }
}

impl DefinitionSource {
    fn priority(&self) -> u8 {
        match self {
            DefinitionSource::Builtin => 0,
            DefinitionSource::Managed => 1,
            DefinitionSource::User => 2,
            DefinitionSource::Project => 3,
            DefinitionSource::Session => 4,
        }
    }
}

/// A definition together with its provenance.
#[derive(Debug, Clone)]
pub struct DefinitionRecord {
    pub definition: AgentDefinition,
    pub source: DefinitionSource,
    /// Absolute path for persisted sources; `None` for session-injected entries.
    pub path: Option<std::path::PathBuf>,
}

/// Ordered, merged registry for one process lifetime.
///
/// Key = outer JSON id (filename stem or JSON map key).
pub struct SessionRegistry {
    entries: std::collections::HashMap<String, DefinitionRecord>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self {
            entries: std::collections::HashMap::new(),
        }
    }

    /// Merge an entry from a higher-priority source; existing keys are overwritten.
    pub fn merge(&mut self, key: String, record: DefinitionRecord) {
        self.entries.insert(key, record);
    }

    /// Resolve by `AgentDefinition.name` (exact, case-sensitive).
    pub fn resolve_by_name(&self, name: &str) -> Option<&DefinitionRecord> {
        self.entries.values().find(|r| r.definition.name == name)
    }

    /// Return all entries sorted by source priority then key.
    pub fn all_sorted(&self) -> Vec<&DefinitionRecord> {
        let mut records: Vec<&DefinitionRecord> = self.entries.values().collect();
        records.sort_by(|a, b| {
            a.source.priority().cmp(&b.source.priority()).then_with(|| {
                // find the key for each record
                let ka = self
                    .entries
                    .iter()
                    .find(|(_, r)| std::ptr::eq(*r, *a))
                    .map(|(k, _)| k.as_str())
                    .unwrap_or("");
                let kb = self
                    .entries
                    .iter()
                    .find(|(_, r)| std::ptr::eq(*r, *b))
                    .map(|(k, _)| k.as_str())
                    .unwrap_or("");
                ka.cmp(kb)
            })
        });
        records
    }

    /// Return whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Error type for registry loading.
#[derive(Debug)]
pub enum LoadError {
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: std::path::PathBuf,
        source: ParseError,
    },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io { path, source } => {
                write!(f, "IO error reading {}: {}", path.display(), source)
            }
            LoadError::Parse { path, source } => {
                write!(f, "parse error in {}: {}", path.display(), source)
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// Scan a single directory and merge its definitions into the registry.
///
/// Files ending in `.agent.md` or `.md` are parsed with `parse_agent_markdown`.
/// Unreadable files emit a warning to stderr and are skipped (no hard error).
fn scan_dir_into_registry(
    dir: &std::path::Path,
    source: DefinitionSource,
    registry: &mut SessionRegistry,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_md = path.extension().map(|e| e == "md").unwrap_or(false);
        if !is_md {
            continue;
        }
        let key = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .trim_end_matches(".agent")
            .to_string();
        if key.is_empty() {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "warning: AGENTS_LIST_IO_ERROR: cannot read {}: {}",
                    path.display(),
                    e
                );
                continue;
            }
        };
        match parse_agent_markdown(&content) {
            Ok(def) => {
                registry.merge(
                    key,
                    DefinitionRecord {
                        definition: def,
                        source: source.clone(),
                        path: Some(path),
                    },
                );
            }
            Err(e) => {
                eprintln!(
                    "warning: AGENTS_LIST_PARSE_WARN: cannot parse {}: {}",
                    path.display(),
                    e
                );
            }
        }
    }
}

/// Load all persisted definitions from the four source directories into a registry.
///
/// Priority order (ascending, later overrides earlier):
/// 1. `<binary-dir>/agents/`  (builtin — reserved, not loaded in this RFC)
/// 2. `~/.aikit/agents/`      (managed)
/// 3. `~/.config/aikit/agents/` (user)
/// 4. `<workdir>/.aikit/agents/` (project)
pub fn load_persisted_registry(workdir: &std::path::Path) -> Result<SessionRegistry, LoadError> {
    let mut registry = SessionRegistry::new();

    // managed: ~/.aikit/agents/
    if let Some(home) = dirs::home_dir() {
        let managed = home.join(".aikit").join("agents");
        scan_dir_into_registry(&managed, DefinitionSource::Managed, &mut registry);
    }

    // user: ~/.config/aikit/agents/
    if let Some(config_dir) = dirs::config_dir() {
        let user = config_dir.join("aikit").join("agents");
        scan_dir_into_registry(&user, DefinitionSource::User, &mut registry);
    }

    // project: <workdir>/.aikit/agents/
    let project = workdir.join(".aikit").join("agents");
    scan_dir_into_registry(&project, DefinitionSource::Project, &mut registry);

    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_minimal_body_prompt() {
        let md = r"---
name: code-reviewer
description: Runs focused reviews after edits
---

You review code.";
        let d = parse_agent_markdown(md).unwrap();
        assert_eq!(d.name, "code-reviewer");
        assert_eq!(d.description, "Runs focused reviews after edits");
        assert_eq!(d.prompt.trim(), "You review code.");
        assert!(d.tools.is_none());
        assert!(d.disallowed_tools.is_none());
        assert!(d.model.is_none());
        assert!(d.delegation.is_none());
    }

    #[test]
    fn markdown_full_frontmatter_tools_model_agents_star() {
        let md = r"---
name: coord
description: Coordinates specialized workers
tools: ['agent', 'read', 'search']
model: inherit
disallowedTools: Write, Edit
agents: '*'
---

Route work.";
        let d = parse_agent_markdown(md).unwrap();
        assert_eq!(d.name, "coord");
        assert_eq!(
            d.tools.as_ref().unwrap(),
            &vec![
                "agent".to_string(),
                "read".to_string(),
                "search".to_string()
            ]
        );
        assert_eq!(
            d.disallowed_tools.as_ref().unwrap(),
            &vec!["Write".to_string(), "Edit".to_string()]
        );
        assert_eq!(d.model.as_deref(), Some("inherit"));
        assert_eq!(d.delegation, Some(DelegationAllowlist::All));
    }

    #[test]
    fn markdown_agents_empty_list_means_no_delegation() {
        let md = r"---
name: leaf
description: No sub-delegation
tools: [Read]
agents: []
---

Do work.";
        let d = parse_agent_markdown(md).unwrap();
        assert_eq!(d.delegation, Some(DelegationAllowlist::None));
    }

    #[test]
    fn markdown_agents_named_list() {
        let md = r"---
name: tdd
description: TDD flow
tools: [agent]
agents: [Red, Green, Refactor]
---

Implement with TDD.";
        let d = parse_agent_markdown(md).unwrap();
        assert_eq!(
            d.delegation,
            Some(DelegationAllowlist::Names(vec![
                "Red".into(),
                "Green".into(),
                "Refactor".into()
            ]))
        );
    }

    #[test]
    fn markdown_tools_inline_string() {
        let md = r"---
name: explore
description: Read-only search
tools: Read, Grep, Glob
---

Explore.";
        let d = parse_agent_markdown(md).unwrap();
        assert_eq!(
            d.tools.as_ref().unwrap(),
            &vec!["Read".to_string(), "Grep".to_string(), "Glob".to_string()]
        );
    }

    #[test]
    fn markdown_ignores_vscode_dropped_keys_still_parses() {
        let md = r"---
name: internal-helper
description: Hidden from interoperability layer
user-invocable: false
disable-model-invocation: true
argument-hint: foo bar
tools: [Read]
---

Only subagents.";
        let d = parse_agent_markdown(md).unwrap();
        assert_eq!(d.name, "internal-helper");
        assert_eq!(d.tools.as_ref().unwrap(), &vec!["Read".to_string()]);
        assert!(d.prompt.contains("Only subagents."));
    }

    #[test]
    fn markdown_missing_description_errors() {
        let md = r"---
name: x
---

body";
        assert!(matches!(
            parse_agent_markdown(md),
            Err(ParseError::MissingField {
                field: "description"
            })
        ));
    }

    #[test]
    fn markdown_explicit_prompt_overrides_body() {
        let md = r"---
name: x
description: d
prompt: |

  Override prompt here
---

Ignored body.";
        let d = parse_agent_markdown(md).unwrap();
        assert!(d.prompt.contains("Override prompt"));
        assert!(!d.prompt.contains("Ignored body."));
    }

    #[test]
    fn markdown_no_prompt_in_fm_and_empty_body_errors() {
        let md = r"---
name: x
description: y
---


";
        assert!(matches!(
            parse_agent_markdown(md),
            Err(ParseError::MissingField { field: "prompt" })
        ));
    }

    #[test]
    fn json_session_single_agent_full() {
        let j = r#"{
  "worker": {
    "description": "Does work",
    "prompt": "You are worker",
    "tools": ["Read", "Edit"],
    "disallowedTools": ["Bash"],
    "model": "sonnet",
    "agents": ["a", "b"]
  }
}"#;
        let m = parse_session_agents_json(j).unwrap();
        let d = m.get("worker").expect("outer JSON key");
        assert_eq!(d.name, "worker");
        assert_eq!(d.description, "Does work");
        assert_eq!(d.prompt, "You are worker");
        assert_eq!(
            d.delegation,
            Some(DelegationAllowlist::Names(vec!["a".into(), "b".into()]))
        );
    }

    #[test]
    fn json_name_defaults_to_map_key() {
        let j = r#"{"my-key": {"description": "d", "prompt": "p"}}"#;
        let m = parse_session_agents_json(j).unwrap();
        assert_eq!(m["my-key"].name, "my-key");
    }

    #[test]
    fn json_agents_star_and_empty() {
        let j = r#"{
  "a": {"description": "d1", "prompt": "p", "agents": "*"},
  "b": {"description": "d2", "prompt": "p", "agents": []}
}"#;
        let m = parse_session_agents_json(j).unwrap();
        assert_eq!(m["a"].delegation, Some(DelegationAllowlist::All));
        assert_eq!(m["b"].delegation, Some(DelegationAllowlist::None));
    }

    #[test]
    fn json_tools_string_comma_split() {
        let j = r#"{"x": {"description": "d", "prompt": "p", "tools": "Read, Grep"}}"#;
        let m = parse_session_agents_json(j).unwrap();
        assert_eq!(m["x"].tools, Some(vec!["Read".into(), "Grep".into()]));
    }

    #[test]
    fn json_invalid_missing_description() {
        let j = r#"{"x": {"prompt": "only"}}"#;
        assert!(matches!(
            parse_session_agents_json(j),
            Err(ParseError::MissingField {
                field: "description"
            })
        ));
    }

    #[test]
    fn json_invalid_not_object() {
        let j = r#"[]"#;
        assert!(matches!(
            parse_session_agents_json(j),
            Err(ParseError::InvalidJson(_))
        ));
    }

    #[test]
    fn markdown_invalid_unclosed_frontmatter() {
        let md = r"---
name: x
description: y
Still open";
        assert!(parse_agent_markdown(md).is_err());
    }

    #[test]
    fn json_agents_string_only_star_allowed() {
        let j = r#"{"x": {"description": "d", "prompt": "p", "agents": "nope"}}"#;
        assert!(parse_session_agents_json(j).is_err());
    }

    #[test]
    fn json_tools_invalid_type() {
        let j = r#"{"x": {"description": "d", "prompt": "p", "tools": 3}}"#;
        assert!(parse_session_agents_json(j).is_err());
    }

    #[test]
    fn json_multi_agent_merge_map() {
        let j = r#"{
  "a": {"description": "da", "prompt": "pa"},
  "b": {"description": "db", "prompt": "pb", "name": "override-b"}
}"#;
        let m = parse_session_agents_json(j).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m["a"].name, "a");
        assert_eq!(m["b"].name, "override-b");
        assert_eq!(m["b"].description, "db");
    }

    #[test]
    fn markdown_snake_disallowed_tools_key() {
        let md = r"---
name: z
description: zd
disallowed_tools: [Write]
---

Body.";
        let d = parse_agent_markdown(md).unwrap();
        assert_eq!(d.disallowed_tools, Some(vec!["Write".into()]));
    }

    #[test]
    fn markdown_agents_single_string_other_than_star_is_name() {
        let md = r"---
name: only-one
description: d
agents: SingleAgent
---

x";
        let d = parse_agent_markdown(md).unwrap();
        assert_eq!(
            d.delegation,
            Some(DelegationAllowlist::Names(vec!["SingleAgent".into()]))
        );
    }

    // ── Registry tests ──────────────────────────────────────────────────────

    fn make_def(name: &str) -> AgentDefinition {
        AgentDefinition {
            name: name.to_string(),
            description: "d".to_string(),
            prompt: "p".to_string(),
            tools: None,
            disallowed_tools: None,
            model: None,
            delegation: None,
        }
    }

    #[test]
    fn registry_empty() {
        let reg = SessionRegistry::new();
        assert!(reg.is_empty());
        assert!(reg.resolve_by_name("anything").is_none());
        assert!(reg.all_sorted().is_empty());
    }

    #[test]
    fn registry_single_entry() {
        let mut reg = SessionRegistry::new();
        reg.merge(
            "worker".to_string(),
            DefinitionRecord {
                definition: make_def("worker"),
                source: DefinitionSource::Project,
                path: None,
            },
        );
        assert!(!reg.is_empty());
        assert!(reg.resolve_by_name("worker").is_some());
        assert!(reg.resolve_by_name("other").is_none());
    }

    #[test]
    fn registry_project_overrides_user() {
        let mut reg = SessionRegistry::new();
        reg.merge(
            "x".to_string(),
            DefinitionRecord {
                definition: make_def("x-user"),
                source: DefinitionSource::User,
                path: None,
            },
        );
        reg.merge(
            "x".to_string(),
            DefinitionRecord {
                definition: make_def("x-project"),
                source: DefinitionSource::Project,
                path: None,
            },
        );
        let rec = reg.resolve_by_name("x-project").expect("project wins");
        assert_eq!(rec.definition.name, "x-project");
    }

    #[test]
    fn registry_resolve_by_name_exact_match() {
        let mut reg = SessionRegistry::new();
        reg.merge(
            "key".to_string(),
            DefinitionRecord {
                definition: make_def("My Persona"),
                source: DefinitionSource::Session,
                path: None,
            },
        );
        assert!(reg.resolve_by_name("My Persona").is_some());
        assert!(reg.resolve_by_name("my persona").is_none());
        assert!(reg.resolve_by_name("key").is_none());
    }

    #[test]
    fn registry_user_invocable_key_parses_ok() {
        let md = r"---
name: internal-helper
description: Hidden from interoperability layer
user-invocable: false
tools: [Read]
---

Only subagents.";
        let def = parse_agent_markdown(md).unwrap();
        let mut reg = SessionRegistry::new();
        reg.merge(
            "internal-helper".to_string(),
            DefinitionRecord {
                definition: def,
                source: DefinitionSource::Project,
                path: None,
            },
        );
        assert!(reg.resolve_by_name("internal-helper").is_some());
    }

    #[test]
    fn load_persisted_registry_empty_workdir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let reg = load_persisted_registry(tmp.path()).unwrap();
        assert!(reg.is_empty());
    }

    #[test]
    fn load_persisted_registry_project_entry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agents_dir = tmp.path().join(".aikit").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(
            agents_dir.join("reviewer.agent.md"),
            "---\nname: reviewer\ndescription: Does reviews\n---\n\nReview code.",
        )
        .unwrap();
        let reg = load_persisted_registry(tmp.path()).unwrap();
        let rec = reg
            .resolve_by_name("reviewer")
            .expect("reviewer in registry");
        assert_eq!(rec.source, DefinitionSource::Project);
    }
}
