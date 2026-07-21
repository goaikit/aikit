pub mod approval;
pub mod argv;
pub mod availability;
pub mod backend;
pub mod backends;
pub mod capabilities;
#[cfg(feature = "claude-control")]
pub mod claude_session;
#[cfg(feature = "codex-app-server")]
pub mod codex_session;
pub mod transport;
pub mod types;
pub mod usage;

pub use types::{
    AgentAvailabilityReason, AgentEvent, AgentEventPayload, AgentEventStream, AgentStatus,
    MessageKind, MessagePhase, MessageRole, OutputMode, ProgressSink, QuotaCategory,
    QuotaExceededInfo, RunError, RunOptions, RunResult, StreamMessage, TokenUsage, UsageSource,
};

pub use argv::{is_runnable, runnable_agents};
pub use availability::{get_agent_status, get_installed_agents, is_agent_available};
pub use backend::{Backend, Decoded};
pub use capabilities::BackendCapabilities;
#[cfg(feature = "claude-control")]
pub use claude_agent_sdk::{HookCallback, HookMatcherConfig, SdkMcpServerConfig};
#[cfg(feature = "claude-control")]
pub use claude_session::{
    open_claude_session, ClaudePermissionMode, ClaudeSession, ClaudeSessionError,
    ClaudeSessionOptions, ControlHandle,
};
#[cfg(feature = "codex-app-server")]
pub use codex_session::{
    open_codex_session, CodexControlHandle, CodexSession, CodexSessionError, CodexSessionOptions,
};
// Shared approval types; available when at least one session feature is enabled.
#[cfg(any(feature = "claude-control", feature = "codex-app-server"))]
pub use approval::{PermissionCallback, ToolApprovalRequest, ToolDecision};
pub use usage::aggregate_token_usage;

use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::{panic, thread};

use types::ReaderMsg;

/// A cancellation handle for a run started via [`run_agent_events_cancellable`].
///
/// This is the single trigger point ADR 0014 requires for terminating a
/// running agent subprocess, in place of separate ad-hoc kill paths: the
/// same [`cancel`](RunCancelHandle::cancel) method is called internally by
/// the run's timeout watchdog, and can also be called by an external caller
/// (e.g. a `serve` client-disconnect handler, or a future explicit interrupt)
/// holding a clone of the handle on another thread. Cloning is cheap — all
/// clones share the same underlying state. Whichever caller cancels first
/// wins; later calls are no-ops.
///
/// Cancelling escalates `SIGTERM` -> (~3s grace) -> `SIGKILL` over the
/// agent's whole process group (see
/// [`transport::subprocess::kill_process_group`]), which reaps any
/// grandchildren the agent spawned and guarantees the run's stdout/stderr
/// reader threads observe EOF — so a cancelled/timed-out run always returns
/// rather than hanging (BUG-1/BUG-4).
///
/// A fresh, unbound handle can be created and cancelled before the run it
/// will be attached to has even spawned; the pending cancellation is applied
/// as soon as the run binds its child process.
///
/// Today, a run terminated via this handle surfaces as
/// [`RunError::TimedOut`] (the closest existing terminal variant); a
/// dedicated `RunError::Cancelled` is natural follow-up once external
/// cancellation is wired through `serve`, but is out of scope here. This
/// handle is deliberately forward-compatible with the future
/// `Session`/`ControlHandle` seam (ADR 0014 consequences): cancellation is
/// meant to become one control op among several (interrupt, set-permission,
/// disconnect) over that seam.
#[derive(Clone)]
pub struct RunCancelHandle {
    inner: Arc<CancelInner>,
}

struct CancelInner {
    triggered: AtomicBool,
    child: Mutex<Option<Arc<Mutex<Child>>>>,
}

