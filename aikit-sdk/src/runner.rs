use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::io::{BufRead, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use std::{panic, thread};

/// Extension trait for adding timeout support to Child.
trait ChildTimeoutExt {
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
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::AgentNotRunnable(key) => {
                write!(
                    f,
                    "Agent '{}' is not runnable. Supported: codex, claude, gemini, opencode, agent",
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
            | RunError::QuotaExceeded(_) => None,
        }
    }
}

/// Identifies which stream an event or error originated from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentEventStream {
    Stdout,
    Stderr,
}

/// Payload carried by a streaming agent event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentEventPayload {
    /// Successfully parsed JSON line
    JsonLine(serde_json::Value),
    /// UTF-8 text line that is not valid JSON
    RawLine(String),
    /// Non-UTF-8 bytes serialized as an array of integers
    RawBytes(Vec<u8>),
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
enum ReaderMsg {
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

/// Returns the list of runnable agent keys.
pub fn runnable_agents() -> &'static [&'static str] {
    &["codex", "claude", "gemini", "opencode", "agent"]
}

/// Checks if an agent key is runnable.
pub fn is_runnable(agent_key: &str) -> bool {
    runnable_agents().contains(&agent_key)
}

/// Builds command-line arguments for codex.
fn build_codex_argv(
    _prompt: &str,
    model: Option<&String>,
    yolo: bool,
    _stream: bool,
) -> Vec<OsString> {
    let mut argv = vec![OsString::from("codex"), OsString::from("exec")];

    if let Some(m) = model {
        argv.push(OsString::from("-m"));
        argv.push(OsString::from(m.as_str()));
    }

    if yolo {
        argv.push(OsString::from("--yolo"));
    }

    argv.push(OsString::from("--json"));
    argv.push(OsString::from("--"));
    argv.push(OsString::from("-"));

    argv
}

/// Builds command-line arguments for claude.
fn build_claude_argv(
    prompt: &str,
    model: Option<&String>,
    _yolo: bool,
    stream: bool,
) -> Vec<OsString> {
    let mut argv = vec![
        OsString::from("claude"),
        OsString::from("-p"),
        OsString::from(prompt),
        OsString::from("--dangerously-skip-permissions"),
    ];

    if let Some(m) = model {
        argv.push(OsString::from("--model"));
        argv.push(OsString::from(m.as_str()));
    }

    argv.push(OsString::from("--output-format"));
    argv.push(OsString::from(if stream { "stream-json" } else { "text" }));

    argv
}

/// Builds command-line arguments for gemini.
fn build_gemini_argv(
    prompt: &str,
    model: Option<&String>,
    _yolo: bool,
    _stream: bool,
) -> Vec<OsString> {
    let mut argv = vec![
        OsString::from("gemini"),
        OsString::from("--prompt"),
        OsString::from(prompt),
    ];

    if let Some(m) = model {
        argv.push(OsString::from("--model"));
        argv.push(OsString::from(m.as_str()));
    }

    argv
}

/// Builds command-line arguments for opencode.
fn build_opencode_argv(
    prompt: &str,
    model: Option<&String>,
    yolo: bool,
    _stream: bool,
) -> Vec<OsString> {
    let mut argv = vec![
        OsString::from("opencode"),
        OsString::from("--prompt"),
        OsString::from(prompt),
    ];

    if let Some(m) = model {
        argv.push(OsString::from("--model"));
        argv.push(OsString::from(m.as_str()));
    }

    if yolo {
        argv.push(OsString::from("--yolo"));
    }

    argv
}

/// Builds command-line arguments for Cursor Agent CLI (headless mode).
fn build_cursor_agent_argv(
    prompt: &str,
    model: Option<&String>,
    yolo: bool,
    stream: bool,
) -> Vec<OsString> {
    let mut argv = vec![OsString::from("agent"), OsString::from("--print")];

    if stream {
        argv.extend_from_slice(&[OsString::from("--output-format"), OsString::from("json")]);
    }

    if let Some(m) = model {
        argv.push(OsString::from("--model"));
        argv.push(OsString::from(m.as_str()));
    }

    if yolo {
        argv.push(OsString::from("--force"));
    }

    argv.push(OsString::from(prompt));
    argv
}

/// Event-mode argv builder for codex: emits machine-readable JSON output.
fn build_codex_argv_events(_prompt: &str, model: Option<&String>, yolo: bool) -> Vec<OsString> {
    let mut argv = vec![OsString::from("codex"), OsString::from("exec")];

    if let Some(m) = model {
        argv.push(OsString::from("-m"));
        argv.push(OsString::from(m.as_str()));
    }

    if yolo {
        argv.push(OsString::from("--yolo"));
    }

    argv.push(OsString::from("--json"));
    argv.push(OsString::from("--"));
    argv.push(OsString::from("-"));

    argv
}

/// Event-mode argv builder for claude: emits stream-json output.
fn build_claude_argv_events(prompt: &str, model: Option<&String>, stream: bool) -> Vec<OsString> {
    let mut argv = vec![
        OsString::from("claude"),
        OsString::from("-p"),
        OsString::from(prompt),
        OsString::from("--dangerously-skip-permissions"),
    ];

    if let Some(m) = model {
        argv.push(OsString::from("--model"));
        argv.push(OsString::from(m.as_str()));
    }

    argv.push(OsString::from("--output-format"));
    argv.push(OsString::from(if stream { "stream-json" } else { "json" }));

    argv
}

/// Event-mode argv builder for gemini: emits stream-json output in headless mode.
fn build_gemini_argv_events(prompt: &str, model: Option<&String>) -> Vec<OsString> {
    let mut argv = vec![
        OsString::from("gemini"),
        OsString::from("--prompt"),
        OsString::from(prompt),
        OsString::from("--output-format"),
        OsString::from("stream-json"),
        OsString::from("--yolo"),
    ];

    if let Some(m) = model {
        argv.push(OsString::from("--model"));
        argv.push(OsString::from(m.as_str()));
    }

    argv
}

/// Event-mode argv builder for opencode: uses `run` subcommand with `--format json`.
fn build_opencode_argv_events(prompt: &str, model: Option<&String>, _yolo: bool) -> Vec<OsString> {
    let mut argv = vec![OsString::from("opencode")];

    if let Some(m) = model {
        argv.push(OsString::from("-m"));
        argv.push(OsString::from(m.as_str()));
    }

    argv.push(OsString::from("run"));
    argv.push(OsString::from(prompt));
    argv.push(OsString::from("--format"));
    argv.push(OsString::from("json"));

    argv
}

/// Event-mode argv builder for Cursor Agent CLI: emits JSON output.
fn build_cursor_agent_argv_events(
    prompt: &str,
    model: Option<&String>,
    yolo: bool,
    stream: bool,
) -> Vec<OsString> {
    let mut argv = vec![
        OsString::from("agent"),
        OsString::from("--print"),
        OsString::from("--output-format"),
    ];

    if stream {
        argv.push(OsString::from("stream-json"));
    } else {
        argv.push(OsString::from("json"));
    }

    if let Some(m) = model {
        argv.push(OsString::from("--model"));
        argv.push(OsString::from(m.as_str()));
    }

    if yolo {
        argv.push(OsString::from("--force"));
    }

    argv.push(OsString::from(prompt));
    argv
}

/// Returns whether the agent key expects the prompt written to stdin.
/// Cursor Agent ("agent") and OpenCode ("opencode") take the prompt as a
/// positional argument instead.
fn should_write_stdin(agent_key: &str) -> bool {
    agent_key != "agent" && agent_key != "opencode"
}

// ---------------------------------------------------------------------------
// Token usage extraction
// ---------------------------------------------------------------------------

fn sum_optional<'a>(vals: impl Iterator<Item = &'a Option<u64>>) -> Option<u64> {
    let collected: Vec<_> = vals.collect();
    if collected.iter().any(|v| v.is_some()) {
        Some(collected.iter().map(|v| v.unwrap_or(0)).sum())
    } else {
        None
    }
}

fn extract_codex_usage(line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
    if line.get("type")?.as_str()? != "turn.completed" {
        return None;
    }
    let usage = line.get("usage")?;
    let input_tokens = usage.get("input_tokens")?.as_u64()?;
    let output_tokens = usage.get("output_tokens")?.as_u64()?;
    let cache_read_tokens = usage.get("cached_input_tokens").and_then(|v| v.as_u64());
    Some((
        TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens: None,
            cache_read_tokens,
            cache_creation_tokens: None,
            reasoning_tokens: None,
        },
        UsageSource::Codex,
    ))
}

fn extract_claude_usage(line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
    let line_type = line.get("type")?.as_str()?;

    let usage = if line_type == "result" {
        line.get("usage")?
    } else if line_type == "stream_event" {
        let event = line.get("event")?;
        let event_type = event.get("type")?.as_str()?;
        if event_type == "message_start" {
            event.get("message")?.get("usage")?
        } else if event_type == "message_delta" {
            event.get("usage")?
        } else {
            return None;
        }
    } else {
        return None;
    };

    let input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_read_tokens = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64());
    let cache_creation_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64());

    if input_tokens == 0
        && output_tokens == 0
        && cache_read_tokens.is_none()
        && cache_creation_tokens.is_none()
    {
        return None;
    }

    Some((
        TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens: None,
            cache_read_tokens,
            cache_creation_tokens,
            reasoning_tokens: None,
        },
        UsageSource::Claude,
    ))
}

fn extract_gemini_usage(line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
    if line.get("type")?.as_str()? != "result" {
        return None;
    }
    let stats = line.get("stats")?;
    let input_tokens = stats.get("input_tokens")?.as_u64()?;
    let output_tokens = stats.get("output_tokens")?.as_u64()?;
    let total_tokens = stats.get("total_tokens").and_then(|v| v.as_u64());
    let cache_read_tokens = stats.get("cached").and_then(|v| v.as_u64());
    Some((
        TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens,
            cache_read_tokens,
            cache_creation_tokens: None,
            reasoning_tokens: None,
        },
        UsageSource::Gemini,
    ))
}

fn extract_opencode_usage(line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
    if line.get("type")?.as_str()? != "step_finish" {
        return None;
    }
    let tokens = line.get("part")?.get("tokens")?;
    let input_tokens = tokens.get("input")?.as_u64()?;
    let output_tokens = tokens.get("output")?.as_u64()?;
    let total_tokens = tokens.get("total").and_then(|v| v.as_u64());
    let reasoning_tokens = tokens.get("reasoning").and_then(|v| v.as_u64());
    let cache_read_tokens = tokens
        .get("cache")
        .and_then(|c| c.get("read"))
        .and_then(|v| v.as_u64());
    let cache_creation_tokens = tokens
        .get("cache")
        .and_then(|c| c.get("write"))
        .and_then(|v| v.as_u64());
    Some((
        TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            reasoning_tokens,
        },
        UsageSource::OpenCode,
    ))
}

fn extract_cursor_usage(line: &serde_json::Value) -> Option<(TokenUsage, UsageSource)> {
    if line.get("type")?.as_str()? != "result" {
        return None;
    }
    let usage = line.get("usage")?;
    let input_tokens = usage.get("inputTokens")?.as_u64()?;
    let output_tokens = usage.get("outputTokens")?.as_u64()?;
    let cache_read_tokens = usage.get("cacheReadTokens").and_then(|v| v.as_u64());
    let cache_creation_tokens = usage.get("cacheWriteTokens").and_then(|v| v.as_u64());
    Some((
        TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens: None,
            cache_read_tokens,
            cache_creation_tokens,
            reasoning_tokens: None,
        },
        UsageSource::Cursor,
    ))
}

