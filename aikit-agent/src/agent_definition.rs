use serde::Deserialize;

/// Runtime persona extracted from an AgentDefinition for use in the loop runner.
///
/// This is the runtime counterpart to `aikit::AgentDefinition`, deserialized from
/// JSON serialized in the CLI layer and passed through `RunOptions`.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentPersona {
    pub name: String,
    pub description: String,
    pub prompt: String,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub disallowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub model: Option<String>,
}
