//! The aikit agent loop as an I/O-free step machine.
//!
//! [`AgentCore`] decides what the agent does next; the host does it. The core
//! returns [`Effect`]s (call the model, call a tool, emit an event, finish)
//! and the host feeds back [`Input`]s (model stream lines, tool results, the
//! time). The core runs no async runtime, makes no HTTP call, touches no file
//! and reads no clock, so it builds for `wasm32-unknown-unknown` and crosses
//! FFI boundaries as JSON: every effect and input is `serde`.
//!
//! `aikit-agent` is the native, blocking host. See
//! `docs/plans/aikit-agent-core.md` in the aikit repository.

pub mod compression;
pub mod context;
pub mod llm;
pub mod machine;

pub use compression::CompressionResult;
pub use context::{ContextToolCall, ContextToolResult, TokenBudget, Turn, TurnRole};
pub use llm::{
    FunctionDefinition, LlmError, LlmMessage, LlmRequest, LlmResponse, LlmStreamEvent, LlmUsage,
    SseLine, SseParser, ToolCall, ToolCallFunction, ToolChoice, ToolDefinition,
};
pub use machine::{
    AgentCore, CoreConfig, Effect, Input, InputError, ModelRequest, Outcome, RunError, ToolOutcome,
    ToolTiming,
};

use serde::{Deserialize, Serialize};

/// What the agent reports while it runs. The `type` tag names the variant in
/// JSON. `aikit-agent` exports this type as `AgentInternalEvent`.
///
/// The core emits the loop's events. `HarnessSnapshot`, `SubagentSpawn` and
/// `SubagentResult` come from the host, which knows the harness and runs
/// sub-agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
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
        /// Wall-clock time the tool took (ADR 0022).
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
    /// The harness in effect for this run, emitted once at run start when the
    /// host captures the harness (ADR 0022).
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
    /// its own [`AgentEvent::ContextCompressed`] frame and is not re-emitted
    /// as a `Hook`.
    Hook {
        phase: HookPhase,
        hook_name: String,
        action: HookAction,
        payload: Option<serde_json::Value>,
    },
}

/// Where in the loop a [`AgentEvent::Hook`] ran (ADR 0022).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPhase {
    RunStart,
    BeforeModel,
    AfterModel,
    BeforeTool,
    AfterTool,
    RunEnd,
}

/// What a [`AgentEvent::Hook`] did (ADR 0022).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookAction {
    Injected,
    Blocked,
    Modified,
    Observed,
}

/// Hook names the agent emits (ADR 0022). Also listed in
/// [`AgentEvent::HarnessSnapshot::hooks`] so a reader can tell "no hook
/// fired" from "this harness has no hooks".
pub mod hook_names {
    /// Context compression before a model call; recorded by
    /// `AgentEvent::ContextCompressed`, folded into a `hook` line by the eval
    /// trace.
    pub const CONTEXT_COMPRESSION: &str = "context_compression";
    /// The loop refused a tool call because no tool of that name is in the
    /// set the model was offered.
    pub const TOOL_DISPATCH: &str = "tool_dispatch";
    /// A persona's `tools` / `disallowed_tools` policy removed tools from
    /// the set at run start.
    pub const PERSONA_TOOL_POLICY: &str = "persona_tool_policy";
}