/// Extract and normalize token usage from a single agent output line.
///
/// Returns `None` for lines that do not carry usage data or for unknown agent keys.
pub fn extract_usage_from_line(
    line: &serde_json::Value,
    agent_key: &str,
) -> Option<(TokenUsage, UsageSource)> {
    match agent_key {
        "codex" => extract_codex_usage(line),
        "claude" => extract_claude_usage(line),
        "gemini" => extract_gemini_usage(line),
        "opencode" => extract_opencode_usage(line),
        "agent" => extract_cursor_usage(line),
        _ => None,
    }
}

fn infer_quota_category(msg: &str) -> QuotaCategory {
    let lower = msg.to_lowercase();
    if lower.contains("hour") {
        QuotaCategory::Hourly
    } else if lower.contains("per day")
        || lower.contains("daily")
        || lower.contains(" day ")
        || lower.ends_with(" day")
        || lower.starts_with("day ")
        || lower.contains("day,")
    {
        QuotaCategory::Daily
    } else if lower.contains("week") {
        QuotaCategory::Weekly
    } else if lower.contains("long context") || lower.contains("token") {
        QuotaCategory::Tokens
    } else if lower.contains("request") {
        QuotaCategory::Requests
    } else {
        QuotaCategory::Unknown
    }
}

fn truncate_message(msg: &str, max_len: usize) -> String {
    if msg.len() <= max_len {
        msg.to_string()
    } else {
        let mut end = max_len;
        while !msg.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        msg[..end].to_string()
    }
}

pub fn extract_quota_signal(
    agent_key: &str,
    payload: &AgentEventPayload,
) -> Option<QuotaExceededInfo> {
    match agent_key {
        "claude" => extract_claude_quota_signal(payload),
        "codex" => extract_codex_quota_signal(payload),
        "gemini" => extract_gemini_quota_signal(payload),
        "opencode" => extract_opencode_quota_signal(payload),
        "agent" => extract_agent_quota_signal(payload),
        _ => None,
    }
}

fn extract_claude_quota_signal(payload: &AgentEventPayload) -> Option<QuotaExceededInfo> {
    let agent_key = "claude";
    match payload {
        AgentEventPayload::RawLine(text) => {
            let lower = text.to_lowercase();

            // §4.5.1 item 1: "Failed to load usage data" with embedded rate_limit_error JSON
            if let Some(idx) = text.find("Failed to load usage data") {
                if let Some(brace_start) = text[idx..].find('{') {
                    let json_fragment = &text[idx + brace_start..];
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_fragment) {
                        if let Some(error_msg) = extract_nested_rate_limit_error(&val) {
                            return Some(QuotaExceededInfo {
                                agent_key: agent_key.to_string(),
                                category: infer_quota_category(&error_msg),
                                raw_message: truncate_message(&error_msg, 500),
                            });
                        }
                    }
                }
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(text),
                    raw_message: truncate_message(text, 500),
                });
            }

            // §4.5.1 item 2: "429" with rate_limit_error JSON
            if text.contains("429") {
                if let Some(brace_start) = text.find('{') {
                    let json_fragment = &text[brace_start..];
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_fragment) {
                        if let Some(error_msg) = extract_nested_rate_limit_error(&val) {
                            return Some(QuotaExceededInfo {
                                agent_key: agent_key.to_string(),
                                category: infer_quota_category(&error_msg),
                                raw_message: truncate_message(&error_msg, 500),
                            });
                        }
                    }
                }
            }

            // §4.5.1 item 3: plain text "API Error:" + rate limit wording
            if lower.contains("api error:")
                && (lower.contains("rate limit reached")
                    || lower.contains("rate limited")
                    || (lower.contains("request rejected") && lower.contains("429")))
            {
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(text),
                    raw_message: truncate_message(text, 500),
                });
            }

            // §4.5.1 item 4: "You've hit your limit" / "hit your limit" + "reset"
            if (lower.contains("you've hit your limit")
                || lower.contains("you've hit your usage limit"))
                || (lower.contains("hit your limit") && lower.contains("reset"))
            {
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(text),
                    raw_message: truncate_message(text, 500),
                });
            }

            // §4.5.1 item 5: "HTTP 429" or "429" + "rate_limit_error"
            if lower.contains("http 429")
                || (text.contains("429") && lower.contains("rate_limit_error"))
            {
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(text),
                    raw_message: truncate_message(text, 500),
                });
            }

            // §4.5.1 item 6: "Error: 429" prefix + JSON
            if text.starts_with("Error: 429") {
                if let Some(brace_start) = text.find('{') {
                    let json_fragment = &text[brace_start..];
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_fragment) {
                        if let Some(error_msg) = extract_nested_rate_limit_error(&val) {
                            return Some(QuotaExceededInfo {
                                agent_key: agent_key.to_string(),
                                category: infer_quota_category(&error_msg),
                                raw_message: truncate_message(&error_msg, 500),
                            });
                        }
                    }
                }
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(text),
                    raw_message: truncate_message(text, 500),
                });
            }

            // Legacy: "usage limit" or "rate limit" substrings
            if lower.contains("usage limit") || lower.contains("rate limit") {
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(text),
                    raw_message: truncate_message(text, 500),
                });
            }

            None
        }
        AgentEventPayload::JsonLine(val) => {
            // §4.5.1 item 7: type == "error" with nested error.type == "rate_limit_error"
            if val.get("type").and_then(|v| v.as_str()) == Some("error") {
                if let Some(error_msg) = extract_nested_rate_limit_error(val) {
                    return Some(QuotaExceededInfo {
                        agent_key: agent_key.to_string(),
                        category: infer_quota_category(&error_msg),
                        raw_message: truncate_message(&error_msg, 500),
                    });
                }
            }

            // type == "result" with error + usage/limit message
            if val.get("type").and_then(|v| v.as_str()) == Some("result") {
                let is_error = val.get("subtype").and_then(|v| v.as_str()) == Some("error")
                    || val.get("is_error").and_then(|v| v.as_bool()) == Some(true);
                if is_error {
                    if let Some(msg) = val.get("message").and_then(|v| v.as_str()) {
                        let msg_lower = msg.to_lowercase();
                        if msg_lower.contains("usage") || msg_lower.contains("limit") {
                            return Some(QuotaExceededInfo {
                                agent_key: agent_key.to_string(),
                                category: infer_quota_category(msg),
                                raw_message: truncate_message(msg, 500),
                            });
                        }
                    }
                    if let Some(result_str) = val.get("result").and_then(|v| v.as_str()) {
                        let r_lower = result_str.to_lowercase();
                        if r_lower.contains("usage") || r_lower.contains("limit") {
                            return Some(QuotaExceededInfo {
                                agent_key: agent_key.to_string(),
                                category: infer_quota_category(result_str),
                                raw_message: truncate_message(result_str, 500),
                            });
                        }
                    }
                }
            }

            // Log-array shape: [0].error may be a string to re-parse
            if val.is_array() {
                if let Some(first) = val.get(0) {
                    if let Some(error_val) = first.get("error") {
                        if error_val.is_string() {
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(
                                error_val.as_str().unwrap_or(""),
                            ) {
                                if let Some(error_msg) = extract_nested_rate_limit_error(&parsed) {
                                    return Some(QuotaExceededInfo {
                                        agent_key: agent_key.to_string(),
                                        category: infer_quota_category(&error_msg),
                                        raw_message: truncate_message(&error_msg, 500),
                                    });
                                }
                            }
                        } else if let Some(error_msg) = extract_nested_rate_limit_error(error_val) {
                            return Some(QuotaExceededInfo {
                                agent_key: agent_key.to_string(),
                                category: infer_quota_category(&error_msg),
                                raw_message: truncate_message(&error_msg, 500),
                            });
                        }
                    }
                }
            }

            None
        }
        _ => None,
    }
}

fn extract_nested_rate_limit_error(val: &serde_json::Value) -> Option<String> {
    let error_obj = val.get("error")?;
    if error_obj.get("type").and_then(|v| v.as_str()) == Some("rate_limit_error") {
        return error_obj
            .get("message")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if error_obj.get("code").and_then(|v| v.as_str()) == Some("rate_limit_error") {
        return error_obj
            .get("message")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    None
}

fn extract_codex_quota_signal(payload: &AgentEventPayload) -> Option<QuotaExceededInfo> {
    let agent_key = "codex";
    match payload {
        AgentEventPayload::JsonLine(val) => {
            if val.get("type").and_then(|v| v.as_str()) == Some("error") {
                let code_matches =
                    val.get("code").and_then(|v| v.as_str()) == Some("rate_limit_exceeded");
                let msg = val.get("message").and_then(|v| v.as_str()).unwrap_or("");
                let msg_matches = msg.to_lowercase().contains("rate limit");
                if code_matches || msg_matches {
                    let raw = if msg.is_empty() {
                        truncate_message(&val.to_string(), 500)
                    } else {
                        truncate_message(msg, 500)
                    };
                    return Some(QuotaExceededInfo {
                        agent_key: agent_key.to_string(),
                        category: infer_quota_category(msg),
                        raw_message: raw,
                    });
                }
            }
            None
        }
        AgentEventPayload::RawLine(text) => {
            let lower = text.to_lowercase();
            if lower.contains("rate limit reached")
                || lower.contains("tokens per min")
                || lower.contains("429 too many requests")
                || lower.contains("rate_limit_exceeded")
            {
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(text),
                    raw_message: truncate_message(text, 500),
                });
            }
            None
        }
        _ => None,
    }
}

fn extract_gemini_quota_signal(payload: &AgentEventPayload) -> Option<QuotaExceededInfo> {
    let agent_key = "gemini";
    match payload {
        AgentEventPayload::JsonLine(val) => {
            if let Some(error_obj) = find_gemini_error_object(val) {
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(&error_obj),
                    raw_message: truncate_message(&error_obj, 500),
                });
            }
            None
        }
        AgentEventPayload::RawLine(text) => {
            let lower = text.to_lowercase();
            if lower.contains("resource_exhausted") {
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(text),
                    raw_message: truncate_message(text, 500),
                });
            }
            if lower.contains("rate limit exceeded") {
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(text),
                    raw_message: truncate_message(text, 500),
                });
            }
            if text.contains("429")
                && (lower.contains("quota exceeded") || lower.contains("rate limit"))
            {
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(text),
                    raw_message: truncate_message(text, 500),
                });
            }
            // Python-like: "ERROR {'code': 429, ...}"
            if lower.contains("error")
                && text.contains("429")
                && (lower.contains("rate limit") || lower.contains("'code'"))
            {
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(text),
                    raw_message: truncate_message(text, 500),
                });
            }
            None
        }
        _ => None,
    }
}

fn find_gemini_error_object(val: &serde_json::Value) -> Option<String> {
    let error = if val.is_array() {
        val.get(0)?.get("error")
    } else {
        val.get("error")
    };

    let error = error?;

    let code_429 = error.get("code").and_then(|v| v.as_u64()) == Some(429);
    let status_exhausted =
        error.get("status").and_then(|v| v.as_str()) == Some("RESOURCE_EXHAUSTED");

    if !code_429 && !status_exhausted {
        return None;
    }

    let msg = error.get("message").and_then(|v| v.as_str()).unwrap_or("");
    Some(if msg.is_empty() {
        error.to_string()
    } else {
        msg.to_string()
    })
}

