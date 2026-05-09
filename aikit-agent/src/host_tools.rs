use serde_json::Value;

/// A tool definition supplied by an embedder.
#[derive(Debug, Clone)]
pub struct HostToolDefinition {
    /// Tool name. MUST be unique and MUST NOT collide with built-in tool names.
    pub name: String,
    /// Human-readable description shown to the LLM.
    pub description: Option<String>,
    /// JSON Schema object describing the tool's input parameters.
    pub parameters: Value,
}

/// Trait implemented by an embedder to inject extra tools into the agent loop.
///
/// `list_tools` is called once at loop start. `call_tool` is called in the
/// tool-execution step when the LLM invokes a host tool by name.
/// Sandboxing and confirmation prompts are the embedder's responsibility.
pub trait HostToolProvider: Send + Sync {
    fn list_tools(&self) -> Vec<HostToolDefinition>;
    fn call_tool(&self, name: &str, arguments: Value) -> Result<String, String>;
}
