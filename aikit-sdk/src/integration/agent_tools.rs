//! Native tool rendering; exporting a bundle is not an owned installation or
//! qualification. The application still authenticates/scopes every invocation.
use super::{HookCommand, IntegrationError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const AGENT_TOOL_REQUEST_LIMIT: usize = 64 * 1024;
pub const AGENT_TOOL_CALL_LIMIT: usize = 128 * 1024;
pub const AGENT_TOOL_SPEC_LIMIT: usize = 256 * 1024;

/// One application dispatcher, with the application's own operation schema.
/// Handler arguments contain fixed credential/context references, never secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolSpec {
    pub application_id: String,
    /// Stable owned installation identity, distinct across registered checkouts.
    pub installation_id: String,
    pub tool_name: String,
    pub description: String,
    pub input_schema: Value,
    pub instructions: String,
    pub handler: HookCommand,
    pub timeout_ms: u32,
}
impl AgentToolSpec {
    pub fn read(reader: impl Read) -> Result<Self, IntegrationError> {
        bounded_json(reader, AGENT_TOOL_SPEC_LIMIT)
    }
}

fn bounded_json<T: serde::de::DeserializeOwned>(
    reader: impl Read,
    limit: usize,
) -> Result<T, IntegrationError> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(IntegrationError::Invalid(
            "agent tool JSON exceeds its input limit".into(),
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| IntegrationError::Invalid("invalid agent tool JSON".into()))
}

/// Native context supplied by the mod, outside model-controlled request data.
/// These fields are claims, NOT a credential or authenticated SessionRef.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeToolContext {
    pub session_id: String,
    pub invocation_id: String,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolCall {
    pub schema_version: u32,
    pub application_id: String,
    pub installation_id: String,
    pub agent_key: String,
    pub tool_name: String,
    pub native_context: NativeToolContext,
    pub request: Value,
}
impl AgentToolCall {
    /// Bounded decoding only. Validate these claims against trusted installation,
    /// credentials and current recorded/native generation before applying data.
    pub fn read(reader: impl Read) -> Result<Self, IntegrationError> {
        let call: Self = bounded_json(reader, AGENT_TOOL_CALL_LIMIT)?;
        if call.schema_version != 1
            || call.agent_key != "claude"
            || !crate::paths::is_safe_id(&call.application_id)
            || !crate::paths::is_safe_id(&call.installation_id)
            || !tool_name(&call.tool_name)
            || call.native_context.session_id.is_empty()
            || call.native_context.session_id.len() > 256
            || call.native_context.session_id.contains('\0')
            || uuid::Uuid::parse_str(&call.native_context.invocation_id).is_err()
            || !call.native_context.cwd.is_absolute()
            || call.native_context.cwd.as_os_str().len() > 32768
            || !call.request.is_object()
            || serde_json::to_vec(&call.request)?.len() > AGENT_TOOL_REQUEST_LIMIT
        {
            return Err(IntegrationError::Invalid(
                "invalid agent tool call envelope".into(),
            ));
        }
        Ok(call)
    }
}

/// In-memory plugin source, with fixed relative file names. No receipt or native
/// settings are changed by rendering/exporting. Reuse owned installation planning
/// before activation; do not treat arbitrary folder replacement as ownership.
pub struct AgentToolPlugin {
    name: String,
    native_name: String,
    files: BTreeMap<String, String>,
}
impl AgentToolPlugin {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn native_tool_name(&self) -> &str {
        &self.native_name
    }
    pub fn files(&self) -> &BTreeMap<String, String> {
        &self.files
    }
    /// Export only into a new directory. Existing files/directories are never
    /// replaced. A failed export can leave its new partial directory for inspection.
    pub fn export_to_new_directory(&self, destination: &Path) -> Result<(), IntegrationError> {
        fs::create_dir(destination)?;
        fs::create_dir(destination.join(".claude-plugin"))?;
        fs::create_dir(destination.join("hooks"))?;
        for (name, contents) in &self.files {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination.join(name))?;
            file.write_all(contents.as_bytes())?;
            file.sync_all()?;
        }
        Ok(())
    }
}