fn extract_opencode_quota_signal(payload: &AgentEventPayload) -> Option<QuotaExceededInfo> {
    let agent_key = "opencode";
    match payload {
        AgentEventPayload::JsonLine(val) => {
            if val.get("type").and_then(|v| v.as_str()) == Some("error") {
                // Check nested error.type == "insufficient_quota" or error.code == "insufficient_quota"
                if let Some(error) = val.get("error") {
                    if error.get("type").and_then(|v| v.as_str()) == Some("insufficient_quota")
                        || error.get("code").and_then(|v| v.as_str()) == Some("insufficient_quota")
                    {
                        let msg = error.get("message").and_then(|v| v.as_str()).unwrap_or("");
                        return Some(QuotaExceededInfo {
                            agent_key: agent_key.to_string(),
                            category: infer_quota_category(msg),
                            raw_message: if msg.is_empty() {
                                truncate_message(&val.to_string(), 500)
                            } else {
                                truncate_message(msg, 500)
                            },
                        });
                    }
                }
                // Fallback: message contains quota/rate-limit keywords
                let msg = val.get("message").and_then(|v| v.as_str()).unwrap_or("");
                let msg_lower = msg.to_lowercase();
                if msg_lower.contains("quota")
                    || msg_lower.contains("rate limit")
                    || msg_lower.contains("insufficient_quota")
                    || msg_lower.contains("429")
                {
                    return Some(QuotaExceededInfo {
                        agent_key: agent_key.to_string(),
                        category: infer_quota_category(msg),
                        raw_message: if msg.is_empty() {
                            truncate_message(&val.to_string(), 500)
                        } else {
                            truncate_message(msg, 500)
                        },
                    });
                }
            }
            None
        }
        AgentEventPayload::RawLine(text) => {
            let lower = text.to_lowercase();
            if lower.contains("rate-limited")
                || lower.contains("daily token quota exceeded")
                || (lower.contains("too many requests") && lower.contains("quota exceeded"))
                || lower.contains("insufficient_quota")
            {
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(text),
                    raw_message: truncate_message(text, 500),
                });
            }
            None
        }
        _ => None,
    }
}

fn extract_agent_quota_signal(payload: &AgentEventPayload) -> Option<QuotaExceededInfo> {
    let agent_key = "agent";
    match payload {
        AgentEventPayload::JsonLine(val) => {
            if val.get("type").and_then(|v| v.as_str()) == Some("error") {
                let msg = val.get("message").and_then(|v| v.as_str()).unwrap_or("");
                let msg_lower = msg.to_lowercase();
                if msg_lower.contains("rate limit")
                    || msg_lower.contains("quota exceeded")
                    || msg_lower.contains("usage limit")
                {
                    return Some(QuotaExceededInfo {
                        agent_key: agent_key.to_string(),
                        category: infer_quota_category(msg),
                        raw_message: if msg.is_empty() {
                            truncate_message(&val.to_string(), 500)
                        } else {
                            truncate_message(msg, 500)
                        },
                    });
                }
            }
            None
        }
        AgentEventPayload::RawLine(text) => {
            // structured-log.info JSON
            if text.contains("structured-log.info") {
                if let Some(brace_start) = text.find('{') {
                    let json_fragment = &text[brace_start..];
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_fragment) {
                        if let Some(metadata) = val.get("metadata") {
                            let outcome = metadata.get("outcome").and_then(|v| v.as_str());
                            if outcome == Some("error") {
                                let grpc_code = metadata
                                    .get("grpc_code")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_lowercase();
                                let error_text = metadata
                                    .get("error_text")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_lowercase();
                                if grpc_code.contains("resource_exhausted")
                                    || error_text.contains("usage limit")
                                    || (error_text.contains("limit")
                                        && (error_text.contains("slow pool")
                                            || error_text.contains("opus")))
                                    || grpc_code.contains("resource_exhausted")
                                {
                                    let raw_msg = metadata
                                        .get("error_text")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or(json_fragment);
                                    return Some(QuotaExceededInfo {
                                        agent_key: agent_key.to_string(),
                                        category: infer_quota_category(raw_msg),
                                        raw_message: truncate_message(raw_msg, 500),
                                    });
                                }
                            }
                        }
                    }
                }
            }

            // Plain text: "You've hit your usage limit"
            let lower = text.to_lowercase();
            if lower.contains("you've hit your usage limit") || lower.contains("usage limit for") {
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(text),
                    raw_message: truncate_message(text, 500),
                });
            }

            None
        }
        _ => None,
    }
}

/// Aggregate a sequence of token usage entries using the per-agent rule.
///
/// - **Codex**: sum all entries (multiple `turn.completed` messages)
/// - **All others**: take the last entry (final `result` / `step_finish`)
///
/// Returns `None` when `usage_entries` is empty.
pub fn aggregate_token_usage(
    usage_entries: &[(TokenUsage, UsageSource)],
    source: UsageSource,
) -> Option<TokenUsage> {
    if usage_entries.is_empty() {
        return None;
    }
    match source {
        UsageSource::Codex => {
            let input_tokens = usage_entries.iter().map(|(u, _)| u.input_tokens).sum();
            let output_tokens = usage_entries.iter().map(|(u, _)| u.output_tokens).sum();
            let total_tokens = sum_optional(usage_entries.iter().map(|(u, _)| &u.total_tokens));
            let cache_read_tokens =
                sum_optional(usage_entries.iter().map(|(u, _)| &u.cache_read_tokens));
            let cache_creation_tokens =
                sum_optional(usage_entries.iter().map(|(u, _)| &u.cache_creation_tokens));
            let reasoning_tokens =
                sum_optional(usage_entries.iter().map(|(u, _)| &u.reasoning_tokens));
            Some(TokenUsage {
                input_tokens,
                output_tokens,
                total_tokens,
                cache_read_tokens,
                cache_creation_tokens,
                reasoning_tokens,
            })
        }
        _ => usage_entries.last().map(|(u, _)| u.clone()),
    }
}

/// Shared internal function that spawns a child process with piped stdio.
///
/// Returns the spawned `Child` and the argv used (for diagnostics).
/// `events_mode` selects event-optimized argv builders over the standard ones.
fn spawn_agent_piped(
    agent_key: &str,
    prompt: &str,
    options: &RunOptions,
    events_mode: bool,
) -> Result<(Child, Vec<OsString>), RunError> {
    if !is_runnable(agent_key) {
        return Err(RunError::AgentNotRunnable(agent_key.to_string()));
    }

    let argv = if events_mode {
        match agent_key {
            "codex" => build_codex_argv_events(prompt, options.model.as_ref(), options.yolo),
            "claude" => build_claude_argv_events(prompt, options.model.as_ref(), options.stream),
            "gemini" => build_gemini_argv_events(prompt, options.model.as_ref()),
            "opencode" => build_opencode_argv_events(prompt, options.model.as_ref(), options.yolo),
            "agent" => build_cursor_agent_argv_events(
                prompt,
                options.model.as_ref(),
                options.yolo,
                options.stream,
            ),
            _ => unreachable!(),
        }
    } else {
        match agent_key {
            "codex" => {
                build_codex_argv(prompt, options.model.as_ref(), options.yolo, options.stream)
            }
            "claude" => {
                build_claude_argv(prompt, options.model.as_ref(), options.yolo, options.stream)
            }
            "gemini" => {
                build_gemini_argv(prompt, options.model.as_ref(), options.yolo, options.stream)
            }
            "opencode" => {
                build_opencode_argv(prompt, options.model.as_ref(), options.yolo, options.stream)
            }
            "agent" => build_cursor_agent_argv(
                prompt,
                options.model.as_ref(),
                options.yolo,
                options.stream,
            ),
            _ => unreachable!(),
        }
    };

    let argv_display: Vec<String> = argv
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    tracing::debug!(
        target: "aikit_sdk::runner",
        agent_key = %agent_key,
        argv = ?argv_display,
        cwd = ?options.current_dir.as_ref().map(|p| p.display().to_string()),
        timeout = ?options.timeout.map(|d| format!("{}s", d.as_secs())),
        events_mode,
        yolo = options.yolo,
        stream = options.stream,
        write_prompt_to_stdin = should_write_stdin(agent_key),
        "spawning agent child process"
    );

    let binary = &argv[0];
    let args = &argv[1..];

    let resolved_program = crate::command_resolve::resolve_command(&binary.to_string_lossy());
    tracing::debug!(
        target: "aikit_sdk::runner",
        resolved_program = ?resolved_program,
        "resolved executable path"
    );
    let mut cmd = Command::new(resolved_program);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(ref dir) = options.current_dir {
        cmd.current_dir(dir);
    }

    let child = cmd.spawn().map_err(RunError::SpawnFailed)?;

    Ok((child, argv))
}

/// Runs an agent with the given prompt and options.
pub fn run_agent(
    agent_key: &str,
    prompt: &str,
    options: RunOptions,
) -> Result<RunResult, RunError> {
    let result = run_agent_events(agent_key, prompt, options, |_event| {})?;
    if let Some(info) = result.quota_exceeded {
        return Err(RunError::QuotaExceeded(info));
    }
    Ok(result)
}

/// Spawns a reader thread that reads lines (delimited by `\n`) from `reader`
/// and sends raw byte chunks (including the newline) to `tx`.
/// Non-UTF-8 and partial final lines are sent as-is.
/// I/O errors are sent as `ReaderMsg::Err` and the thread exits.
fn spawn_reader_thread<R>(
    reader: R,
    stream: AgentEventStream,
    tx: mpsc::Sender<ReaderMsg>,
) -> thread::JoinHandle<()>
where
    R: io::Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = io::BufReader::new(reader);
        let mut buf: Vec<u8> = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if tx
                        .send(ReaderMsg::Chunk {
                            stream: stream.clone(),
                            raw: buf.clone(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(ReaderMsg::Err { stream, source: e });
                    break;
                }
            }
        }
    })
}

/// Parses a raw byte chunk into an `AgentEventPayload`.
///
/// Strips a trailing CRLF or LF before attempting UTF-8 and JSON decode.
/// The raw bytes (including newline) are accumulated separately by the caller.
fn parse_payload(raw: &[u8]) -> AgentEventPayload {
    let stripped = raw
        .strip_suffix(b"\r\n")
        .or_else(|| raw.strip_suffix(b"\n"))
        .unwrap_or(raw);

    match std::str::from_utf8(stripped) {
        Ok(s) => match serde_json::from_str::<serde_json::Value>(s) {
            Ok(v) => AgentEventPayload::JsonLine(v),
            Err(_) => AgentEventPayload::RawLine(s.to_string()),
        },
        Err(_) => AgentEventPayload::RawBytes(stripped.to_vec()),
    }
}

