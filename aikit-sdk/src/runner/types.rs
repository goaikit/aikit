use serde::{Deserialize, Serialize};
use std::io;
use std::process::{Child, ExitStatus};
use std::time::Duration;

/// Extension trait for adding timeout support to Child.
pub(super) trait ChildTimeoutExt {
    /// Wait for the child to exit, with a timeout.
    ///
    /// Returns Ok(Some(status)) if the child exited within the timeout.
    /// Returns Ok(None) if the timeout elapsed.
    /// Returns Err if waiting failed.
    fn wait_timeout(&mut self, duration: Duration) -> io::Result<Option<ExitStatus>>;
}

#[cfg(unix)]
impl ChildTimeoutExt for Child {
    fn wait_timeout(&mut self, duration: Duration) -> io::Result<Option<ExitStatus>> {
        use std::thread;
        let start = std::time::Instant::now();

        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Ok(Some(status)),
                Ok(None) => {
                    let elapsed = start.elapsed();
                    if elapsed >= duration {
                        return Ok(None);
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(windows)]
impl ChildTimeoutExt for Child {
    fn wait_timeout(&mut self, duration: Duration) -> io::Result<Option<ExitStatus>> {
        use std::thread;
        let start = std::time::Instant::now();

        loop {
            match self.try_wait() {
                Ok(Some(status)) => return Ok(Some(status)),
                Ok(None) => {
                    let elapsed = start.elapsed();
                    if elapsed >= duration {
                        return Ok(None);
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Normalized token usage from an agent run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Input (prompt) tokens consumed
    pub input_tokens: u64,
    /// Output (completion) tokens produced
    pub output_tokens: u64,
    /// Total tokens (input + output), if reported by the agent
    pub total_tokens: Option<u64>,
    /// Cache read tokens, if reported by the agent
    pub cache_read_tokens: Option<u64>,
    /// Cache creation/write tokens, if reported by the agent
    pub cache_creation_tokens: Option<u64>,
    /// Reasoning tokens (e.g. chain-of-thought), if reported by the agent
    pub reasoning_tokens: Option<u64>,
}

/// Identifies the agent that produced a token usage entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UsageSource {
    /// OpenAI Codex (`turn.completed.usage`)
    Codex,
    /// Anthropic Claude Code (`result.usage` or `stream_event` message)
    Claude,
    /// Google Gemini CLI (`result.stats`)
    Gemini,
    /// OpenCode (`step_finish.part.tokens`)
    OpenCode,
    /// Cursor Agent (`result.usage` camelCase fields)
    Cursor,
    /// Built-in aikit agent
    Aikit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaCategory {
    Hourly,
    Daily,
    Weekly,
    Requests,
    Tokens,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaExceededInfo {
    pub agent_key: String,
    pub category: QuotaCategory,
    pub raw_message: String,
}

/// Options for running an agent.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RunOptions {
    /// Optional model name/identifier
    pub model: Option<String>,
    /// Whether to run in "yolo" mode (auto-confirm, skip checks)
    pub yolo: bool,
    /// Whether to stream output incrementally
    pub stream: bool,
    /// Maximum wall-clock duration from spawn to child exit or kill.
    ///
    /// When set, a watchdog thread monitors the child process and kills it if
    /// it runs longer than this duration. The kill is process-level (not just
    /// future cancellation), guaranteeing resource cleanup.
    pub timeout: Option<std::time::Duration>,
    /// Working directory for the agent child process only.
    ///
    /// When set, only the spawned child process changes directory; the parent
    /// process working directory is unaffected. This is safe for concurrent
    /// use and async environments.
    pub current_dir: Option<std::path::PathBuf>,
    /// When true (the default), emit `TokenUsageLine` events after each
    /// `JsonLine` event that contains extractable token usage.  Set to false
    /// to suppress those events while still populating `RunResult.token_usage`.
    pub emit_token_usage_events: bool,
    /// When true, emit `RawTransportLine` events alongside `StreamMessage`
    /// events for debugging. Off by default.
    pub emit_raw_transport: bool,
    /// Serialized session persona definition (JSON). Only used for the aikit backend.
    pub session_persona: Option<serde_json::Value>,
    /// Serialized ephemeral agent definitions (JSON map). Only used for the aikit backend.
    pub session_agents: std::collections::HashMap<String, serde_json::Value>,
    /// Session ID for resume. None = new session (default, preserves existing behaviour).
    pub session_id: Option<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            model: None,
            yolo: false,
            stream: false,
            timeout: None,
            current_dir: None,
            emit_token_usage_events: true,
            emit_raw_transport: false,
            session_persona: None,
            session_agents: std::collections::HashMap::new(),
            session_id: None,
        }
    }
}

impl RunOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_yolo(mut self, yolo: bool) -> Self {
        self.yolo = yolo;
        self
    }

    pub fn with_stream(mut self, stream: bool) -> Self {
        self.stream = stream;
        self
    }

    /// Set a maximum wall-clock timeout for the agent child process.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set the working directory for the agent child process.
    pub fn with_current_dir(mut self, path: std::path::PathBuf) -> Self {
        self.current_dir = Some(path);
        self
    }

    /// Control whether `TokenUsageLine` events are emitted.
    pub fn with_emit_token_usage_events(mut self, emit: bool) -> Self {
        self.emit_token_usage_events = emit;
        self
    }

    /// Control whether `RawTransportLine` events are emitted.
    pub fn with_emit_raw_transport(mut self, emit: bool) -> Self {
        self.emit_raw_transport = emit;
        self
    }

    /// Set the session persona (serialized AgentDefinition JSON).
    pub fn with_session_persona(mut self, persona: serde_json::Value) -> Self {
        self.session_persona = Some(persona);
        self
    }

    /// Set the session agents map (serialized AgentDefinition JSON values).
    pub fn with_session_agents(
        mut self,
        agents: std::collections::HashMap<String, serde_json::Value>,
    ) -> Self {
        self.session_agents = agents;
        self
    }

    /// Set the session ID for resume. None (default) starts a new session.
    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }
}

/// Result of running an agent.
#[derive(Debug)]
pub struct RunResult {
    /// Process exit status
    pub status: ExitStatus,
    /// Captured stdout
    pub stdout: Vec<u8>,
    /// Captured stderr
    pub stderr: Vec<u8>,
    /// Aggregated token usage extracted from the agent's output, if any.
    pub token_usage: Option<TokenUsage>,
    /// Structured quota-exceeded signal, if detected during the run.
    pub quota_exceeded: Option<QuotaExceededInfo>,
}

impl RunResult {
    pub fn new(status: ExitStatus, stdout: Vec<u8>, stderr: Vec<u8>) -> Self {
        Self {
            status,
            stdout,
            stderr,
            token_usage: None,
            quota_exceeded: None,
        }
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.status.code()
    }

    pub fn success(&self) -> bool {
        self.status.success()
    }
}

/// Error types for run operations.
#[derive(Debug)]
#[non_exhaustive]
pub enum RunError {
    /// Agent key is not runnable
    AgentNotRunnable(String),
    /// Failed to spawn process
    SpawnFailed(io::Error),
    /// Failed to write to stdin
    StdinFailed(io::Error),
    /// Failed to read stdout/stderr
    OutputFailed(io::Error),
    /// User callback panicked during event processing
    CallbackPanic(Box<dyn std::any::Any + Send>),
    /// Reader thread encountered an I/O error
    ReaderFailed {
        stream: AgentEventStream,
        source: io::Error,
    },
    /// Agent child was killed because it exceeded the configured timeout
    TimedOut {
        /// The timeout duration that was exceeded
        timeout: Duration,
        /// Stdout bytes collected before the child was killed (may be partial)
        stdout: Vec<u8>,
        /// Stderr bytes collected before the child was killed (may be partial)
        stderr: Vec<u8>,
    },
    /// Agent terminated due to a quota or rate-limit signal
    QuotaExceeded(QuotaExceededInfo),
    /// `OutputMode::Progress` was requested but no `ProgressSink` was provided.
    MissingProgressSink,
    /// `run_builtin_agent` was called with a key other than "aikit".
    WrongAgentKey(String),
    /// Session file does not exist at the expected path.
    SessionNotFound(String),
    /// Session file exists but could not be loaded or deserialized.
    SessionLoadFailed { id: String, reason: String },
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::AgentNotRunnable(key) => {
                write!(
                    f,
                    "Agent '{}' is not runnable. Supported: codex, claude, gemini, opencode, agent, aikit",
                    key
                )
            }
            RunError::SpawnFailed(err) => write!(f, "Failed to spawn process: {}", err),
            RunError::StdinFailed(err) => write!(f, "Failed to write to stdin: {}", err),
            RunError::OutputFailed(err) => write!(f, "Failed to read output: {}", err),
            RunError::CallbackPanic(_) => write!(f, "Event callback panicked"),
            RunError::ReaderFailed { stream, source } => {
                write!(f, "Reader failed on {:?} stream: {}", stream, source)
            }
            RunError::TimedOut { timeout, .. } => {
                write!(f, "Agent timed out after {:.3}s", timeout.as_secs_f64())
            }
            RunError::QuotaExceeded(info) => {
                write!(
                    f,
                    "Agent '{}' stopped: quota exceeded ({:?}): {}",
                    info.agent_key, info.category, info.raw_message
                )
            }
            RunError::MissingProgressSink => {
                write!(
                    f,
                    "OutputMode::Progress requires a ProgressSink but none was provided"
                )
            }
            RunError::WrongAgentKey(key) => {
                write!(
                    f,
                    "run_builtin_agent only accepts agent_key \"aikit\", got \"{}\"",
                    key
                )
            }
            RunError::SessionNotFound(id) => {
                write!(f, "error: session '{}' not found", id)
            }
            RunError::SessionLoadFailed { id, reason } => {
                write!(f, "error: session '{}' could not be loaded: {}", id, reason)
            }
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RunError::SpawnFailed(err) => Some(err),
            RunError::StdinFailed(err) => Some(err),
            RunError::OutputFailed(err) => Some(err),
            RunError::ReaderFailed { source, .. } => Some(source),
            RunError::AgentNotRunnable(_)
            | RunError::CallbackPanic(_)
            | RunError::TimedOut { .. }
            | RunError::QuotaExceeded(_)
            | RunError::MissingProgressSink
            | RunError::WrongAgentKey(_)
            | RunError::SessionNotFound(_)
            | RunError::SessionLoadFailed { .. } => None,
        }
    }
}

/// Selects how built-in agent output is delivered to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OutputMode {
    /// Write raw agent text bytes to `writer` (plain output).
    Plain,
    /// Emit one `AgentEvent` JSON line per event to `writer`.
    Events,
    /// Render a live human-readable progress view via `ProgressSink`.
    Progress,
}

/// Receives progress updates during a `run_builtin_agent` call in `Progress` mode.
pub trait ProgressSink: Send {
    fn on_progress(&mut self, progress: &crate::run_progress::RunProgress);
    fn on_finalize(&mut self, exit_code: i32, token_footer: Option<String>);
}

/// Identifies which stream an event or error originated from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentEventStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessagePhase {
    Delta,
    Final,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    Assistant,
    Tool,
    System,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Message,
    Reasoning,
    ToolOutput,
    Status,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamMessage {
    pub text: String,
    pub phase: MessagePhase,
    pub role: MessageRole,
    pub kind: MessageKind,
    pub source: AgentEventStream,
    pub raw_line_seq: u64,
    pub turn_id: Option<String>,
}

/// Payload carried by a streaming agent event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentEventPayload {
    /// Successfully parsed JSON line (internal; not emitted to callbacks by default).
    JsonLine(serde_json::Value),
    /// UTF-8 text line that is not valid JSON
    RawLine(String),
    /// Non-UTF-8 bytes serialized as an array of integers
    RawBytes(Vec<u8>),
    /// Canonical text output from an agent, engine-agnostic.
    StreamMessage(StreamMessage),
    /// Normalized token usage extracted from the preceding `JsonLine`.
    /// Emitted immediately after the corresponding `JsonLine` event when
    /// `RunOptions::emit_token_usage_events` is `true`.
    TokenUsageLine {
        usage: TokenUsage,
        source: UsageSource,
        /// Sequence number of the `JsonLine` event this was extracted from.
        raw_agent_line_seq: u64,
    },
    /// Quota or rate-limit exceeded signal detected from agent output.
    QuotaExceeded {
        info: QuotaExceededInfo,
        /// Sequence number of the JsonLine or RawLine that triggered detection.
        raw_agent_line_seq: u64,
    },
    /// Opt-in raw transport line for debugging (behind `RunOptions::emit_raw_transport`).
    RawTransportLine {
        raw: String,
        stream: AgentEventStream,
        seq: u64,
    },
    /// Built-in aikit agent text delta.
    AikitTextDelta {
        content: String,
        turn_id: Option<String>,
    },
    /// Built-in aikit agent final text.
    AikitTextFinal {
        content: String,
        turn_id: Option<String>,
    },
    /// Built-in aikit agent tool use.
    AikitToolUse {
        tool_name: String,
        tool_input: serde_json::Value,
        call_id: String,
    },
    /// Built-in aikit agent tool result.
    AikitToolResult {
        call_id: String,
        output: String,
        is_error: bool,
    },
    /// Built-in aikit agent sub-agent spawn.
    AikitSubagentSpawn {
        subagent_id: String,
        workdir: String,
    },
    /// Built-in aikit agent sub-agent result.
    AikitSubagentResult {
        subagent_id: String,
        status: String,
        changed_files: Vec<String>,
        key_findings: String,
        final_message: String,
    },
    /// Built-in aikit agent context compression.
    AikitContextCompressed {
        original_tokens: u64,
        compressed_tokens: u64,
        turns_summarized: u64,
    },
    /// Built-in aikit agent step finish.
    AikitStepFinish {
        iteration: u32,
        finish_reason: String,
    },
    /// Emitted once per run as the first event when the SDK has assigned (or
    /// been given) a session_id. Useful for callers that mint a session
    /// implicitly (no `session_id` in `RunOptions`) and need to learn the new
    /// id without parsing stderr.
    SessionStarted { session_id: String },
}

