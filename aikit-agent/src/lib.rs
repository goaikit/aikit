pub mod agent_definition;
pub mod compression;
pub mod config;
pub mod context;
pub mod errors;
pub mod host_tools;
pub mod llm;
pub mod loop_runner;
pub mod skills;
pub mod subagents;
pub mod tools;

pub use agent_definition::AgentPersona;
pub use config::AgentConfig;
pub use errors::AgentError;
pub use host_tools::{HostToolDefinition, HostToolProvider};
pub use llm::{LlmError, LlmGateway, LlmRequest, LlmResponse, LlmStreamEvent, LlmUsage};

#[derive(Debug, Clone)]
pub enum AgentInternalEvent {
    TextDelta {
        content: String,
        turn_id: Option<String>,
    },
    TextFinal {
        content: String,
        turn_id: Option<String>,
    },
    ToolUse {
        tool_name: String,
        tool_input: serde_json::Value,
        call_id: String,
    },
    ToolResult {
        call_id: String,
        output: String,
        is_error: bool,
    },
    SubagentSpawn {
        subagent_id: String,
        workdir: String,
    },
    SubagentResult {
        subagent_id: String,
        status: String,
        changed_files: Vec<String>,
        key_findings: String,
        final_message: String,
    },
    ContextCompressed {
        original_tokens: u64,
        compressed_tokens: u64,
        turns_summarized: u64,
    },
    StepFinish {
        iteration: u32,
        finish_reason: String,
    },
    TokenUsage {
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: Option<u64>,
    },
    Error {
        code: String,
        message: String,
    },
}

pub fn run(
    config: AgentConfig,
    prompt: &str,
    gateway: Box<dyn LlmGateway>,
) -> Result<Vec<AgentInternalEvent>, AgentError> {
    loop_runner::run(config, prompt, gateway)
}

pub use context::Turn;
pub use loop_runner::run_with_context;

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }
}