/// Runs an agent with the given prompt and options, delivering events via
/// `on_event` callback as output lines are produced.
///
/// The final `RunResult` accumulates identical bytes to what `run_agent`
/// would produce. Callback panics are isolated and reported as
/// `RunError::CallbackPanic`. Reader I/O failures are reported as
/// `RunError::ReaderFailed`. If `options.timeout` is set and the child
/// exceeds it, the child is killed and `RunError::TimedOut` is returned
/// with the partial output collected so far.
pub fn run_agent_events<F>(
    agent_key: &str,
    prompt: &str,
    options: RunOptions,
    mut on_event: F,
) -> Result<RunResult, RunError>
where
    F: FnMut(AgentEvent) + Send,
{
    use std::sync::{Arc, Mutex};

    tracing::debug!(
        target: "aikit_sdk::runner",
        agent_key = %agent_key,
        prompt_len = prompt.len(),
        timeout = ?options.timeout.map(|d| d.as_secs()),
        stream = options.stream,
        yolo = options.yolo,
        "run_agent_events"
    );

    let (mut child, _argv) = spawn_agent_piped(agent_key, prompt, &options, true)?;

    // Write prompt and close stdin before reading output.
    // Cursor Agent ("agent") takes the prompt as a positional argument, so
    // we do not write stdin; we still must `take()` and drop it so the write
    // end of the pipe closes. Otherwise the child's stdin stays open and some
    // agents block reading it when stdout is a pipe (non-TTY).
    if should_write_stdin(agent_key) {
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .map_err(RunError::StdinFailed)?;
        }
    } else {
        drop(child.stdin.take());
    }

    let stdout_pipe = child.stdout.take().expect("stdout was piped");
    let stderr_pipe = child.stderr.take().expect("stderr was piped");

    // Wrap child in Arc<Mutex> so the watchdog thread can call child.kill()
    // while the main thread retains access for child.wait() after the loop.
    let child = Arc::new(Mutex::new(child));

    let (tx, rx) = mpsc::channel::<ReaderMsg>();

    let stdout_thread = spawn_reader_thread(stdout_pipe, AgentEventStream::Stdout, tx.clone());
    let stderr_thread = spawn_reader_thread(stderr_pipe, AgentEventStream::Stderr, tx.clone());

    // Watchdog: if timeout is configured, spawn a dedicated thread. It blocks
    // on a cancel channel for the configured duration. On timeout it kills the
    // child and sets the `killed` flag. The kill causes pipe EOF → reader
    // threads finish → tx senders drop → rx closes → drain loop exits.
    //
    // The watchdog does NOT hold a tx sender. This avoids a deadlock where the
    // watchdog's tx would keep rx alive on the natural-exit path (blocking the
    // drain loop until the full timeout elapses).
    let killed = Arc::new(AtomicBool::new(false));
    let watchdog_cancel: Option<mpsc::Sender<()>> = if let Some(timeout_duration) = options.timeout
    {
        let (cancel_tx, cancel_rx) = mpsc::channel::<()>();
        let child_watchdog = Arc::clone(&child);
        let killed_watchdog = Arc::clone(&killed);
        thread::spawn(move || {
            match cancel_rx.recv_timeout(timeout_duration) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // Natural exit signaled or cancel_tx dropped — do nothing.
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Timeout elapsed: mark killed, then kill child.
                    // The kill closes pipes → reader threads get EOF and exit →
                    // rx drains → main thread checks `killed` flag after loop.
                    killed_watchdog.store(true, Ordering::SeqCst);
                    let _ = child_watchdog.lock().unwrap().kill();
                }
            }
        });
        Some(cancel_tx)
    } else {
        None
    };

    // Drop our extra tx so rx closes when reader threads finish.
    drop(tx);

    let mut seq: u64 = 0;
    let mut stdout_bytes: Vec<u8> = Vec::new();
    let mut stderr_bytes: Vec<u8> = Vec::new();
    let mut reader_error: Option<RunError> = None;
    let mut callback_panic: Option<Box<dyn std::any::Any + Send>> = None;
    let mut usage_entries: Vec<(TokenUsage, UsageSource)> = Vec::new();
    let mut quota_exceeded: Option<QuotaExceededInfo> = None;

    for msg in rx {
        match msg {
            ReaderMsg::Chunk { stream, raw } => {
                // Accumulate raw bytes verbatim.
                match stream {
                    AgentEventStream::Stdout => stdout_bytes.extend_from_slice(&raw),
                    AgentEventStream::Stderr => stderr_bytes.extend_from_slice(&raw),
                }

                let payload = parse_payload(&raw);

                // Always extract token usage for RunResult.token_usage aggregation.
                let extracted_usage = if let AgentEventPayload::JsonLine(ref json_val) = payload {
                    extract_usage_from_line(json_val, agent_key)
                } else {
                    None
                };
                if let Some(ref up) = extracted_usage {
                    usage_entries.push(up.clone());
                }

                let json_line_seq = seq;
                let event_stream = stream.clone();
                let quota_signal = extract_quota_signal(agent_key, &payload);
                let event = AgentEvent {
                    agent_key: agent_key.to_string(),
                    seq,
                    stream,
                    payload,
                };
                seq += 1;

                // Isolate callback panics; stop calling after first panic.
                if callback_panic.is_none() {
                    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                        on_event(event);
                    }));
                    if let Err(p) = result {
                        callback_panic = Some(p);
                    }
                }

                // Emit TokenUsageLine event immediately after the JsonLine, if enabled.
                if options.emit_token_usage_events {
                    if let Some((usage, source)) = extracted_usage {
                        let token_event = AgentEvent {
                            agent_key: agent_key.to_string(),
                            seq,
                            stream: event_stream.clone(),
                            payload: AgentEventPayload::TokenUsageLine {
                                usage,
                                source,
                                raw_agent_line_seq: json_line_seq,
                            },
                        };
                        seq += 1;
                        if callback_panic.is_none() {
                            let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                                on_event(token_event);
                            }));
                            if let Err(p) = result {
                                callback_panic = Some(p);
                            }
                        }
                    }
                }

                if let Some(info) = quota_signal {
                    if quota_exceeded.is_none() {
                        quota_exceeded = Some(info.clone());
                    }
                    let quota_event = AgentEvent {
                        agent_key: agent_key.to_string(),
                        seq,
                        stream: event_stream,
                        payload: AgentEventPayload::QuotaExceeded {
                            info,
                            raw_agent_line_seq: json_line_seq,
                        },
                    };
                    seq += 1;
                    if callback_panic.is_none() {
                        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                            on_event(quota_event);
                        }));
                        if let Err(p) = result {
                            callback_panic = Some(p);
                        }
                    }
                }
            }
            ReaderMsg::Err { stream, source } => {
                if reader_error.is_none() {
                    reader_error = Some(RunError::ReaderFailed { stream, source });
                }
            }
        }
    }

    // Signal watchdog on natural exit path (no-op if it already fired).
    if let Some(cancel_tx) = watchdog_cancel {
        let _ = cancel_tx.send(());
    }

    // Join reader threads before wait() to prevent pipe deadlock.
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    let timed_out = killed.load(Ordering::SeqCst);

    if timed_out {
        // child.wait() reaps the zombie even after kill(); ignore status.
        let _ = child.lock().unwrap().wait();
        return Err(RunError::TimedOut {
            timeout: options.timeout.unwrap(),
            stdout: stdout_bytes,
            stderr: stderr_bytes,
        });
    }

    let status = child
        .lock()
        .unwrap()
        .wait()
        .map_err(RunError::OutputFailed)?;

    if let Some(p) = callback_panic {
        let _ = child.lock().unwrap().kill();
        return Err(RunError::CallbackPanic(p));
    }

    if let Some(err) = reader_error {
        let _ = child.lock().unwrap().kill();
        return Err(err);
    }

    // Aggregate token usage from all extracted entries.
    let token_usage = usage_entries
        .first()
        .map(|(_, source)| source.clone())
        .and_then(|source| aggregate_token_usage(&usage_entries, source));

    Ok(RunResult {
        status,
        stdout: stdout_bytes,
        stderr: stderr_bytes,
        token_usage,
        quota_exceeded,
    })
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

/// Timeout for agent availability probing in milliseconds.
const PROBE_TIMEOUT_MS: u64 = 1500;

/// Gets the binary candidates for an agent key.
fn get_binary_candidates(agent_key: &str) -> &'static [&'static str] {
    match agent_key {
        "codex" => &["codex"],
        "claude" => &["claude"],
        "gemini" => &["gemini"],
        "opencode" => &["opencode", "opencode-desktop"],
        "agent" => &["agent"],
        _ => &[],
    }
}

/// Probes a binary with a --version check under timeout.
///
/// Returns Ok(true) if binary responds successfully to --version,
/// Ok(false) if binary exists but --version fails,
/// Err if binary not found or timeout occurs.
fn probe_binary_with_timeout(binary: &str) -> Result<bool, AgentAvailabilityReason> {
    let resolved_binary = crate::command_resolve::resolve_command(binary);
    let mut cmd = Command::new(resolved_binary);
    cmd.arg("--version");
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    let mut child = cmd
        .spawn()
        .map_err(|_| AgentAvailabilityReason::BinaryNotFound)?;

    let timeout = Duration::from_millis(PROBE_TIMEOUT_MS);

    match child.wait_timeout(timeout) {
        Ok(Some(status)) => Ok(status.success()),
        Ok(None) => {
            let _ = child.kill();
            Err(AgentAvailabilityReason::TimedOut)
        }
        Err(_) => Err(AgentAvailabilityReason::BinaryNotFound),
    }
}

/// Checks if an agent is available (installed and responds to --version).
///
/// Returns false for non-runnable agents.
/// For runnable agents, probes each binary candidate and returns true
/// if any responds successfully to --version.
pub fn is_agent_available(agent_key: &str) -> bool {
    if !is_runnable(agent_key) {
        return false;
    }

    let candidates = get_binary_candidates(agent_key);
    for binary in candidates {
        if probe_binary_with_timeout(binary).unwrap_or(false) {
            return true;
        }
    }

    false
}

/// Gets the list of installed and available runnable agents.
///
/// Returns sorted list of agent keys that are runnable and available.
pub fn get_installed_agents() -> Vec<String> {
    let mut agents: Vec<String> = runnable_agents()
        .iter()
        .filter(|&&key| is_agent_available(key))
        .map(|s| s.to_string())
        .collect();
    agents.sort();
    agents
}