/// A single event emitted by a streaming agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEvent {
    /// The agent key that produced this event
    pub agent_key: String,
    /// Monotonically increasing sequence number across all streams
    pub seq: u64,
    /// Which stream this event came from
    pub stream: AgentEventStream,
    /// The event payload
    pub payload: AgentEventPayload,
}

impl AgentEvent {
    /// Serialize this event to a JSON string.
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// Internal channel message from reader threads to the dispatcher.
pub(crate) enum ReaderMsg {
    Chunk {
        stream: AgentEventStream,
        /// Raw bytes including any newline character(s)
        raw: Vec<u8>,
    },
    Err {
        stream: AgentEventStream,
        source: io::Error,
    },
}

/// Reason why an agent is not available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentAvailabilityReason {
    /// Agent is not runnable (not in runnable_agents list)
    NotRunnable,
    /// Binary not found in PATH
    BinaryNotFound,
    /// Binary found but --version check failed
    VersionCheckFailed,
    /// Probe timed out
    TimedOut,
}

impl std::fmt::Display for AgentAvailabilityReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentAvailabilityReason::NotRunnable => write!(f, "not_runnable"),
            AgentAvailabilityReason::BinaryNotFound => write!(f, "binary_not_found"),
            AgentAvailabilityReason::VersionCheckFailed => write!(f, "version_check_failed"),
            AgentAvailabilityReason::TimedOut => write!(f, "timed_out"),
        }
    }
}

/// Status of an agent's availability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentStatus {
    /// Whether the agent is available and runnable
    pub available: bool,
    /// Reason if not available
    pub reason: Option<AgentAvailabilityReason>,
}

impl AgentStatus {
    pub fn available() -> Self {
        Self {
            available: true,
            reason: None,
        }
    }

    pub fn unavailable(reason: AgentAvailabilityReason) -> Self {
        Self {
            available: false,
            reason: Some(reason),
        }
    }
}
