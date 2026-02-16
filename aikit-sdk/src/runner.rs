use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::io::Write;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

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

/// Options for running an agent.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Optional model name/identifier
    pub model: Option<String>,
    /// Whether to run in "yolo" mode (auto-confirm, skip checks)
    pub yolo: bool,
    /// Whether to stream output incrementally
    pub stream: bool,
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
}

impl RunResult {
    pub fn new(status: ExitStatus, stdout: Vec<u8>, stderr: Vec<u8>) -> Self {
        Self {
            status,
            stdout,
            stderr,
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
pub enum RunError {
    /// Agent key is not runnable
    AgentNotRunnable(String),
    /// Failed to spawn process
    SpawnFailed(io::Error),
    /// Failed to write to stdin
    StdinFailed(io::Error),
    /// Failed to read stdout/stderr
    OutputFailed(io::Error),
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
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RunError::SpawnFailed(err) => Some(err),
            RunError::StdinFailed(err) => Some(err),
            RunError::OutputFailed(err) => Some(err),
            RunError::AgentNotRunnable(_) => None,
        }
    }
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

/// Builds command-line arguments for agent CLI.
fn build_agent_argv(
    prompt: &str,
    model: Option<&String>,
    yolo: bool,
    stream: bool,
) -> Vec<OsString> {
    let mut argv = vec![
        OsString::from("agent"),
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

    if stream {
        argv.push(OsString::from("--stream"));
    }

    argv
}

/// Runs an agent with the given prompt and options.
pub fn run_agent(
    agent_key: &str,
    prompt: &str,
    options: RunOptions,
) -> Result<RunResult, RunError> {
    if !is_runnable(agent_key) {
        return Err(RunError::AgentNotRunnable(agent_key.to_string()));
    }

    let argv = match agent_key {
        "codex" => build_codex_argv(prompt, options.model.as_ref(), options.yolo, options.stream),
        "claude" => build_claude_argv(prompt, options.model.as_ref(), options.yolo, options.stream),
        "gemini" => build_gemini_argv(prompt, options.model.as_ref(), options.yolo, options.stream),
        "opencode" => {
            build_opencode_argv(prompt, options.model.as_ref(), options.yolo, options.stream)
        }
        "agent" => build_agent_argv(prompt, options.model.as_ref(), options.yolo, options.stream),
        _ => unreachable!(),
    };

    let binary = &argv[0];
    let args = &argv[1..];

    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(RunError::SpawnFailed)?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(RunError::StdinFailed)?;
    }

    let output = child.wait_with_output().map_err(RunError::OutputFailed)?;

    Ok(RunResult::new(output.status, output.stdout, output.stderr))
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
    let mut cmd = Command::new(binary);
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
        let argv = build_agent_argv("test prompt", Some(&"custom-model".to_string()), true, true);
        assert!(argv.contains(&OsString::from("agent")));
        assert!(argv.contains(&OsString::from("--prompt")));
        assert!(argv.contains(&OsString::from("test prompt")));
        assert!(argv.contains(&OsString::from("--model")));
        assert!(argv.contains(&OsString::from("custom-model")));
        assert!(argv.contains(&OsString::from("--yolo")));
        assert!(argv.contains(&OsString::from("--stream")));
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
        let result = RunResult::new(ExitStatus::from_raw(256), stdout, stderr);

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
}
