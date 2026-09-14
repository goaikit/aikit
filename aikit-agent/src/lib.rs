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
        /// Wall-clock time the tool took, measured around `execute_tool`
        /// (ADR 0022).
        duration_ms: u64,
        /// Unix epoch milliseconds when the tool started.
        started_at_ms: i64,
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
    /// The harness in effect for this run, emitted once at run start when
    /// [`AgentConfig::capture_harness`] is set (ADR 0022).
    HarnessSnapshot {
        model: String,
        /// The system prompt exactly as the model receives it.
        system_prompt: String,
        /// Tool definitions offered to the model, after the persona filter.
        tools: Vec<llm::types::ToolDefinition>,
        /// Names of the skills discovered for the run.
        skills: Vec<String>,
        /// Names of the hooks this harness may fire.
        hooks: Vec<String>,
    },
    /// A harness hook acted on the run (ADR 0022). Context compression keeps
    /// its own [`AgentInternalEvent::ContextCompressed`] frame and is not
    /// re-emitted as a `Hook`.
    Hook {
        phase: HookPhase,
        hook_name: String,
        action: HookAction,
        payload: Option<serde_json::Value>,
    },
}

/// Where in the loop a [`AgentInternalEvent::Hook`] ran (ADR 0022).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPhase {
    RunStart,
    BeforeModel,
    AfterModel,
    BeforeTool,
    AfterTool,
    RunEnd,
}

/// What a [`AgentInternalEvent::Hook`] did (ADR 0022).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookAction {
    Injected,
    Blocked,
    Modified,
    Observed,
}

/// Hook names the in-process agent emits (ADR 0022). Also listed in
/// [`AgentInternalEvent::HarnessSnapshot::hooks`] so a reader can tell "no
/// hook fired" from "this harness has no hooks".
pub mod hook_names {
    /// Context compression before a model call; recorded by
    /// `AgentInternalEvent::ContextCompressed`, folded into a `hook` line by
    /// the eval trace.
    pub const CONTEXT_COMPRESSION: &str = "context_compression";
    /// The loop refused a tool call because no tool of that name is in the
    /// set the model was offered.
    pub const TOOL_DISPATCH: &str = "tool_dispatch";
    /// A persona's `tools` / `disallowed_tools` policy removed tools from
    /// the set at run start.
    pub const PERSONA_TOOL_POLICY: &str = "persona_tool_policy";
}

/// Run the agent to completion, collecting every event into a `Vec` returned once the
/// run finishes. A thin convenience wrapper over [`run_streaming`] for callers that want
/// the "collect everything" contract (e.g. tests, or callers that don't need incremental
/// delivery).
pub fn run(
    config: AgentConfig,
    prompt: &str,
    gateway: Box<dyn LlmGateway>,
) -> Result<Vec<AgentInternalEvent>, AgentError> {
    loop_runner::run(config, prompt, gateway)
}

/// Streaming entry point: `on_event` is invoked immediately as each [`AgentInternalEvent`]
/// is produced by the agent loop, instead of buffering the whole run in memory and
/// replaying it once the run completes. This is the primitive [`run`] is built on.
pub fn run_streaming(
    config: AgentConfig,
    prompt: &str,
    gateway: Box<dyn LlmGateway>,
    on_event: impl FnMut(AgentInternalEvent),
) -> Result<(), AgentError> {
    loop_runner::run_streaming(config, prompt, gateway, on_event)
}

pub use context::Turn;
pub use loop_runner::{run_with_context, run_with_context_streaming};

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }
}