/// Gets the status for all runnable agents.
///
/// Returns BTreeMap for stable ordering. Includes all runnable agents
/// with their availability status and reason if unavailable.
pub fn get_agent_status() -> BTreeMap<String, AgentStatus> {
    let mut status = BTreeMap::new();

    for &agent_key in runnable_agents() {
        if !is_runnable(agent_key) {
            status.insert(
                agent_key.to_string(),
                AgentStatus::unavailable(AgentAvailabilityReason::NotRunnable),
            );
            continue;
        }

        let candidates = get_binary_candidates(agent_key);
        let mut available = false;
        let mut last_error = AgentAvailabilityReason::BinaryNotFound;

        for binary in candidates {
            match probe_binary_with_timeout(binary) {
                Ok(true) => {
                    available = true;
                    break;
                }
                Ok(false) => {
                    last_error = AgentAvailabilityReason::VersionCheckFailed;
                }
                Err(e) => {
                    last_error = e;
                }
            }
        }

        if available {
            status.insert(agent_key.to_string(), AgentStatus::available());
        } else {
            status.insert(agent_key.to_string(), AgentStatus::unavailable(last_error));
        }
    }

    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;

    #[test]
    fn test_runnable_agents_includes_codex_claude_gemini_opencode_agent() {
        let agents = runnable_agents();
        assert!(agents.contains(&"codex"));
        assert!(agents.contains(&"claude"));
        assert!(agents.contains(&"gemini"));
        assert!(agents.contains(&"opencode"));
        assert!(agents.contains(&"agent"));
        assert_eq!(agents.len(), 5);
    }

    #[test]
    fn test_is_runnable_true_for_supported_false_for_others() {
        assert!(is_runnable("codex"));
        assert!(is_runnable("claude"));
        assert!(is_runnable("gemini"));
        assert!(is_runnable("opencode"));
        assert!(is_runnable("agent"));
        assert!(!is_runnable("copilot"));
        assert!(!is_runnable("cursor-agent"));
        assert!(!is_runnable("unknown"));
    }

    #[test]
    fn test_build_codex_argv_contains_exec_and_model() {
        let argv = build_codex_argv("test prompt", Some(&"gpt-4".to_string()), true, false);
        assert!(argv.contains(&OsString::from("codex")));
        assert!(argv.contains(&OsString::from("exec")));
        assert!(argv.contains(&OsString::from("-m")));
        assert!(argv.contains(&OsString::from("gpt-4")));
        assert!(argv.contains(&OsString::from("--yolo")));
        assert!(argv.contains(&OsString::from("--json")));
    }

    #[test]
    fn test_build_codex_argv_no_model() {
        let argv = build_codex_argv("test prompt", None, false, false);
        assert!(!argv.contains(&OsString::from("-m")));
        assert!(!argv.contains(&OsString::from("--yolo")));
        assert!(argv.contains(&OsString::from("--json")));
    }

    #[test]
    fn test_build_claude_argv_contains_prompt_and_model() {
        let argv = build_claude_argv(
            "test prompt",
            Some(&"claude-3-opus".to_string()),
            false,
            true,
        );
        assert!(argv.contains(&OsString::from("claude")));
        assert!(argv.contains(&OsString::from("-p")));
        assert!(argv.contains(&OsString::from("test prompt")));
        assert!(argv.contains(&OsString::from("--model")));
        assert!(argv.contains(&OsString::from("claude-3-opus")));
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("stream-json")));
    }

    #[test]
    fn test_build_claude_argv_text_format() {
        let argv = build_claude_argv("test prompt", None, false, false);
        assert!(argv.contains(&OsString::from("text")));
        assert!(!argv.contains(&OsString::from("stream-json")));
    }

    #[test]
    fn test_build_gemini_argv_contains_prompt_and_model() {
        let argv = build_gemini_argv("test prompt", Some(&"gemini-pro".to_string()), false, false);
        assert!(argv.contains(&OsString::from("gemini")));
        assert!(argv.contains(&OsString::from("--prompt")));
        assert!(argv.contains(&OsString::from("test prompt")));
        assert!(argv.contains(&OsString::from("--model")));
        assert!(argv.contains(&OsString::from("gemini-pro")));
    }

    #[test]
    fn test_build_opencode_argv_contains_prompt_and_model() {
        let argv = build_opencode_argv(
            "test prompt",
            Some(&"zai-coding-plan/glm-4.7".to_string()),
            true,
            false,
        );
        assert!(argv.contains(&OsString::from("opencode")));
        assert!(argv.contains(&OsString::from("--prompt")));
        assert!(argv.contains(&OsString::from("test prompt")));
        assert!(argv.contains(&OsString::from("--model")));
        assert!(argv.contains(&OsString::from("zai-coding-plan/glm-4.7")));
        assert!(argv.contains(&OsString::from("--yolo")));
    }

    #[test]
    fn test_build_opencode_argv_no_options() {
        let argv = build_opencode_argv("test prompt", None, false, false);
        assert!(!argv.contains(&OsString::from("--yolo")));
    }

    #[test]
    fn test_build_agent_argv_contains_all_options() {
        let argv =
            build_cursor_agent_argv("test prompt", Some(&"custom-model".to_string()), true, true);
        assert!(argv.contains(&OsString::from("agent")));
        assert!(argv.contains(&OsString::from("--print")));
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("json")));
        assert!(argv.contains(&OsString::from("--model")));
        assert!(argv.contains(&OsString::from("custom-model")));
        assert!(argv.contains(&OsString::from("--force")));
        assert!(argv.contains(&OsString::from("test prompt")));
        assert!(!argv.contains(&OsString::from("--prompt")));
        assert!(!argv.contains(&OsString::from("--yolo")));
    }

    #[test]
    fn test_run_agent_not_runnable() {
        let result = run_agent("unknown", "test", RunOptions::default());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, RunError::AgentNotRunnable(_)));
    }

    #[test]
    fn test_run_options_builder() {
        let options = RunOptions::new()
            .with_model("test-model")
            .with_yolo(true)
            .with_stream(true);

        assert_eq!(options.model, Some("test-model".to_string()));
        assert!(options.yolo);
        assert!(options.stream);
    }

    #[test]
    fn test_run_result_success() {
        let stdout = b"output".to_vec();
        let stderr = b"".to_vec();
        let result = RunResult::new(ExitStatus::from_raw(0), stdout, stderr);

        assert!(result.success());
        assert_eq!(result.exit_code(), Some(0));
    }

    #[test]
    fn test_run_result_failure() {
        let stdout = b"".to_vec();
        let stderr = b"error".to_vec();
        // Unix wait status encodes exit code 1 as 256; Windows uses the code directly.
        #[cfg(unix)]
        let status = ExitStatus::from_raw(256);
        #[cfg(windows)]
        let status = ExitStatus::from_raw(1);
        let result = RunResult::new(status, stdout, stderr);

        assert!(!result.success());
        assert_eq!(result.exit_code(), Some(1));
    }

    #[test]
    fn test_run_error_display() {
        let err = RunError::AgentNotRunnable("unknown".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("not runnable"));
        assert!(msg.contains("codex, claude, gemini, opencode, agent"));
    }

    #[test]
    fn test_is_runnable_case_sensitive() {
        assert!(is_runnable("codex"));
        assert!(!is_runnable("Codex"));
        assert!(!is_runnable("CODEX"));
    }

    #[test]
    fn test_is_agent_available_false_for_non_runnable() {
        assert!(!is_agent_available("copilot"));
        assert!(!is_agent_available("cursor-agent"));
        assert!(!is_agent_available("unknown"));
    }

    #[test]
    fn test_get_agent_status_keys_match_runnable_agents() {
        let status = get_agent_status();
        let runnable_set: std::collections::HashSet<_> =
            runnable_agents().iter().copied().collect();
        let status_keys: std::collections::HashSet<_> = status.keys().map(|s| s.as_str()).collect();
        assert_eq!(runnable_set, status_keys);
    }

    #[test]
    fn test_get_installed_agents_is_subset_of_runnable_agents() {
        let installed = get_installed_agents();
        let runnable_set: std::collections::HashSet<_> =
            runnable_agents().iter().copied().collect();
        for agent in &installed {
            assert!(runnable_set.contains(agent.as_str()));
        }
    }

    #[test]
    fn test_get_installed_agents_sorted() {
        let installed = get_installed_agents();
        let mut sorted_installed = installed.clone();
        sorted_installed.sort();
        assert_eq!(installed, sorted_installed);
    }

    #[test]
    fn test_unavailable_statuses_have_reason() {
        let status = get_agent_status();
        for (agent_key, agent_status) in &status {
            if !agent_status.available {
                assert!(
                    agent_status.reason.is_some(),
                    "Agent {} is unavailable but has no reason",
                    agent_key
                );
            }
        }
    }

    #[test]
    fn test_binary_candidates_mapping() {
        assert_eq!(get_binary_candidates("codex"), &["codex"] as &[&str]);
        assert_eq!(get_binary_candidates("claude"), &["claude"]);
        assert_eq!(get_binary_candidates("gemini"), &["gemini"]);
        assert_eq!(
            get_binary_candidates("opencode"),
            &["opencode", "opencode-desktop"]
        );
        assert_eq!(get_binary_candidates("agent"), &["agent"]);
        assert!(get_binary_candidates("unknown").is_empty());
    }

    #[test]
    fn test_agent_status_available() {
        let status = AgentStatus::available();
        assert!(status.available);
        assert!(status.reason.is_none());
    }

    #[test]
    fn test_agent_status_unavailable() {
        let status = AgentStatus::unavailable(AgentAvailabilityReason::BinaryNotFound);
        assert!(!status.available);
        assert_eq!(status.reason, Some(AgentAvailabilityReason::BinaryNotFound));
    }

    #[test]
    fn test_agent_availability_reason_display() {
        assert_eq!(
            format!("{}", AgentAvailabilityReason::NotRunnable),
            "not_runnable"
        );
        assert_eq!(
            format!("{}", AgentAvailabilityReason::BinaryNotFound),
            "binary_not_found"
        );
        assert_eq!(
            format!("{}", AgentAvailabilityReason::VersionCheckFailed),
            "version_check_failed"
        );
        assert_eq!(
            format!("{}", AgentAvailabilityReason::TimedOut),
            "timed_out"
        );
    }

    // --- Streaming API tests ---

    /// Global mutex to serialize tests that mutate PATH via std::env::set_var.
    /// Parallel test threads racing on PATH can cause stub lookup to find the
    /// real binary instead of the temp-dir stub, producing spurious events.
    #[cfg(unix)]
    static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_run_agent_events_not_runnable() {
        let result = run_agent_events("unknown", "test", RunOptions::default(), |_| {});
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RunError::AgentNotRunnable(_)));
    }

    #[test]
    fn test_parse_payload_json_line() {
        let raw = b"{\"key\": \"value\"}\n";
        let payload = parse_payload(raw);
        assert!(matches!(payload, AgentEventPayload::JsonLine(_)));
    }

    #[test]
    fn test_parse_payload_raw_line() {
        let raw = b"just some text\n";
        let payload = parse_payload(raw);
        if let AgentEventPayload::RawLine(s) = payload {
            assert_eq!(s, "just some text");
        } else {
            panic!("Expected RawLine");
        }
    }

    #[test]
    fn test_parse_payload_crlf_normalized() {
        let raw = b"just some text\r\n";
        let payload = parse_payload(raw);
        if let AgentEventPayload::RawLine(s) = payload {
            assert_eq!(s, "just some text");
        } else {
            panic!("Expected RawLine with CRLF stripped");
        }
    }

    #[test]
    fn test_parse_payload_raw_bytes_non_utf8() {
        let raw = vec![0xff, 0xfe, 0x00, b'\n'];
        let payload = parse_payload(&raw);
        assert!(matches!(payload, AgentEventPayload::RawBytes(_)));
    }

    #[test]
    fn test_parse_payload_empty_json_object() {
        let raw = b"{}\n";
        let payload = parse_payload(raw);
        assert!(matches!(payload, AgentEventPayload::JsonLine(_)));
    }

    #[test]
    fn test_parse_payload_no_newline() {
        let raw = b"incomplete line";
        let payload = parse_payload(raw);
        if let AgentEventPayload::RawLine(s) = payload {
            assert_eq!(s, "incomplete line");
        } else {
            panic!("Expected RawLine");
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_run_agent_events_with_echo_stub() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        // Create a stub script that outputs two JSON lines then exits
        let dir = tempfile::tempdir().unwrap();
        let stub_path = dir.path().join("agent");
        let mut f = std::fs::File::create(&stub_path).unwrap();
        writeln!(
            f,
            "#!/bin/sh\necho '{{\"msg\":\"line1\"}}'\necho '{{\"msg\":\"line2\"}}'"
        )
        .unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
        drop(f);

        // Prepend dir to PATH
        let orig_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), orig_path);
        std::env::set_var("PATH", &new_path);

        let mut events: Vec<AgentEvent> = Vec::new();
        let result = run_agent_events("agent", "hello", RunOptions::default(), |ev| {
            events.push(ev)
        });

        std::env::set_var("PATH", orig_path);

        assert!(
            result.is_ok(),
            "run_agent_events should succeed: {:?}",
            result.err()
        );
        assert_eq!(events.len(), 2, "Should have received 2 events");
        assert_eq!(events[0].seq, 0);
        assert_eq!(events[1].seq, 1);
        assert!(matches!(events[0].payload, AgentEventPayload::JsonLine(_)));
        assert!(matches!(events[1].payload, AgentEventPayload::JsonLine(_)));
    }

    #[cfg(unix)]
    #[test]
    fn test_run_agent_events_sequence_numbers_strictly_increasing() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let stub_path = dir.path().join("codex");
        let mut f = std::fs::File::create(&stub_path).unwrap();
        writeln!(f, "#!/bin/sh\necho 'a'\necho 'b'\necho 'c' >&2\necho 'd'").unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
        drop(f);

        let orig_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), orig_path);
        std::env::set_var("PATH", &new_path);

        let mut seqs: Vec<u64> = Vec::new();
        let result = run_agent_events("codex", "hi", RunOptions::default(), |ev| seqs.push(ev.seq));

        std::env::set_var("PATH", orig_path);

        assert!(result.is_ok());
        // Sequence numbers must be strictly increasing
        for w in seqs.windows(2) {
            assert!(w[1] > w[0], "seq {} should be > {}", w[1], w[0]);
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_run_agent_events_callback_panic_isolated() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let stub_path = dir.path().join("gemini");
        let mut f = std::fs::File::create(&stub_path).unwrap();
        writeln!(f, "#!/bin/sh\necho 'line1'\necho 'line2'").unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
        drop(f);

        let orig_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), orig_path);
        std::env::set_var("PATH", &new_path);

        let result = run_agent_events("gemini", "hi", RunOptions::default(), |_ev| {
            panic!("test panic")
        });

        std::env::set_var("PATH", orig_path);

        assert!(
            matches!(result, Err(RunError::CallbackPanic(_))),
            "Expected CallbackPanic, got {:?}",
            result.map(|_| ())
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_run_agent_events_empty_output() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let stub_path = dir.path().join("opencode");
        let mut f = std::fs::File::create(&stub_path).unwrap();
        writeln!(f, "#!/bin/sh\nexit 0").unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
        drop(f);

        let orig_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), orig_path);
        std::env::set_var("PATH", &new_path);

        let mut event_count = 0usize;
        let result = run_agent_events("opencode", "hi", RunOptions::default(), |_ev| {
            event_count += 1
        });

        std::env::set_var("PATH", orig_path);

        assert!(result.is_ok());
        assert_eq!(event_count, 0, "Empty output should produce zero events");
    }

    #[cfg(unix)]
    #[test]
    fn test_run_agent_events_mixed_json_and_raw() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let stub_path = dir.path().join("claude");
        let mut f = std::fs::File::create(&stub_path).unwrap();
        writeln!(
            f,
            "#!/bin/sh\necho '{{\"type\":\"text\"}}'\necho 'plain text'"
        )
        .unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
        drop(f);

        let orig_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), orig_path);
        std::env::set_var("PATH", &new_path);

        let mut payloads: Vec<String> = Vec::new();
        let result = run_agent_events("claude", "hi", RunOptions::default(), |ev| {
            let kind = match &ev.payload {
                AgentEventPayload::JsonLine(_) => "json",
                AgentEventPayload::RawLine(_) => "raw",
                AgentEventPayload::RawBytes(_) => "bytes",
                AgentEventPayload::TokenUsageLine { .. } => "token_usage",
                AgentEventPayload::QuotaExceeded { .. } => "quota_exceeded",
            };
            payloads.push(kind.to_string());
        });

        std::env::set_var("PATH", orig_path);

        assert!(result.is_ok());
        assert_eq!(payloads, vec!["json", "raw"]);
    }

    #[test]
    fn test_build_codex_argv_events_has_json_flag() {
        let argv = build_codex_argv_events("test", Some(&"gpt-4".to_string()), true);
        assert!(argv.contains(&OsString::from("--json")));
        assert!(argv.contains(&OsString::from("--yolo")));
        assert!(argv.contains(&OsString::from("-m")));
    }

    #[test]
    fn test_build_claude_argv_events_json_format() {
        let argv = build_claude_argv_events("test", None, false);
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("json")));
    }

    #[test]
    fn test_build_claude_argv_events_stream_json_format() {
        let argv = build_claude_argv_events("test", None, true);
        assert!(argv.contains(&OsString::from("stream-json")));
    }

    #[test]
    fn test_build_gemini_argv_events_stream_json_headless() {
        let argv = build_gemini_argv_events("test", None);
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("stream-json")));
        assert!(argv.contains(&OsString::from("--yolo")));
        assert!(!argv.contains(&OsString::from("--json")));
    }

    #[test]
    fn test_build_opencode_argv_events_uses_run_subcommand() {
        let argv = build_opencode_argv_events("test prompt", None, false);
        assert!(argv.contains(&OsString::from("opencode")));
        assert!(argv.contains(&OsString::from("run")));
        assert!(argv.contains(&OsString::from("test prompt")));
        assert!(argv.contains(&OsString::from("--format")));
        assert!(argv.contains(&OsString::from("json")));
        assert!(!argv.contains(&OsString::from("--json")));
        assert!(!argv.contains(&OsString::from("--prompt")));
    }

    #[test]
    fn test_build_opencode_argv_events_with_model() {
        let model = "zai-coding-plan/glm-4.7".to_string();
        let argv = build_opencode_argv_events("test", Some(&model), false);
        assert!(argv.contains(&OsString::from("-m")));
        assert!(argv.contains(&OsString::from("zai-coding-plan/glm-4.7")));
        // -m must appear before "run"
        let m_pos = argv.iter().position(|a| a == "-m").unwrap();
        let run_pos = argv.iter().position(|a| a == "run").unwrap();
        assert!(m_pos < run_pos);
    }

    #[test]
    fn test_build_agent_argv_events_has_json_flag() {
        let argv = build_cursor_agent_argv_events("test", None, false, false);
        assert!(argv.contains(&OsString::from("--print")));
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("json")));
        assert!(!argv.contains(&OsString::from("--json")));
    }

    #[test]
    fn test_build_cursor_agent_argv_events_stream_json() {
        let argv = build_cursor_agent_argv_events("test", None, false, true);
        assert!(argv.contains(&OsString::from("stream-json")));
        assert!(argv.contains(&OsString::from("test")));
    }

    #[test]
    fn test_should_write_stdin() {
        assert!(!should_write_stdin("agent"));
        assert!(!should_write_stdin("opencode"));
        assert!(should_write_stdin("codex"));
        assert!(should_write_stdin("claude"));
        assert!(should_write_stdin("gemini"));
    }

    #[test]
    fn test_run_error_callback_panic_display() {
        // Cannot easily construct a CallbackPanic without a real panic, so just
        // test the other new variant.
        use std::io;
        let err = RunError::ReaderFailed {
            stream: AgentEventStream::Stdout,
            source: io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("Stdout") || msg.contains("stdout"));
    }

    #[test]
    fn test_agent_event_stream_eq() {
        assert_eq!(AgentEventStream::Stdout, AgentEventStream::Stdout);
        assert_ne!(AgentEventStream::Stdout, AgentEventStream::Stderr);
    }

    // --- RunOptions new fields ---

    #[test]
    fn test_run_options_default_has_no_timeout_or_current_dir() {
        let opts = RunOptions::default();
        assert!(opts.timeout.is_none());
        assert!(opts.current_dir.is_none());
    }

    #[test]
    fn test_run_options_with_timeout_builder() {
        let dur = Duration::from_secs(30);
        let opts = RunOptions::new().with_timeout(dur);
        assert_eq!(opts.timeout, Some(dur));
    }

    #[test]
    fn test_run_options_with_current_dir_builder() {
        let path = std::path::PathBuf::from("/tmp");
        let opts = RunOptions::new().with_current_dir(path.clone());
        assert_eq!(opts.current_dir, Some(path));
    }

    #[test]
    fn test_run_error_timed_out_display() {
        let err = RunError::TimedOut {
            timeout: Duration::from_secs(5),
            stdout: b"partial".to_vec(),
            stderr: vec![],
        };
        let msg = format!("{}", err);
        assert!(msg.contains("timed out") || msg.contains("timeout") || msg.contains("5"));
    }

    #[test]
    fn test_run_error_timed_out_partial_output() {
        let stdout = b"partial output".to_vec();
        let stderr = b"err output".to_vec();
        let err = RunError::TimedOut {
            timeout: Duration::from_millis(100),
            stdout: stdout.clone(),
            stderr: stderr.clone(),
        };
        if let RunError::TimedOut {
            stdout: out,
            stderr: err_bytes,
            ..
        } = err
        {
            assert_eq!(out, stdout);
            assert_eq!(err_bytes, stderr);
        } else {
            panic!("Expected TimedOut");
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_run_agent_events_timeout_kills_child() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        // Create a stub that sleeps for a long time
        let dir = tempfile::tempdir().unwrap();
        let stub_path = dir.path().join("agent");
        let mut f = std::fs::File::create(&stub_path).unwrap();
        writeln!(
            f,
            "#!/bin/sh\necho 'before sleep'\nsleep 60\necho 'after sleep'"
        )
        .unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
        drop(f);

        let orig_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), orig_path);
        std::env::set_var("PATH", &new_path);

        let opts = RunOptions::new().with_timeout(Duration::from_millis(500));
        let result = run_agent_events("agent", "hi", opts, |_| {});

        std::env::set_var("PATH", orig_path);

        assert!(
            matches!(result, Err(RunError::TimedOut { .. })),
            "Expected TimedOut, got {:?}",
            result.map(|_| ())
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_run_agent_events_no_timeout_on_fast_exit() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        // Stub exits immediately — long timeout should not fire
        let dir = tempfile::tempdir().unwrap();
        let stub_path = dir.path().join("agent");
        let mut f = std::fs::File::create(&stub_path).unwrap();
        writeln!(f, "#!/bin/sh\necho 'done'\nexit 0").unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
        drop(f);

        let orig_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), orig_path);
        std::env::set_var("PATH", &new_path);

        let opts = RunOptions::new().with_timeout(Duration::from_secs(60));
        let result = run_agent_events("agent", "hi", opts, |_| {});

        std::env::set_var("PATH", orig_path);

        assert!(
            result.is_ok(),
            "Fast exit with long timeout should succeed: {:?}",
            result.err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_current_dir_applied_to_child() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        // Stub that prints cwd to stdout
        let stub_dir = tempfile::tempdir().unwrap();
        let stub_path = stub_dir.path().join("agent");
        let mut f = std::fs::File::create(&stub_path).unwrap();
        writeln!(f, "#!/bin/sh\npwd").unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
        drop(f);

        // Use a known target directory (e.g. /tmp)
        let target_dir = std::path::PathBuf::from("/tmp");
        let parent_cwd = std::env::current_dir().unwrap();

        let orig_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", stub_dir.path().display(), orig_path);
        std::env::set_var("PATH", &new_path);

        let mut collected_output = Vec::<u8>::new();
        let opts = RunOptions::new().with_current_dir(target_dir.clone());
        let result = run_agent_events("agent", "hi", opts, |ev| {
            if let AgentEventPayload::RawLine(ref line) = ev.payload {
                collected_output.extend_from_slice(line.as_bytes());
                collected_output.push(b'\n');
            }
        });

        std::env::set_var("PATH", orig_path);

        // Parent cwd must not have changed
        assert_eq!(
            std::env::current_dir().unwrap(),
            parent_cwd,
            "Parent cwd should be unchanged"
        );

        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        let output = String::from_utf8_lossy(&collected_output);
        // The child's pwd should be under /tmp (may be symlink-resolved)
        assert!(
            output.trim().starts_with("/tmp") || output.trim().contains("tmp"),
            "Child cwd should be /tmp but got: {}",
            output.trim()
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_current_dir_bad_path_returns_spawn_error() {
        let opts = RunOptions::new().with_current_dir(std::path::PathBuf::from(
            "/nonexistent/path/that/does/not/exist",
        ));
        let result = run_agent_events("agent", "hi", opts, |_| {});
        assert!(
            matches!(result, Err(RunError::SpawnFailed(_))),
            "Non-existent current_dir should return SpawnFailed, got {:?}",
            result.map(|_| ())
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_timeout_partial_output_returned() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        // Stub that writes some output then sleeps
        let dir = tempfile::tempdir().unwrap();
        let stub_path = dir.path().join("agent");
        let mut f = std::fs::File::create(&stub_path).unwrap();
        writeln!(f, "#!/bin/sh\necho 'partial line'\nsleep 60").unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
        drop(f);

        let orig_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), orig_path);
        std::env::set_var("PATH", &new_path);

        let opts = RunOptions::new().with_timeout(Duration::from_millis(600));
        let result = run_agent_events("agent", "hi", opts, |_| {});

        std::env::set_var("PATH", orig_path);

        match result {
            Err(RunError::TimedOut { stdout, .. }) => {
                let stdout_str = String::from_utf8_lossy(&stdout);
                assert!(
                    stdout_str.contains("partial line"),
                    "Partial output should be preserved on timeout, got: {:?}",
                    stdout_str
                );
            }
            other => panic!(
                "Expected TimedOut with partial output, got {:?}",
                other.map(|_| ())
            ),
        }
    }

    // --- Token usage extraction tests (using recorded_case01 fixture data) ---

    #[test]
    fn test_extract_codex_usage_from_turn_completed() {
        let line: serde_json::Value = serde_json::from_str(
            r#"{"type":"turn.completed","usage":{"input_tokens":8058,"cached_input_tokens":6912,"output_tokens":15}}"#,
        )
        .unwrap();
        let result = extract_usage_from_line(&line, "codex");
        assert!(result.is_some());
        let (usage, source) = result.unwrap();
        assert_eq!(source, UsageSource::Codex);
        assert_eq!(usage.input_tokens, 8058);
        assert_eq!(usage.output_tokens, 15);
        assert_eq!(usage.cache_read_tokens, Some(6912));
        assert!(usage.total_tokens.is_none());
    }

    #[test]
    fn test_extract_codex_usage_ignores_other_types() {
        let line: serde_json::Value = serde_json::from_str(r#"{"type":"turn.started"}"#).unwrap();
        assert!(extract_usage_from_line(&line, "codex").is_none());
    }

    #[test]
    fn test_extract_claude_usage_from_result() {
        let line: serde_json::Value = serde_json::from_str(
            r#"{"type":"result","subtype":"success","usage":{"input_tokens":3,"cache_creation_input_tokens":4807,"cache_read_input_tokens":11219,"output_tokens":5}}"#,
        )
        .unwrap();
        let result = extract_usage_from_line(&line, "claude");
        assert!(result.is_some());
        let (usage, source) = result.unwrap();
        assert_eq!(source, UsageSource::Claude);
        assert_eq!(usage.input_tokens, 3);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.cache_read_tokens, Some(11219));
        assert_eq!(usage.cache_creation_tokens, Some(4807));
    }

    #[test]
    fn test_extract_claude_usage_from_stream_event_message_start() {
        let line: serde_json::Value = serde_json::from_str(
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"usage":{"input_tokens":3,"cache_creation_input_tokens":4807,"cache_read_input_tokens":11219,"output_tokens":1}}}}"#,
        )
        .unwrap();
        let result = extract_usage_from_line(&line, "claude");
        assert!(result.is_some());
        let (usage, source) = result.unwrap();
        assert_eq!(source, UsageSource::Claude);
        assert_eq!(usage.input_tokens, 3);
        assert_eq!(usage.cache_creation_tokens, Some(4807));
    }

    #[test]
    fn test_extract_gemini_usage_from_result_stats() {
        let line: serde_json::Value = serde_json::from_str(
            r#"{"type":"result","status":"success","stats":{"total_tokens":7039,"input_tokens":7003,"output_tokens":2,"cached":6637,"input":366,"duration_ms":7615,"tool_calls":0}}"#,
        )
        .unwrap();
        let result = extract_usage_from_line(&line, "gemini");
        assert!(result.is_some());
        let (usage, source) = result.unwrap();
        assert_eq!(source, UsageSource::Gemini);
        assert_eq!(usage.input_tokens, 7003);
        assert_eq!(usage.output_tokens, 2);
        assert_eq!(usage.total_tokens, Some(7039));
        assert_eq!(usage.cache_read_tokens, Some(6637));
    }

    #[test]
    fn test_extract_gemini_usage_ignores_non_result() {
        let line: serde_json::Value =
            serde_json::from_str(r#"{"type":"message","role":"user","content":"ok"}"#).unwrap();
        assert!(extract_usage_from_line(&line, "gemini").is_none());
    }

    #[test]
    fn test_extract_opencode_usage_from_step_finish() {
        let line: serde_json::Value = serde_json::from_str(
            r#"{"type":"step_finish","timestamp":1775657635524,"part":{"id":"prt_1","reason":"stop","type":"step-finish","tokens":{"total":11287,"input":11162,"output":42,"reasoning":39,"cache":{"write":0,"read":83}},"cost":0}}"#,
        )
        .unwrap();
        let result = extract_usage_from_line(&line, "opencode");
        assert!(result.is_some());
        let (usage, source) = result.unwrap();
        assert_eq!(source, UsageSource::OpenCode);
        assert_eq!(usage.input_tokens, 11162);
        assert_eq!(usage.output_tokens, 42);
        assert_eq!(usage.total_tokens, Some(11287));
        assert_eq!(usage.reasoning_tokens, Some(39));
        assert_eq!(usage.cache_read_tokens, Some(83));
        assert_eq!(usage.cache_creation_tokens, Some(0));
    }

    #[test]
    fn test_extract_cursor_usage_from_result_camelcase() {
        let line: serde_json::Value = serde_json::from_str(
            r#"{"type":"result","subtype":"success","usage":{"inputTokens":2,"outputTokens":4,"cacheReadTokens":12228,"cacheWriteTokens":2392}}"#,
        )
        .unwrap();
        let result = extract_usage_from_line(&line, "agent");
        assert!(result.is_some());
        let (usage, source) = result.unwrap();
        assert_eq!(source, UsageSource::Cursor);
        assert_eq!(usage.input_tokens, 2);
        assert_eq!(usage.output_tokens, 4);
        assert_eq!(usage.cache_read_tokens, Some(12228));
        assert_eq!(usage.cache_creation_tokens, Some(2392));
    }

    #[test]
    fn test_extract_usage_unknown_agent_returns_none() {
        let line: serde_json::Value =
            serde_json::from_str(r#"{"type":"result","usage":{"input_tokens":1}}"#).unwrap();
        assert!(extract_usage_from_line(&line, "copilot").is_none());
        assert!(extract_usage_from_line(&line, "unknown").is_none());
    }

    #[test]
    fn test_aggregate_codex_sums_all_entries() {
        let entries = vec![
            (
                TokenUsage {
                    input_tokens: 100,
                    output_tokens: 10,
                    total_tokens: None,
                    cache_read_tokens: Some(50),
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                UsageSource::Codex,
            ),
            (
                TokenUsage {
                    input_tokens: 200,
                    output_tokens: 20,
                    total_tokens: None,
                    cache_read_tokens: Some(75),
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                UsageSource::Codex,
            ),
        ];
        let result = aggregate_token_usage(&entries, UsageSource::Codex).unwrap();
        assert_eq!(result.input_tokens, 300);
        assert_eq!(result.output_tokens, 30);
        assert_eq!(result.cache_read_tokens, Some(125));
    }

    #[test]
    fn test_aggregate_claude_takes_last() {
        let entries = vec![
            (
                TokenUsage {
                    input_tokens: 10,
                    output_tokens: 1,
                    total_tokens: None,
                    cache_read_tokens: None,
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                UsageSource::Claude,
            ),
            (
                TokenUsage {
                    input_tokens: 99,
                    output_tokens: 7,
                    total_tokens: None,
                    cache_read_tokens: Some(500),
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                UsageSource::Claude,
            ),
        ];
        let result = aggregate_token_usage(&entries, UsageSource::Claude).unwrap();
        assert_eq!(result.input_tokens, 99);
        assert_eq!(result.output_tokens, 7);
        assert_eq!(result.cache_read_tokens, Some(500));
    }

    #[test]
    fn test_aggregate_empty_returns_none() {
        assert!(aggregate_token_usage(&[], UsageSource::Codex).is_none());
        assert!(aggregate_token_usage(&[], UsageSource::Claude).is_none());
    }

    #[test]
    fn test_run_options_default_emit_token_usage_events_true() {
        let opts = RunOptions::default();
        assert!(opts.emit_token_usage_events);
    }

    #[test]
    fn test_run_options_with_emit_token_usage_events() {
        let opts = RunOptions::new().with_emit_token_usage_events(false);
        assert!(!opts.emit_token_usage_events);
    }

    #[test]
    fn test_recorded_case01_codex_fixture() {
        let fixture = include_str!("../tests/fixtures/recorded_case01/codex.jsonl");
        let mut found = false;
        for line in fixture.lines().filter(|l| !l.is_empty()) {
            let val: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some((usage, source)) = extract_usage_from_line(&val, "codex") {
                assert_eq!(source, UsageSource::Codex);
                assert!(usage.input_tokens > 0);
                found = true;
            }
        }
        assert!(
            found,
            "Should find at least one token usage line in codex fixture"
        );
    }

    #[test]
    fn test_recorded_case01_claude_fixture() {
        let fixture = include_str!("../tests/fixtures/recorded_case01/claude.jsonl");
        let mut found = false;
        for line in fixture.lines().filter(|l| !l.is_empty()) {
            let val: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some((usage, source)) = extract_usage_from_line(&val, "claude") {
                assert_eq!(source, UsageSource::Claude);
                assert!(usage.input_tokens > 0 || usage.cache_read_tokens.is_some());
                found = true;
            }
        }
        assert!(
            found,
            "Should find at least one token usage line in claude fixture"
        );
    }

    #[test]
    fn test_recorded_case01_gemini_fixture() {
        let fixture = include_str!("../tests/fixtures/recorded_case01/gemini.jsonl");
        let mut found = false;
        for line in fixture.lines().filter(|l| !l.is_empty()) {
            let val: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some((usage, source)) = extract_usage_from_line(&val, "gemini") {
                assert_eq!(source, UsageSource::Gemini);
                assert!(usage.input_tokens > 0);
                found = true;
            }
        }
        assert!(
            found,
            "Should find at least one token usage line in gemini fixture"
        );
    }

    #[test]
    fn test_recorded_case01_opencode_fixture() {
        let fixture = include_str!("../tests/fixtures/recorded_case01/opencode.jsonl");
        let mut found = false;
        for line in fixture.lines().filter(|l| !l.is_empty()) {
            let val: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some((usage, source)) = extract_usage_from_line(&val, "opencode") {
                assert_eq!(source, UsageSource::OpenCode);
                assert!(usage.input_tokens > 0);
                found = true;
            }
        }
        assert!(
            found,
            "Should find at least one token usage line in opencode fixture"
        );
    }

    #[test]
    fn test_recorded_case01_cursor_fixture() {
        let fixture = include_str!("../tests/fixtures/recorded_case01/cursor-agent.jsonl");
        let mut found = false;
        for line in fixture.lines().filter(|l| !l.is_empty()) {
            let val: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Some((usage, source)) = extract_usage_from_line(&val, "agent") {
                assert_eq!(source, UsageSource::Cursor);
                assert!(usage.input_tokens > 0);
                found = true;
            }
        }
        assert!(
            found,
            "Should find at least one token usage line in cursor fixture"
        );
    }

    // --- Quota detection unit tests ---

    #[test]
    fn test_quota_category_serde_roundtrip() {
        let cats = vec![
            QuotaCategory::Hourly,
            QuotaCategory::Daily,
            QuotaCategory::Weekly,
            QuotaCategory::Requests,
            QuotaCategory::Tokens,
            QuotaCategory::Unknown,
        ];
        for cat in &cats {
            let json = serde_json::to_string(cat).unwrap();
            let back: QuotaCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(*cat, back);
        }
    }

    #[test]
    fn test_quota_exceeded_info_serde_roundtrip() {
        let info = QuotaExceededInfo {
            agent_key: "claude".to_string(),
            category: QuotaCategory::Hourly,
            raw_message: "usage limit".to_string(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: QuotaExceededInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn test_run_result_new_has_quota_exceeded_none() {
        use std::os::unix::process::ExitStatusExt;
        let status = std::process::ExitStatus::from_raw(0);
        let result = RunResult::new(status, vec![], vec![]);
        assert!(result.quota_exceeded.is_none());
    }

    #[test]
    fn test_run_error_quota_exceeded_display() {
        let info = QuotaExceededInfo {
            agent_key: "claude".to_string(),
            category: QuotaCategory::Hourly,
            raw_message: "limit reached".to_string(),
        };
        let err = RunError::QuotaExceeded(info);
        let msg = format!("{}", err);
        assert!(msg.contains("claude"));
        assert!(msg.contains("quota exceeded"));
        assert!(msg.contains("limit reached"));
    }

    #[test]
    fn test_infer_quota_category_hourly() {
        assert_eq!(
            infer_quota_category("hourly limit reached"),
            QuotaCategory::Hourly
        );
        assert_eq!(
            infer_quota_category("reset in 1 hour"),
            QuotaCategory::Hourly
        );
    }

    #[test]
    fn test_infer_quota_category_weekly() {
        assert_eq!(
            infer_quota_category("weekly quota exceeded"),
            QuotaCategory::Weekly
        );
        assert_eq!(
            infer_quota_category("resets next week"),
            QuotaCategory::Weekly
        );
    }

    #[test]
    fn test_infer_quota_category_long_context_tokens() {
        assert_eq!(
            infer_quota_category("Extra usage is required for long context requests"),
            QuotaCategory::Tokens
        );
    }

    #[test]
    fn test_infer_quota_category_unknown() {
        assert_eq!(
            infer_quota_category("something went wrong"),
            QuotaCategory::Unknown
        );
        assert_eq!(
            infer_quota_category("monthly billing cycle"),
            QuotaCategory::Unknown
        );
    }

    #[test]
    fn test_extract_quota_signal_claude_rawline_usage_limit() {
        let payload = AgentEventPayload::RawLine(
            "Claude usage limit reached. Your limit will reset at 5 PM.".to_string(),
        );
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
        assert_eq!(info.category, QuotaCategory::Unknown);
    }

    #[test]
    fn test_extract_quota_signal_claude_rawline_rate_limit_hourly() {
        let payload = AgentEventPayload::RawLine("Rate limit hit for hourly usage".to_string());
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
        assert_eq!(info.category, QuotaCategory::Hourly);
    }

    #[test]
    fn test_extract_quota_signal_claude_failed_to_load_usage_data() {
        let payload = AgentEventPayload::RawLine(
            r#"Error: Failed to load usage data: {"error":{"type":"rate_limit_error","message":"Rate limited. Please try again later."}}"#.to_string(),
        );
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
        assert!(info.raw_message.contains("Rate limited"));
    }

    #[test]
    fn test_extract_quota_signal_claude_api_error_rate_limit_reached() {
        let payload = AgentEventPayload::RawLine("API Error: Rate limit reached".to_string());
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
    }

    #[test]
    fn test_extract_quota_signal_claude_api_error_429() {
        let payload = AgentEventPayload::RawLine(
            "API Error: Request rejected (429) · Rate limited".to_string(),
        );
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
    }

    #[test]
    fn test_extract_quota_signal_claude_hit_your_limit() {
        let payload = AgentEventPayload::RawLine(
            "⎿ You've hit your limit · resets 10am (Asia/Manila)".to_string(),
        );
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
    }

    #[test]
    fn test_extract_quota_signal_claude_http_429_rate_limit_error() {
        let payload = AgentEventPayload::RawLine(
            "HTTP 429: rate_limit_error: This request would exceed your account's rate limit."
                .to_string(),
        );
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
    }

    #[test]
    fn test_extract_quota_signal_claude_error_429_json() {
        let payload = AgentEventPayload::RawLine(
            r#"Error: 429 {"type":"error","error":{"type":"rate_limit_error","message":"Extra usage is required for long context requests."},"request_id":"req_abc123"}"#.to_string(),
        );
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
        assert_eq!(info.category, QuotaCategory::Tokens);
    }

    #[test]
    fn test_extract_quota_signal_claude_json_type_error_rate_limit() {
        let payload = AgentEventPayload::JsonLine(serde_json::from_str(
            r#"{"type":"error","error":{"type":"rate_limit_error","message":"Rate limited. Please try again later."}}"#,
        ).unwrap());
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
    }

    #[test]
    fn test_extract_quota_signal_claude_json_result_error_usage() {
        let payload = AgentEventPayload::JsonLine(
            serde_json::from_str(
                r#"{"type":"result","subtype":"error","message":"usage limit reached"}"#,
            )
            .unwrap(),
        );
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
    }

    #[test]
    fn test_extract_quota_signal_codex_rate_limit_code() {
        let payload = AgentEventPayload::JsonLine(serde_json::from_str(
            r#"{"type":"error","code":"rate_limit_exceeded","message":"You have exceeded your request rate limit"}"#,
        ).unwrap());
        let info = extract_quota_signal("codex", &payload).unwrap();
        assert_eq!(info.agent_key, "codex");
    }

    #[test]
    fn test_extract_quota_signal_codex_rawline_tpm() {
        let payload = AgentEventPayload::RawLine(
            "stream disconnected before completion: Rate limit reached for organization org-abc on tokens per min (TPM): Limit 250000, Used 250000".to_string(),
        );
        let info = extract_quota_signal("codex", &payload).unwrap();
        assert_eq!(info.agent_key, "codex");
    }

    #[test]
    fn test_extract_quota_signal_codex_rawline_429() {
        let payload = AgentEventPayload::RawLine(
            "error: http 429 Too Many Requests: rate_limit_exceeded".to_string(),
        );
        let info = extract_quota_signal("codex", &payload).unwrap();
        assert_eq!(info.agent_key, "codex");
    }

    #[test]
    fn test_extract_quota_signal_gemini_resource_exhausted() {
        let payload = AgentEventPayload::JsonLine(serde_json::from_str(
            r#"{"error":{"code":429,"status":"RESOURCE_EXHAUSTED","message":"Quota exceeded"}}"#,
        ).unwrap());
        let info = extract_quota_signal("gemini", &payload).unwrap();
        assert_eq!(info.agent_key, "gemini");
        assert_eq!(info.category, QuotaCategory::Unknown);
    }

    #[test]
    fn test_extract_quota_signal_gemini_rawline_error_429() {
        let payload = AgentEventPayload::RawLine(
            "prompt 1: ERROR {'code': 429, 'message': 'Rate limit exceeded. Try again later.'}"
                .to_string(),
        );
        let info = extract_quota_signal("gemini", &payload).unwrap();
        assert_eq!(info.agent_key, "gemini");
    }

    #[test]
    fn test_extract_quota_signal_opencode_quota_message() {
        let payload = AgentEventPayload::JsonLine(
            serde_json::from_str(r#"{"type":"error","message":"weekly quota exceeded"}"#).unwrap(),
        );
        let info = extract_quota_signal("opencode", &payload).unwrap();
        assert_eq!(info.agent_key, "opencode");
        assert_eq!(info.category, QuotaCategory::Weekly);
    }

    #[test]
    fn test_extract_quota_signal_opencode_insufficient_quota_json() {
        let payload = AgentEventPayload::JsonLine(serde_json::from_str(
            r#"{"type":"error","sequence_number":2,"error":{"type":"insufficient_quota","code":"insufficient_quota","message":"You exceeded your current quota.","param":null}}"#,
        ).unwrap());
        let info = extract_quota_signal("opencode", &payload).unwrap();
        assert_eq!(info.agent_key, "opencode");
    }

    #[test]
    fn test_extract_quota_signal_opencode_rawline_daily_token() {
        let payload = AgentEventPayload::RawLine("Your daily token quota exceeded".to_string());
        let info = extract_quota_signal("opencode", &payload).unwrap();
        assert_eq!(info.agent_key, "opencode");
        assert_eq!(info.category, QuotaCategory::Daily);
    }

    #[test]
    fn test_extract_quota_signal_opencode_rawline_rate_limited() {
        let payload = AgentEventPayload::RawLine("You are rate-limited".to_string());
        let info = extract_quota_signal("opencode", &payload).unwrap();
        assert_eq!(info.agent_key, "opencode");
    }

    #[test]
    fn test_extract_quota_signal_agent_rate_limit() {
        let payload = AgentEventPayload::JsonLine(
            serde_json::from_str(
                r#"{"type":"error","message":"Rate limit exceeded for hourly requests"}"#,
            )
            .unwrap(),
        );
        let info = extract_quota_signal("agent", &payload).unwrap();
        assert_eq!(info.agent_key, "agent");
        assert_eq!(info.category, QuotaCategory::Hourly);
    }

    #[test]
    fn test_extract_quota_signal_agent_structured_log_resource_exhausted() {
        let payload = AgentEventPayload::RawLine(
            r#"structured-log.info {"message":"agent_cli.turn.outcome","metadata":{"outcome":"error","grpc_code":"resource_exhausted","error_text":"Usage limit for slow pool"}}"#.to_string(),
        );
        let info = extract_quota_signal("agent", &payload).unwrap();
        assert_eq!(info.agent_key, "agent");
    }

    #[test]
    fn test_extract_quota_signal_agent_rawline_usage_limit() {
        let payload = AgentEventPayload::RawLine(
            "b: You've hit your usage limit for Opus. Switch to Auto.".to_string(),
        );
        let info = extract_quota_signal("agent", &payload).unwrap();
        assert_eq!(info.agent_key, "agent");
    }

    #[test]
    fn test_extract_quota_signal_no_match_returns_none() {
        let payload = AgentEventPayload::RawLine("Normal output line".to_string());
        assert!(extract_quota_signal("claude", &payload).is_none());
        assert!(extract_quota_signal("codex", &payload).is_none());
        assert!(extract_quota_signal("gemini", &payload).is_none());
        assert!(extract_quota_signal("opencode", &payload).is_none());
        assert!(extract_quota_signal("agent", &payload).is_none());
    }

    #[test]
    fn test_extract_quota_signal_unknown_agent_returns_none() {
        let payload = AgentEventPayload::RawLine("Rate limit reached".to_string());
        assert!(extract_quota_signal("copilot", &payload).is_none());
    }
}