impl RunCancelHandle {
    /// Create a fresh, unbound cancel handle.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CancelInner {
                triggered: AtomicBool::new(false),
                child: Mutex::new(None),
            }),
        }
    }

    /// Request cancellation of the bound run. Idempotent: the first caller
    /// wins and performs the process-group kill escalation; subsequent
    /// calls (from a racing watchdog vs. external caller, for example) are
    /// no-ops. If called before a child has been bound yet, the request is
    /// still recorded and applied immediately once the run binds its child.
    pub fn cancel(&self) {
        if self
            .inner
            .triggered
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            if let Ok(guard) = self.inner.child.lock() {
                if let Some(child) = guard.as_ref() {
                    transport::subprocess::kill_process_group(child);
                }
            }
        }
    }

    /// True once cancellation has been requested (the child may still be in
    /// the process of exiting).
    pub fn is_cancelled(&self) -> bool {
        self.inner.triggered.load(Ordering::SeqCst)
    }

    /// Bind the handle to a live child, right after the subprocess spawns.
    /// If `cancel()` already raced ahead of the bind, kill immediately
    /// rather than leaving the just-spawned child running unattended.
    fn bind(&self, child: Arc<Mutex<Child>>) {
        {
            let mut guard = self.inner.child.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(Arc::clone(&child));
        }
        if self.inner.triggered.load(Ordering::SeqCst) {
            transport::subprocess::kill_process_group(&child);
        }
    }

    /// Detach the bound child once the run has reaped it, so a later `Drop`
    /// backstop does not act on a handle whose pid may since have been
    /// reused by an unrelated process.
    fn unbind(&self) {
        if let Ok(mut guard) = self.inner.child.lock() {
            *guard = None;
        }
    }
}

impl Default for RunCancelHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CancelInner {
    fn drop(&mut self) {
        // Backstop for a handle dropped (e.g. a `serve` session cleaned up)
        // while still bound to a live child: kill the group rather than
        // leaking it. A no-op if the run already reaped and unbound.
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.take() {
                transport::subprocess::kill_process_group(&child);
            }
        }
    }
}

/// Decode one inbound JSON line into canonical [`StreamMessage`]s for the given
/// agent key. Unknown keys yield `[]` (with a warning). Thin wrapper over
/// [`Backend::decode`]; retained for API compatibility.
pub fn normalize_json_line(
    agent_key: &str,
    stream: AgentEventStream,
    value: &serde_json::Value,
    raw_line_seq: u64,
) -> Vec<StreamMessage> {
    match Backend::from_key(agent_key) {
        // The StreamMessage view of decode; structured tool frames (visible via
        // `run_agent_events`) are not surfaced through this legacy helper.
        Some(backend) => backend
            .decode(value, stream, raw_line_seq)
            .into_iter()
            .filter_map(|d| match d {
                backend::Decoded::Stream(m) => Some(m),
                _ => None,
            })
            .collect(),
        None => {
            tracing::warn!(
                target: "aikit_sdk::runner::decode",
                agent_key = %agent_key,
                "E_DECODE_UNKNOWN_AGENT: unknown agent key"
            );
            Vec::new()
        }
    }
}

/// Extract token usage from one inbound JSON line. `None` for lines without
/// usage data or for unknown agent keys. Thin wrapper over
/// [`Backend::extract_usage`].
pub fn extract_usage_from_line(
    line: &serde_json::Value,
    agent_key: &str,
) -> Option<(TokenUsage, UsageSource)> {
    Backend::from_key(agent_key)?.extract_usage(line)
}