fn tool_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

struct LocalSchemaOnly;
impl jsonschema::Retrieve for LocalSchemaOnly {
    fn retrieve(
        &self,
        _: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err("agent tool schemas must be self-contained".into())
    }
}

/// Render the verified Claude mod API. Other catalog keys are explicit
/// Unsupported. Rendering never starts an agent or enables native readiness.
pub fn render_agent_tool_plugin(
    agent_key: &str,
    spec: &AgentToolSpec,
) -> Result<AgentToolPlugin, IntegrationError> {
    if crate::agent(agent_key).is_none() {
        return Err(IntegrationError::UnknownAgent(agent_key.into()));
    }
    if agent_key != "claude" {
        return Err(IntegrationError::Unsupported(
            "native agent tool rendering for this adapter".into(),
        ));
    }
    super::command::validate_handler(&spec.handler)?;
    if serde_json::to_vec(spec)?.len() > AGENT_TOOL_SPEC_LIMIT
        || !crate::paths::is_safe_id(&spec.application_id)
        || !crate::paths::is_safe_id(&spec.installation_id)
        || !tool_name(&spec.tool_name)
        || spec.description.trim().is_empty()
        || spec.description.len() > 4096
        || spec.instructions.len() > 32768
        || !(1..=30000).contains(&spec.timeout_ms)
        || spec.input_schema.get("type").and_then(Value::as_str) != Some("object")
        || serde_json::to_vec(&spec.input_schema)?.len() > AGENT_TOOL_REQUEST_LIMIT
    {
        return Err(IntegrationError::Invalid(
            "invalid bounded agent tool specification".into(),
        ));
    }
    jsonschema::options()
        .with_retriever(LocalSchemaOnly)
        .build(&spec.input_schema)
        .map_err(|_| IntegrationError::Invalid("invalid agent tool input schema".into()))?;
    let stem: String = spec
        .application_id
        .chars()
        .take(20)
        .map(|ch| {
            if ch == '.' || ch == '_' {
                '-'
            } else {
                ch.to_ascii_lowercase()
            }
        })
        .collect();
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(
            &spec.application_id,
            &spec.installation_id
        ))?)
    );
    let name = format!("aikit-{stem}-{}", &digest[..8]);
    let native_name = format!("mcp__{name}__{}", spec.tool_name);
    if native_name.len() > 64 {
        return Err(IntegrationError::Invalid(
            "combined native tool name exceeds 64 bytes".into(),
        ));
    }
    let argv: Vec<_> = std::iter::once(
        spec.handler
            .executable
            .to_str()
            .expect("validated UTF-8")
            .to_owned(),
    )
    .chain(spec.handler.arguments.clone())
    .collect();
    let config = json!({"applicationId":spec.application_id,"installationId":spec.installation_id,"toolName":spec.tool_name,"description":spec.description,
        "schema":spec.input_schema,"instructions":spec.instructions,"contextName":format!("{name}-instructions"),
        "argv":argv,"timeoutMs":spec.timeout_ms});
    let module = include_str!("agent_tools_claude.js")
        .replace(
            "'AIKIT_NATIVE_TOOL_NAME'",
            &serde_json::to_string(&native_name)?,
        )
        .replace("/* AIKIT_CONFIG */null", &serde_json::to_string(&config)?);
    let content_digest = format!("{:x}", Sha256::digest(module.as_bytes()));
    let version = format!("0.1.0-sdk{}", &content_digest[..12]);
    let files = BTreeMap::from([
        (
            ".claude-plugin/plugin.json".into(),
            serde_json::to_string_pretty(
                &json!({"name":name,"version":version,"description":spec.description,"author":{"name":"AIKit"}}),
            )?,
        ),
        (
            "hooks/hooks.json".into(),
            "{\"modules\":[\"./register.js\"]}".into(),
        ),
        ("hooks/register.js".into(), module),
    ]);
    Ok(AgentToolPlugin {
        name,
        native_name,
        files,
    })
}

#[cfg(test)]
#[path = "agent_tools_tests.rs"]
mod tests;
