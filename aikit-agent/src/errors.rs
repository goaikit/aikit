use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("E_AIKIT_NO_API_KEY: API key not found (checked: {checked})")]
    NoApiKey { checked: String },

    #[error("E_AIKIT_LLM_REQUEST_FAILED: {message}")]
    LlmRequestFailed { message: String },

    #[error("E_AIKIT_LLM_ERROR_RESPONSE: HTTP {status} from {url}: {body}")]
    LlmErrorResponse {
        status: u16,
        url: String,
        body: String,
    },

    #[error("E_AIKIT_STREAM_PROTOCOL: invalid stream event at line {line}: {detail}")]
    StreamProtocol { line: u64, detail: String },

    #[error("E_AIKIT_CONTEXT_COMPRESSION: {message}")]
    ContextCompression { message: String },

    #[error("E_AIKIT_TOOL_EXEC_FAILED: tool '{tool}' failed: {reason}")]
    ToolExecFailed { tool: String, reason: String },

    #[error("E_AIKIT_SUBAGENT_LIMIT: {message}")]
    SubagentLimit { message: String },

    #[error("E_AIKIT_MAX_ITERATIONS: exceeded max iterations ({max})")]
    MaxIterations { max: u32 },

    #[error("E_AIKIT_SKILL_PARSE_ERROR: skill '{name}': {reason}")]
    SkillParseError { name: String, reason: String },

    #[error("E_AIKIT_AGENTS_MD_READ: failed to read AGENTS.md: {reason}")]
    AgentsMdRead { reason: String },
}