/// Detect a quota / rate-limit signal from one payload. `None` for no match or
/// unknown agent keys. Thin wrapper over [`Backend::extract_quota`].
pub fn extract_quota_signal(
    agent_key: &str,
    payload: &AgentEventPayload,
) -> Option<QuotaExceededInfo> {
    Backend::from_key(agent_key)?.extract_quota(payload)
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

/// Run the built-in aikit agent with a given output mode.
///
/// `agent_key` MUST be `"aikit"`; other values return `RunError::WrongAgentKey`.
/// `writer` receives plain text (Plain mode) or NDJSON lines (Events mode).
/// `err_writer` receives plain stderr bytes (Plain and Events modes).
/// `progress_sink` MUST be `Some` when `mode == OutputMode::Progress`.
pub fn run_builtin_agent(
    agent_key: &str,
    prompt: &str,
    options: RunOptions,
    mode: OutputMode,
    writer: &mut dyn std::io::Write,
    err_writer: &mut dyn std::io::Write,
    progress_sink: Option<Box<dyn ProgressSink>>,
) -> Result<RunResult, RunError> {
    if agent_key != "aikit" {
        return Err(RunError::WrongAgentKey(agent_key.to_string()));
    }

    match mode {
        OutputMode::Progress => {
            let mut sink = progress_sink.ok_or(RunError::MissingProgressSink)?;
            let mut collected: Vec<AgentEvent> = Vec::new();
            let result =
                crate::aikit_agent_adapter::run_aikit_agent(prompt, &options, None, |event| {
                    collected.push(event);
                })?;
            let mut progress = crate::run_progress::RunProgress::new(
                crate::run_progress::ProgressViewConfig::default(),
            );
            for event in &collected {
                progress.push("aikit", event);
                sink.on_progress(&progress);
            }
            let exit_code = result.exit_code().unwrap_or(1);
            sink.on_finalize(exit_code, progress.token_footer());
            Ok(result)
        }
        OutputMode::Events => {
            let mut collected: Vec<AgentEvent> = Vec::new();
            let result =
                crate::aikit_agent_adapter::run_aikit_agent(prompt, &options, None, |event| {
                    collected.push(event);
                })?;
            for event in &collected {
                if let Ok(line) = serde_json::to_string(event) {
                    let _ = writeln!(writer, "{}", line);
                }
            }
            let _ = err_writer.write_all(&result.stderr);
            Ok(result)
        }
        OutputMode::Plain => {
            let result =
                crate::aikit_agent_adapter::run_aikit_agent(prompt, &options, None, |_| {})?;
            let _ = writer.write_all(&result.stdout);
            let _ = err_writer.write_all(&result.stderr);
            Ok(result)
        }
    }
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
///
/// Thin wrapper over [`run_agent_events_cancellable`] with a fresh,
/// internal-only [`RunCancelHandle`] — equivalent to before except that
/// termination (on timeout) now escalates over the whole process group
/// rather than only the direct child (ADR 0014 / BUG-4). Callers that need
/// to cancel a run from another thread (e.g. on client disconnect) should
/// call [`run_agent_events_cancellable`] directly and hold on to the handle.
pub fn run_agent_events<F>(
    agent_key: &str,
    prompt: &str,
    options: RunOptions,
    on_event: F,
) -> Result<RunResult, RunError>
where
    F: FnMut(AgentEvent) + Send,
{
    run_agent_events_cancellable(
        agent_key,
        prompt,
        options,
        &RunCancelHandle::new(),
        on_event,
    )
}

/// Same as [`run_agent_events`], but the caller supplies a [`RunCancelHandle`]
/// that can be triggered from another thread to terminate the run early —
/// the seam ADR 0014 introduces so a future `serve` layer can cancel a run on
/// client disconnect (or an explicit interrupt) without duplicating the
/// process-group kill logic.
pub fn run_agent_events_cancellable<F>(
    agent_key: &str,
    prompt: &str,
    options: RunOptions,
    cancel: &RunCancelHandle,
    mut on_event: F,
) -> Result<RunResult, RunError>
where
    F: FnMut(AgentEvent) + Send,
{
    tracing::debug!(
        target: "aikit_sdk::runner",
        agent_key = %agent_key,
        prompt_len = prompt.len(),
        timeout = ?options.timeout.map(|d| d.as_secs()),
        stream = options.stream,
        yolo = options.yolo,
        "run_agent_events"
    );

    // The built-in aikit Backend is in-process: it emits canonical events
    // directly rather than over a subprocess Transport (ADR 0009). It has no
    // subprocess for `cancel` to act on.
    if agent_key == "aikit" {
        return crate::aikit_agent_adapter::run_aikit_agent(prompt, &options, None, on_event);
    }

    let backend = Backend::from_key(agent_key)
        .ok_or_else(|| RunError::AgentNotRunnable(agent_key.to_string()))?;

    // Establish the subprocess-stdout-lines Transport: spawn (in its own
    // process group), start the stdout/stderr reader threads, start the
    // stdin writer thread. See `transport::subprocess::connect` for why the
    // prompt write does not happen on this thread (BUG-1).
    let transport::subprocess::SubprocessConnection {
        child,
        rx,
        stdout_thread,
        stderr_thread,
        stdin_thread,
        argv: _argv,
    } = transport::subprocess::connect(backend, prompt, &options, true)?;

    // Bind the cancel handle to the live child immediately — before the
    // timeout watchdog and before draining begins — so an external cancel()
    // racing in from another thread always has a child to act on.
    cancel.bind(Arc::clone(&child));

    // Watchdog: if timeout is configured, spawn a dedicated thread. It blocks
    // on a done channel for the configured duration. On timeout it calls
    // `cancel.cancel()`, which escalates SIGTERM -> SIGKILL over the child's
    // process group (ADR 0014 / BUG-4) and closes the pipes -> reader
    // threads finish -> tx senders drop -> rx closes -> drain loop exits.
    //
    // The watchdog does NOT hold a tx sender. This avoids a deadlock where the
    // watchdog's tx would keep rx alive on the natural-exit path (blocking the
    // drain loop until the full timeout elapses).
    let watchdog_done: Option<mpsc::Sender<()>> = if let Some(timeout_duration) = options.timeout {
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let cancel_watchdog = cancel.clone();
        thread::spawn(move || {
            match done_rx.recv_timeout(timeout_duration) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // Natural exit signaled or done_tx dropped — do nothing.
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    cancel_watchdog.cancel();
                }
            }
        });
        Some(done_tx)
    } else {
        None
    };

    // The Transport owns both senders (see `subprocess::connect`), so `rx`
    // closes naturally once the reader threads finish — no extra sender to drop.

    let mut seq: u64 = 0;
    let mut stdout_bytes: Vec<u8> = Vec::new();
    let mut stderr_bytes: Vec<u8> = Vec::new();
    let mut reader_error: Option<RunError> = None;
    let mut callback_panic: Option<Box<dyn std::any::Any + Send>> = None;
    let mut usage_entries: Vec<(TokenUsage, UsageSource)> = Vec::new();
    let mut quota_exceeded: Option<QuotaExceededInfo> = None;
    let mut json_lines_seen: u64 = 0;
    let mut stream_messages_emitted: u64 = 0;
    let mut json_lines_unmapped: u64 = 0;

    let emit_raw = options.emit_raw_transport;

    for msg in rx {
        match msg {
            ReaderMsg::Chunk { stream, raw } => {
                // Accumulate raw bytes verbatim.
                match stream {
                    AgentEventStream::Stdout => stdout_bytes.extend_from_slice(&raw),
                    AgentEventStream::Stderr => stderr_bytes.extend_from_slice(&raw),
                }

                let payload = parse_payload(&raw);

                match payload {
                    AgentEventPayload::JsonLine(ref json_val) => {
                        json_lines_seen += 1;

                        let extracted_usage = backend.extract_usage(json_val);
                        if let Some(ref up) = extracted_usage {
                            usage_entries.push(up.clone());
                        }

                        let json_line_seq = seq;
                        let quota_signal = backend.extract_quota(&payload);

                        if emit_raw {
                            let stripped = raw
                                .strip_suffix(b"\r\n")
                                .or_else(|| raw.strip_suffix(b"\n"))
                                .unwrap_or(&raw);
                            let raw_str = String::from_utf8_lossy(stripped).to_string();
                            let raw_event = AgentEvent {
                                agent_key: agent_key.to_string(),
                                seq,
                                stream,
                                payload: AgentEventPayload::RawTransportLine {
                                    raw: raw_str,
                                    stream,
                                    seq: json_line_seq,
                                },
                            };
                            seq += 1;
                            if callback_panic.is_none() {
                                let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                                    on_event(raw_event);
                                }));
                                if let Err(p) = result {
                                    callback_panic = Some(p);
                                }
                            }
                        }

                        let decoded = backend.decode(json_val, stream, json_line_seq);
                        if decoded.is_empty() {
                            json_lines_unmapped += 1;
                        }
                        for frame in decoded {
                            let payload = match frame {
                                backend::Decoded::Stream(m) => {
                                    stream_messages_emitted += 1;
                                    AgentEventPayload::StreamMessage(m)
                                }
                                backend::Decoded::ToolUse {
                                    call_id,
                                    tool_name,
                                    input,
                                } => AgentEventPayload::ToolUse {
                                    call_id,
                                    tool_name,
                                    input,
                                },
                                backend::Decoded::ToolResult {
                                    call_id,
                                    output,
                                    is_error,
                                } => AgentEventPayload::ToolResult {
                                    call_id,
                                    output,
                                    is_error,
                                },
                            };
                            let frame_event = AgentEvent {
                                agent_key: agent_key.to_string(),
                                seq,
                                stream,
                                payload,
                            };
                            seq += 1;
                            if callback_panic.is_none() {
                                let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                                    on_event(frame_event);
                                }));
                                if let Err(p) = result {
                                    callback_panic = Some(p);
                                }
                            }
                        }

                        if options.emit_token_usage_events {
                            if let Some((usage, source)) = extracted_usage {
                                let token_event = AgentEvent {
                                    agent_key: agent_key.to_string(),
                                    seq,
                                    stream,
                                    payload: AgentEventPayload::TokenUsageLine {
                                        usage,
                                        source,
                                        raw_agent_line_seq: json_line_seq,
                                    },
                                };
                                seq += 1;
                                if callback_panic.is_none() {
                                    let result =
                                        panic::catch_unwind(panic::AssertUnwindSafe(|| {
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
                                stream,
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
                    _ => {
                        let source_seq = seq;
                        // BUG-11: for Claude, a non-JSON stdout line is
                        // ordinary assistant text (e.g. `--output-format
                        // text`), not a structured error channel — matching
                        // "usage limit"/"rate limit" substrings there is a
                        // false-positive quota-exceeded trap (the assistant
                        // merely discussing rate limits ends the run). Only
                        // treat it as a quota signal on stderr (or via the
                        // JSON-error branch above, which is already
                        // structurally gated). Other backends are unaffected.
                        let quota_signal = if backend == Backend::Claude
                            && stream == AgentEventStream::Stdout
                            && matches!(payload, AgentEventPayload::RawLine(_))
                        {
                            None
                        } else {
                            backend.extract_quota(&payload)
                        };
                        let event = AgentEvent {
                            agent_key: agent_key.to_string(),
                            seq,
                            stream,
                            payload,
                        };
                        seq += 1;

                        if callback_panic.is_none() {
                            let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                                on_event(event);
                            }));
                            if let Err(p) = result {
                                callback_panic = Some(p);
                            }
                        }

                        if let Some(info) = quota_signal {
                            if quota_exceeded.is_none() {
                                quota_exceeded = Some(info.clone());
                            }
                            let quota_event = AgentEvent {
                                agent_key: agent_key.to_string(),
                                seq,
                                stream,
                                payload: AgentEventPayload::QuotaExceeded {
                                    info,
                                    raw_agent_line_seq: source_seq,
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
                }
            }
            ReaderMsg::Err { stream, source } => {
                if reader_error.is_none() {
                    reader_error = Some(RunError::ReaderFailed { stream, source });
                }
            }
        }
    }

    tracing::info!(
        target: "aikit_sdk::runner::normalize",
        json_lines_seen,
        stream_messages_emitted,
        json_lines_unmapped,
        "run_agent_events completed"
    );

    // Signal watchdog on natural exit path (no-op if it already fired).
    if let Some(done_tx) = watchdog_done {
        let _ = done_tx.send(());
    }

    // Join reader threads before wait() to prevent pipe deadlock.
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    // The stdin writer thread is independent of the drain loop (it may have
    // finished long ago, or — for a child that never reads its input to
    // completion — be blocked until the child exits/closes stdin, which the
    // timeout/cancel path below guarantees). A non-BrokenPipe write failure
    // is recorded but does not preempt a `timed_out`/panic/reader-error
    // outcome, which take precedence below.
    let stdin_error = match stdin_thread.join() {
        Ok(Ok(())) => None,
        Ok(Err(e)) => Some(RunError::StdinFailed(e)),
        Err(_) => None, // writer thread panicked; treat like other thread panics (ignored)
    };

    let timed_out = cancel.is_cancelled();

    if timed_out {
        // child.wait() reaps the zombie even after the process-group kill;
        // ignore status. Unbind so a later Drop backstop is a no-op.
        let _ = child.lock().unwrap_or_else(|e| e.into_inner()).wait();
        cancel.unbind();
        return Err(RunError::TimedOut {
            // `options.timeout` is `None` when cancellation came from an
            // external caller rather than the watchdog; there is no
            // meaningful duration to report, so fall back to zero.
            timeout: options.timeout.unwrap_or_default(),
            stdout: stdout_bytes,
            stderr: stderr_bytes,
        });
    }

    let status = child
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .wait()
        .map_err(RunError::OutputFailed)?;
    cancel.unbind();

    if let Some(p) = callback_panic {
        transport::subprocess::kill_process_group(&child);
        return Err(RunError::CallbackPanic(p));
    }

    if let Some(err) = reader_error {
        transport::subprocess::kill_process_group(&child);
        return Err(err);
    }

    if let Some(err) = stdin_error {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::types::{
        AgentEventPayload, AgentEventStream, RunError, RunOptions, RunResult,
    };

    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;

    use std::process::ExitStatus;
    use std::time::Duration;

    #[cfg(unix)]
    static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_run_options_session_id_default_is_none() {
        assert_eq!(RunOptions::default().session_id, None);
    }

    #[test]
    fn test_run_options_with_session_id() {
        let opts = RunOptions::default().with_session_id("abc");
        assert_eq!(opts.session_id, Some("abc".to_string()));
    }

    #[test]
    fn test_run_error_session_not_found_display() {
        let err = RunError::SessionNotFound("x".to_string());
        assert_eq!(err.to_string(), "error: session 'x' not found");
    }

    #[test]
    fn test_run_error_session_load_failed_display() {
        let err = RunError::SessionLoadFailed {
            id: "x".to_string(),
            reason: "bad json".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "error: session 'x' could not be loaded: bad json"
        );
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
        let result = run_agent_events("cursor", "hello", RunOptions::default(), |ev| {
            events.push(ev)
        });

        std::env::set_var("PATH", orig_path);

        assert!(
            result.is_ok(),
            "run_agent_events should succeed: {:?}",
            result.err()
        );
        assert_eq!(
            events.len(),
            0,
            "Unmapped JSON lines should produce no callback events"
        );
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
                AgentEventPayload::StreamMessage(_) => "stream_message",
                AgentEventPayload::ToolUse { .. } => "tool_use",
                AgentEventPayload::ToolResult { .. } => "tool_result",
                AgentEventPayload::TokenUsageLine { .. } => "token_usage",
                AgentEventPayload::QuotaExceeded { .. } => "quota_exceeded",
                AgentEventPayload::RawTransportLine { .. } => "raw_transport",
                AgentEventPayload::AikitTextDelta { .. } => "aikit_text_delta",
                AgentEventPayload::AikitTextFinal { .. } => "aikit_text_final",
                AgentEventPayload::AikitToolUse { .. } => "aikit_tool_use",
                AgentEventPayload::AikitToolResult { .. } => "aikit_tool_result",
                AgentEventPayload::AikitSubagentSpawn { .. } => "aikit_subagent_spawn",
                AgentEventPayload::AikitSubagentResult { .. } => "aikit_subagent_result",
                AgentEventPayload::AikitContextCompressed { .. } => "aikit_context_compressed",
                AgentEventPayload::AikitStepFinish { .. } => "aikit_step_finish",
                AgentEventPayload::SessionStarted { .. } => "session_started",
            };
            payloads.push(kind.to_string());
        });

        std::env::set_var("PATH", orig_path);

        assert!(result.is_ok());
        assert_eq!(payloads, vec!["raw"]);
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
        let start = std::time::Instant::now();
        let result = run_agent_events("cursor", "hi", opts, |_| {});
        let elapsed = start.elapsed();

        std::env::set_var("PATH", orig_path);

        assert!(
            matches!(result, Err(RunError::TimedOut { .. })),
            "Expected TimedOut, got {:?}",
            result.map(|_| ())
        );
        // The stub sleeps for 60s; the call must return promptly after the
        // 500ms timeout (well under the SIGTERM->3s grace->SIGKILL ceiling),
        // not hang until the child's sleep completes (ADR 0014 / BUG-1).
        assert!(
            elapsed < Duration::from_secs(6),
            "run_agent_events should return promptly after timeout, took {:?}",
            elapsed
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
        let result = run_agent_events("cursor", "hi", opts, |_| {});

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
        let result = run_agent_events("cursor", "hi", opts, |ev| {
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
        let result = run_agent_events("cursor", "hi", opts, |_| {});
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
        let result = run_agent_events("cursor", "hi", opts, |_| {});

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

    // --- ADR 0014 / BUG-1: stdin write must not deadlock against readers ---

    #[cfg(unix)]
    #[test]
    fn test_large_prompt_does_not_deadlock_stdin_write() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        // A stub that fills its OWN stdout pipe (comfortably past a typical
        // 64KiB OS pipe buffer) BEFORE it reads any stdin, then drains
        // stdin, then exits. If the harness wrote the prompt synchronously
        // on the calling thread before any reader was draining stdout (the
        // pre-fix behaviour), this reproduces the BUG-1 deadlock: the child
        // blocks writing output into a full pipe while the harness blocks
        // writing input into a child that isn't reading — and with no
        // watchdog armed yet, nothing breaks the standoff.
        let dir = tempfile::tempdir().unwrap();
        let stub_path = dir.path().join("agent");
        let mut f = std::fs::File::create(&stub_path).unwrap();
        writeln!(
            f,
            "#!/bin/sh\nyes x | head -c 200000\ncat >/dev/null\necho done"
        )
        .unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
        drop(f);

        let orig_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), orig_path);
        std::env::set_var("PATH", &new_path);

        // A prompt larger than a typical pipe buffer, to be written on the
        // stdin side while the child is simultaneously producing its own
        // >64KiB burst of stdout.
        let big_prompt = "p".repeat(200_000);
        // A generous but finite safety-net timeout: this test asserts the
        // run completes well within it (real deadlock would hang past 60s).
        let opts = RunOptions::new().with_timeout(Duration::from_secs(15));

        let start = std::time::Instant::now();
        let result = run_agent_events("cursor", &big_prompt, opts, |_| {});
        let elapsed = start.elapsed();

        std::env::set_var("PATH", orig_path);

        assert!(
            result.is_ok(),
            "large prompt + large stdout burst must not deadlock: {:?}",
            result.err()
        );
        assert!(
            elapsed < Duration::from_secs(10),
            "run should complete quickly, not hang on a stdin/stdout standoff: took {:?}",
            elapsed
        );
    }

    // --- ADR 0014 / BUG-4: process-group kill reaps grandchildren ---

    #[cfg(target_os = "linux")]
    #[test]
    fn test_process_group_kill_reaps_grandchild() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("grandchild.pid");
        let stub_path = dir.path().join("agent");
        let mut f = std::fs::File::create(&stub_path).unwrap();
        // The direct child backgrounds a long-sleeping grandchild, records
        // its pid, then itself sleeps well past the configured timeout.
        writeln!(
            f,
            "#!/bin/sh\nsleep 60 &\necho $! > {}\necho started\nsleep 60",
            pidfile.display()
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
        let result = run_agent_events("cursor", "hi", opts, |_| {});

        std::env::set_var("PATH", orig_path);

        assert!(
            matches!(result, Err(RunError::TimedOut { .. })),
            "Expected TimedOut, got {:?}",
            result.map(|_| ())
        );

        // Give the OS a brief moment to finish reaping after SIGKILL.
        std::thread::sleep(Duration::from_millis(300));

        let pid_str = std::fs::read_to_string(&pidfile)
            .expect("grandchild pid file should have been written");
        let pid: i32 = pid_str.trim().parse().expect("grandchild pid recorded");
        let proc_path = format!("/proc/{}", pid);
        assert!(
            !std::path::Path::new(&proc_path).exists(),
            "grandchild pid {} should have been reaped by the process-group kill, not orphaned",
            pid
        );
    }

    // --- ADR 0014: first-class external cancel mechanism ---

    #[cfg(unix)]
    #[test]
    fn test_external_cancel_terminates_run_and_returns() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let stub_path = dir.path().join("agent");
        let mut f = std::fs::File::create(&stub_path).unwrap();
        writeln!(f, "#!/bin/sh\necho 'before sleep'\nsleep 60").unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
        drop(f);

        let orig_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), orig_path);
        std::env::set_var("PATH", &new_path);

        // No timeout configured: only an external caller can end this run —
        // simulating a `serve` client-disconnect handler cancelling a
        // session from a different thread than the one running the agent.
        let cancel = RunCancelHandle::new();
        let cancel_for_run = cancel.clone();
        let run_thread = thread::spawn(move || {
            run_agent_events_cancellable(
                "cursor",
                "hi",
                RunOptions::default(),
                &cancel_for_run,
                |_| {},
            )
        });

        std::thread::sleep(Duration::from_millis(300));
        assert!(!cancel.is_cancelled(), "should not be cancelled yet");
        cancel.cancel();

        let start = std::time::Instant::now();
        let result = run_thread.join().expect("run thread should not panic");
        let elapsed = start.elapsed();

        std::env::set_var("PATH", orig_path);

        assert!(
            matches!(result, Err(RunError::TimedOut { .. })),
            "external cancel should terminate the run, got {:?}",
            result.map(|_| ())
        );
        assert!(
            elapsed < Duration::from_secs(6),
            "external cancel should return promptly, took {:?}",
            elapsed
        );
    }

    #[test]
    fn test_cancel_handle_cancel_before_bind_is_recorded() {
        let cancel = RunCancelHandle::new();
        assert!(!cancel.is_cancelled());
        cancel.cancel();
        assert!(cancel.is_cancelled());
        // Calling cancel again is a harmless no-op.
        cancel.cancel();
        assert!(cancel.is_cancelled());
    }

    // --- BUG-11: plain assistant text must not be mistaken for quota errors ---

    #[cfg(unix)]
    #[test]
    fn test_bug11_claude_stdout_rawline_rate_limit_not_quota() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        // Plain (non-JSON) stdout, as claude emits with `--output-format
        // text`. The assistant's own answer happens to discuss rate limits —
        // this must NOT be mistaken for a structured quota-exceeded signal.
        let dir = tempfile::tempdir().unwrap();
        let stub_path = dir.path().join("claude");
        let mut f = std::fs::File::create(&stub_path).unwrap();
        writeln!(
            f,
            "#!/bin/sh\necho 'To avoid a 429, back off when you hit a rate limit or usage limit.'"
        )
        .unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
        drop(f);

        let orig_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), orig_path);
        std::env::set_var("PATH", &new_path);

        let result = run_agent("claude", "explain rate limits", RunOptions::default());

        std::env::set_var("PATH", orig_path);

        assert!(
            result.is_ok(),
            "plain assistant text mentioning 'rate limit'/'usage limit' must not be \
             treated as quota exceeded: {:?}",
            result.err()
        );
        assert!(result.unwrap().quota_exceeded.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_bug11_claude_stderr_rawline_rate_limit_still_detected() {
        let _guard = PATH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        // A genuine CLI-emitted rate-limit line on stderr — the structured
        // error channel — must still be detected (regression guard: BUG-11's
        // fix must not blanket-disable quota detection for Claude).
        let dir = tempfile::tempdir().unwrap();
        let stub_path = dir.path().join("claude");
        let mut f = std::fs::File::create(&stub_path).unwrap();
        writeln!(
            f,
            "#!/bin/sh\necho 'rate limit exceeded, try again later' >&2"
        )
        .unwrap();
        let mut perms = f.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        f.set_permissions(perms).unwrap();
        drop(f);

        let orig_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), orig_path);
        std::env::set_var("PATH", &new_path);

        let result = run_agent("claude", "hi", RunOptions::default());

        std::env::set_var("PATH", orig_path);

        match result {
            Err(RunError::QuotaExceeded(_)) => {}
            other => panic!(
                "expected QuotaExceeded from a stderr rate-limit line, got {:?}",
                other.map(|_| ())
            ),
        }
    }
}
