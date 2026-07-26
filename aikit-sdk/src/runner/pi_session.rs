//! Bidirectional Pi sessions over `pi --mode rpc` (ADR 0017).
//!
//! The Pi analogue of [`claude_session`](super::claude_session) and
//! [`codex_session`](super::codex_session): a dedicated bridge thread drives a
//! `pi --mode rpc` subprocess, forwarding its JSONL event stream as canonical
//! [`AgentEvent`]s while a [`PiControlHandle`] issues follow-up prompts,
//! interrupts (`abort`), and steering / follow-up messages.
//!
//! Unlike the Claude/Codex sessions — which bridge an async SDK client on a
//! current-thread tokio runtime — Pi's RPC is plain JSONL over stdio with no
//! client library, so this module is pure-std: a single [`mpsc`] channel merges
//! the subprocess's stdout frames, its stderr lines, and the control handle's
//! commands into one stream the bridge thread multiplexes with blocking
//! `recv`. No tokio feature is pulled in.
//!
//! Pi keeps a conversation in memory as long as its stdin stays open, so the
//! bridge never closes stdin while the session is live (the one-shot run path's
//! stdin-hold insight, applied for the whole session lifetime). Multi-turn is a
//! second `prompt` command; interrupt is the dedicated `abort` command (which
//! stops the current operation without ending the session); `steer` and
//! `follow_up` map 1:1 to Pi's same-named commands.

use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::runner::backend::Decoded;
use crate::runner::backends::{argv_spec::ArgvCtx, pi};
use crate::runner::transport::subprocess::kill_process_group;
use crate::runner::types::{AgentEvent, AgentEventPayload, AgentEventStream};

/// How long [`open_pi_session`] waits for Pi to acknowledge the first prompt
/// before surfacing a synchronous [`PiSessionError::Connect`]. Generous on
/// purpose: Pi is a Node process and its first RPC response can land several
/// seconds after spawn once extensions load. Only a pathological hang (Pi
/// started but never answers) hits this; a healthy run resolves in well under a
/// second once Pi is up.
const READINESS_TIMEOUT: Duration = Duration::from_secs(30);

/// Options for opening a Pi session.
#[derive(Clone, Default)]
pub struct PiSessionOptions {
    /// Model pattern / id passed as `--model` (e.g. `anthropic/claude-...`).
    /// `None` = Pi's configured default.
    pub model: Option<String>,
    /// Working directory for the spawned `pi` process.
    pub cwd: Option<PathBuf>,
    /// Resume an existing Pi session by id (passed as `--session`).
    pub session_id: Option<String>,
}

impl std::fmt::Debug for PiSessionOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PiSessionOptions")
            .field("model", &self.model)
            .field("cwd", &self.cwd)
            .field("session_id", &self.session_id)
            .finish()
    }
}

/// Errors opening or driving a Pi session.
#[derive(Debug)]
pub enum PiSessionError {
    /// The bridge thread could not be spawned.
    Runtime(String),
    /// Spawning / handshaking with `pi --mode rpc` failed, or Pi rejected the
    /// first prompt (e.g. auth, config, model resolution).
    Connect(String),
    /// The control channel is closed (the session has ended).
    Closed,
}

impl std::fmt::Display for PiSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PiSessionError::Runtime(e) => write!(f, "pi session runtime error: {e}"),
            PiSessionError::Connect(e) => write!(f, "pi session connect error: {e}"),
            PiSessionError::Closed => write!(f, "pi session control channel closed"),
        }
    }
}

impl std::error::Error for PiSessionError {}

/// Commands forwarded from a [`PiControlHandle`] to the session bridge.
#[derive(Debug)]
enum ControlCmd {
    /// New user turn: a second `prompt` command on the same session.
    SendTurn(String),
    /// Abort the in-flight operation (Pi's `abort`; the session survives).
    Interrupt,
    /// Queue a steering message mid-turn (Pi's `steer`).
    Steer(String),
    /// Queue a message for after the agent settles (Pi's `follow_up`).
    FollowUp(String),
    /// End the session.
    Disconnect,
}

/// One unit multiplexed over the bridge's single inbound channel: either a
/// parsed Pi stdout frame, a stderr diagnostic line, the stdout-EOF signal
/// (Pi exited), or a control command from the handle.
#[derive(Debug)]
enum Msg {
    Frame(Value),
    Stderr(String),
    StdoutEof,
    Cmd(ControlCmd),
}

/// A sync handle to drive a live Pi session. Methods queue a command and return
/// once it is on the channel (fire-and-forget); Pi acts on each asynchronously.
pub struct PiControlHandle {
    tx: mpsc::Sender<Msg>,
}

impl PiControlHandle {
    fn cmd(&self, cmd: ControlCmd) -> Result<(), PiSessionError> {
        self.tx
            .send(Msg::Cmd(cmd))
            .map_err(|_| PiSessionError::Closed)
    }

    /// Send a follow-up user prompt on the same session (multi-turn). If Pi is
    /// still streaming, prefer [`PiControlHandle::steer`] or
    /// [`PiControlHandle::follow_up`].
    pub fn send_turn(&self, text: impl Into<String>) -> Result<(), PiSessionError> {
        self.cmd(ControlCmd::SendTurn(text.into()))
    }

    /// Abort the in-flight operation. The session stays open for further turns.
    pub fn interrupt(&self) -> Result<(), PiSessionError> {
        self.cmd(ControlCmd::Interrupt)
    }

    /// Queue a steering message delivered after the current turn's tool calls,
    /// before the next LLM call.
    pub fn steer(&self, text: impl Into<String>) -> Result<(), PiSessionError> {
        self.cmd(ControlCmd::Steer(text.into()))
    }

    /// Queue a message processed once the agent has settled.
    pub fn follow_up(&self, text: impl Into<String>) -> Result<(), PiSessionError> {
        self.cmd(ControlCmd::FollowUp(text.into()))
    }

    /// End the session.
    pub fn disconnect(&self) -> Result<(), PiSessionError> {
        self.cmd(ControlCmd::Disconnect)
    }
}

impl Drop for PiControlHandle {
    fn drop(&mut self) {
        // Dropping the only handle without an explicit `disconnect()` would
        // otherwise leave the bridge reading forever (the stdout/stderr reader
        // threads still hold sender clones, so the merged channel never sees
        // `Err`). Queue a disconnect so teardown is guaranteed.
        let _ = self.tx.send(Msg::Cmd(ControlCmd::Disconnect));
    }
}

/// A live Pi session: a [`PiControlHandle`] and the stream of canonical events.
/// The event channel closes when the session ends; the bridge thread is joined
/// on drop of [`PiSession`].
pub struct PiSession {
    pub control: PiControlHandle,
    pub events: mpsc::Receiver<AgentEvent>,
    join: Option<JoinHandle<()>>,
}

impl Drop for PiSession {
    fn drop(&mut self) {
        let _ = self.control.disconnect();
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl PiSession {
    /// Dissolve the session into its control handle and event receiver.
    ///
    /// The bridge thread is detached and exits naturally when the returned
    /// [`PiControlHandle`] is dropped (its `Drop` queues a disconnect → the
    /// bridge tears the subprocess down).
    pub fn into_parts(self) -> (PiControlHandle, mpsc::Receiver<AgentEvent>) {
        let this = std::mem::ManuallyDrop::new(self);
        // SAFETY: `ManuallyDrop` prevents the destructor; we read every field
        // exactly once and handle the JoinHandle explicitly.
        let control = unsafe { std::ptr::read(&this.control) };
        let events = unsafe { std::ptr::read(&this.events) };
        let join = unsafe { std::ptr::read(&this.join) };
        if let Some(j) = join {
            drop(j); // detach
        }
        (control, events)
    }
}

/// Open a bidirectional Pi session, sending `prompt` as the first turn.
///
/// Blocks until Pi acknowledges the first prompt (or fails / the readiness
/// timeout elapses), so spawn and handshake failures surface synchronously as
/// [`PiSessionError::Connect`] rather than as a silently-closed event stream.
pub fn open_pi_session(
    prompt: impl Into<String>,
    options: PiSessionOptions,
) -> Result<PiSession, PiSessionError> {
    let prompt = prompt.into();
    let ctx = ArgvCtx {
        model: options.model.as_ref(),
        yolo: false,
        stream: false,
        events_mode: false,
        session_id: options.session_id.as_deref(),
        envelope: None,
    };
    let argv = pi::argv(ctx);

    let binary = &argv[0];
    let resolved_program = crate::command_resolve::resolve_command(&binary.to_string_lossy());
    let mut cmd = Command::new(resolved_program);
    cmd.args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = &options.cwd {
        cmd.current_dir(dir);
    }
    // Mirrors the one-shot subprocess transport (BUG-4 / ADR 0014): make `pi`
    // the leader of its own process group so teardown can reap tool
    // subprocesses Pi spawned, guaranteeing the pipes reach EOF.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| PiSessionError::Connect(format!("failed to spawn `pi`: {e}")))?;
    let stdin = child.stdin.take().expect("stdin was piped");
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let child = Arc::new(Mutex::new(child));

    let (event_tx, event_rx) = mpsc::channel::<AgentEvent>();
    let (msg_tx, msg_rx) = mpsc::channel::<Msg>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    // A clone of the merged-channel sender the bridge hands to the reader
    // threads; the original stays here and is moved into the control handle on
    // a successful open.
    let reader_tx = msg_tx.clone();

    let join = thread::Builder::new()
        .name("aikit-pi-session".into())
        .spawn(move || {
            run_session(
                prompt, child, stdin, stdout, stderr, event_tx, reader_tx, msg_rx, ready_tx,
            );
        })
        .map_err(|e| PiSessionError::Runtime(e.to_string()))?;

    match ready_rx.recv() {
        Ok(Ok(())) => Ok(PiSession {
            control: PiControlHandle { tx: msg_tx },
            events: event_rx,
            join: Some(join),
        }),
        Ok(Err(msg)) => {
            let _ = join.join();
            Err(PiSessionError::Connect(msg))
        }
        Err(_) => {
            let _ = join.join();
            Err(PiSessionError::Connect(
                "pi session thread terminated before ready".to_string(),
            ))
        }
    }
}

/// The bridge body: spawns the reader threads, sends the first prompt, then
/// runs the two-phase driver and tears the subprocess down on every exit path.
#[allow(clippy::too_many_arguments)]
fn run_session(
    prompt: String,
    child: Arc<Mutex<Child>>,
    mut stdin: ChildStdin,
    stdout: ChildStdout,
    stderr: ChildStderr,
    event_tx: mpsc::Sender<AgentEvent>,
    reader_tx: mpsc::Sender<Msg>,
    msg_rx: mpsc::Receiver<Msg>,
    ready_tx: mpsc::Sender<Result<(), String>>,
) {
    let stdout_tx = reader_tx.clone();
    let stderr_tx = reader_tx;
    let stdout_hdl = spawn_stdout_reader(stdout, stdout_tx);
    let stderr_hdl = spawn_stderr_reader(stderr, stderr_tx);

    // Send the first prompt. A failure here means Pi is unusable before the
    // turn even starts — surface it as a synchronous connect error.
    if let Err(e) = stdin
        .write_all(pi::prompt_command(&prompt).as_bytes())
        .and_then(|_| stdin.flush())
    {
        let msg = format!("failed to send first prompt to pi: {e}");
        let _ = ready_tx.send(Err(msg));
        teardown(child, stdin, stdout_hdl, stderr_hdl, event_tx);
        return;
    }

    let ready_deadline = Instant::now() + READINESS_TIMEOUT;
    drive(&mut stdin, &event_tx, msg_rx, ready_tx, ready_deadline);
    teardown(child, stdin, stdout_hdl, stderr_hdl, event_tx);
}

/// Run the readiness phase (bounded wait for Pi to acknowledge the first
/// prompt) then the long-lived main phase. Returns on every exit path —
/// readiness failure, stdout EOF, `Disconnect`, or a closed channel — so the
/// caller can run a single teardown afterwards. The readiness phase bounds each
/// wait with the time remaining before `ready_deadline`; the main phase blocks
/// indefinitely (the session ends on `Disconnect`, stdout EOF, or a closed
/// channel).
fn drive(
    stdin: &mut ChildStdin,
    event_tx: &mpsc::Sender<AgentEvent>,
    msg_rx: mpsc::Receiver<Msg>,
    ready_tx: mpsc::Sender<Result<(), String>>,
    ready_deadline: Instant,
) {
    let mut seq: u64 = 0;
    let mut ready = false;

    // ── readiness phase ──
    while !ready {
        let remaining = ready_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = ready_tx.send(Err(
                "pi did not acknowledge the prompt within the readiness timeout".to_string(),
            ));
            return;
        }
        match msg_rx.recv_timeout(remaining) {
            Ok(Msg::Frame(v)) => {
                if let Some(outcome) = prompt_response_outcome(&v) {
                    match outcome {
                        Ok(()) => {
                            ready = true;
                            let _ = ready_tx.send(Ok(()));
                        }
                        Err(msg) => {
                            let _ = ready_tx.send(Err(msg));
                            return;
                        }
                    }
                }
                if !emit_frame(event_tx, &mut seq, &v) {
                    return;
                }
            }
            Ok(Msg::Stderr(s)) => {
                if !emit_stderr(event_tx, &mut seq, s) {
                    return;
                }
            }
            Ok(Msg::StdoutEof) => {
                if !ready {
                    let _ = ready_tx.send(Err(
                        "pi closed its output before acknowledging the prompt".to_string(),
                    ));
                }
                return;
            }
            Ok(Msg::Cmd(ControlCmd::Disconnect)) => {
                let _ = ready_tx.send(Err("disconnected before the session was ready".to_string()));
                return;
            }
            Ok(Msg::Cmd(_)) => { /* control before ready: ignore */ }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = ready_tx.send(Err(
                    "pi did not acknowledge the prompt within the readiness timeout".to_string(),
                ));
                return;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = ready_tx.send(Err("pi session channel closed before ready".to_string()));
                return;
            }
        }
    }

    // ── main phase: stream frames, act on control commands ──
    loop {
        match msg_rx.recv() {
            Ok(Msg::Frame(v)) => {
                if !emit_frame(event_tx, &mut seq, &v) {
                    return;
                }
            }
            Ok(Msg::Stderr(s)) => {
                if !emit_stderr(event_tx, &mut seq, s) {
                    return;
                }
            }
            Ok(Msg::StdoutEof) => return,
            Ok(Msg::Cmd(ControlCmd::SendTurn(t))) => {
                write_cmd(stdin, pi::prompt_command(&t), event_tx, &mut seq)
            }
            Ok(Msg::Cmd(ControlCmd::Interrupt)) => {
                write_cmd(stdin, abort_command(), event_tx, &mut seq)
            }
            Ok(Msg::Cmd(ControlCmd::Steer(t))) => {
                write_cmd(stdin, steer_command(&t), event_tx, &mut seq)
            }
            Ok(Msg::Cmd(ControlCmd::FollowUp(t))) => {
                write_cmd(stdin, follow_up_command(&t), event_tx, &mut seq)
            }
            Ok(Msg::Cmd(ControlCmd::Disconnect)) => return,
            Err(_) => return,
        }
    }
}

/// Drop stdin, kill the process group (reaping Pi's tool subprocesses), join
/// the reader threads, then drop the event sender so the caller observes the
/// session end.
fn teardown(
    child: Arc<Mutex<Child>>,
    stdin: ChildStdin,
    stdout_hdl: JoinHandle<()>,
    stderr_hdl: JoinHandle<()>,
    event_tx: mpsc::Sender<AgentEvent>,
) {
    drop(stdin);
    kill_process_group(&child);
    let _ = stdout_hdl.join();
    let _ = stderr_hdl.join();
    drop(event_tx);
}

// ── stdin command framers ────────────────────────────────────────────────────

/// `{"type":"abort"}\n` — aborts the in-flight operation; the session survives.
fn abort_command() -> String {
    format!("{}\n", serde_json::json!({"type":"abort"}))
}

/// `{"type":"steer","message":...}\n` — queued mid-turn steering message.
fn steer_command(message: &str) -> String {
    let cmd = serde_json::json!({"type":"steer","message":message});
    format!("{cmd}\n")
}

/// `{"type":"follow_up","message":...}\n` — queued for after the agent settles.
fn follow_up_command(message: &str) -> String {
    let cmd = serde_json::json!({"type":"follow_up","message":message});
    format!("{cmd}\n")
}

// ── stdout/stderr reader threads ──────────────────────────────────────────────

/// Read Pi's stdout one JSONL record at a time (splitting on `\n` only and
/// stripping an optional trailing `\r`, per the RPC framing spec). Each parsed
/// line is a [`Msg::Frame`]; a non-JSON line is surfaced as stderr. On EOF or
/// read error the thread emits [`Msg::StdoutEof`] and exits.
fn spawn_stdout_reader(reader: ChildStdout, tx: mpsc::Sender<Msg>) -> JoinHandle<()> {
    thread::Builder::new()
        .name("aikit-pi-session-stdout".into())
        .spawn(move || {
            let mut br = BufReader::new(reader);
            loop {
                let mut buf = Vec::new();
                match br.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(Msg::StdoutEof);
                        break;
                    }
                    Ok(_) => {
                        // Strip the trailing newline (and optional CR).
                        if buf.last() == Some(&b'\n') {
                            buf.pop();
                        }
                        if buf.last() == Some(&b'\r') {
                            buf.pop();
                        }
                        let Ok(line) = String::from_utf8(buf) else {
                            continue;
                        };
                        if line.trim().is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<Value>(&line) {
                            Ok(v) => {
                                let _ = tx.send(Msg::Frame(v));
                            }
                            Err(_) => {
                                let _ =
                                    tx.send(Msg::Stderr(format!("non-JSON stdout line: {line}")));
                            }
                        }
                    }
                }
            }
        })
        .expect("spawn pi stdout reader")
}

/// Read Pi's stderr line by line and forward each as a [`Msg::Stderr`]. A
/// stderr EOF is silent: only stdout EOF ends the session.
fn spawn_stderr_reader(reader: ChildStderr, tx: mpsc::Sender<Msg>) -> JoinHandle<()> {
    thread::Builder::new()
        .name("aikit-pi-session-stderr".into())
        .spawn(move || {
            let mut br = BufReader::new(reader);
            loop {
                let mut buf = String::new();
                match br.read_line(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let _ = tx.send(Msg::Stderr(buf.trim_end().to_string()));
                    }
                }
            }
        })
        .expect("spawn pi stderr reader")
}

// ── event emission ───────────────────────────────────────────────────────────

/// Map a [`Decoded`] frame to its canonical event payload.
fn decoded_to_payload(frame: Decoded) -> AgentEventPayload {
    match frame {
        Decoded::Stream(m) => AgentEventPayload::StreamMessage(m),
        Decoded::ToolUse {
            call_id,
            tool_name,
            input,
        } => AgentEventPayload::ToolUse {
            call_id,
            tool_name,
            input,
        },
        Decoded::ToolResult {
            call_id,
            output,
            is_error,
        } => AgentEventPayload::ToolResult {
            call_id,
            output,
            is_error,
        },
    }
}

fn send_event(
    event_tx: &mpsc::Sender<AgentEvent>,
    seq: &mut u64,
    stream: AgentEventStream,
    payload: AgentEventPayload,
) -> bool {
    let ev = AgentEvent {
        agent_key: pi::KEY.to_string(),
        seq: *seq,
        stream,
        payload,
    };
    *seq += 1;
    event_tx.send(ev).is_ok()
}

/// Emit the canonical events for one Pi frame: decoded stream/tool frames,
/// then a token-usage event when the frame carries usage, then a step-finish
/// event when Pi reports the run settled. Returns `false` if the caller has
/// gone away (the event channel closed) so the bridge can stop.
fn emit_frame(event_tx: &mpsc::Sender<AgentEvent>, seq: &mut u64, value: &Value) -> bool {
    let stream = AgentEventStream::Stdout;
    for frame in pi::decode(value, stream, *seq) {
        if !send_event(event_tx, seq, stream, decoded_to_payload(frame)) {
            return false;
        }
    }
    if let Some((usage, source)) = pi::extract_usage(value) {
        if !send_event(
            event_tx,
            seq,
            stream,
            AgentEventPayload::TokenUsageLine {
                usage,
                source,
                raw_agent_line_seq: *seq,
            },
        ) {
            return false;
        }
    }
    if pi::is_settled_event(value)
        && !send_event(
            event_tx,
            seq,
            stream,
            AgentEventPayload::AikitStepFinish {
                iteration: 0,
                finish_reason: "turn_completed".into(),
            },
        )
    {
        return false;
    }
    true
}

fn emit_stderr(event_tx: &mpsc::Sender<AgentEvent>, seq: &mut u64, line: String) -> bool {
    if line.is_empty() {
        return true;
    }
    send_event(
        event_tx,
        seq,
        AgentEventStream::Stderr,
        AgentEventPayload::RawLine(line),
    )
}

/// Write a framed command to Pi's stdin. A `BrokenPipe` means Pi has gone away;
/// the stdout reader will deliver EOF and end the session, so it is not logged.
/// Any other write error is surfaced as a stderr event for diagnostics.
fn write_cmd(
    stdin: &mut ChildStdin,
    bytes: String,
    event_tx: &mpsc::Sender<AgentEvent>,
    seq: &mut u64,
) {
    if let Err(e) = stdin
        .write_all(bytes.as_bytes())
        .and_then(|_| stdin.flush())
    {
        if e.kind() != io::ErrorKind::BrokenPipe {
            let _ = emit_stderr(event_tx, seq, format!("pi stdin write failed: {e}"));
        }
    }
}

/// Inspect a frame for the first prompt's acknowledgement. Returns:
/// - `Some(Ok(()))` when it is the `prompt` command's success response,
/// - `Some(Err(msg))` when Pi rejected the prompt,
/// - `None` for any other frame.
fn prompt_response_outcome(value: &Value) -> Option<Result<(), String>> {
    if value.get("type").and_then(|v| v.as_str()) != Some("response") {
        return None;
    }
    if value.get("command").and_then(|v| v.as_str()) != Some("prompt") {
        return None;
    }
    let success = value
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if success {
        Some(Ok(()))
    } else {
        let msg = value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("pi rejected the prompt")
            .to_string();
        Some(Err(msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::types::MessageKind;

    fn json(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn send_turn_interrupt_steer_follow_up_queue_correctly() {
        let (tx, rx) = mpsc::channel::<Msg>();
        let h = PiControlHandle { tx };
        h.send_turn("hello again").unwrap();
        h.interrupt().unwrap();
        h.steer("while you're at it").unwrap();
        h.follow_up("then do this").unwrap();

        match rx.recv().unwrap() {
            Msg::Cmd(ControlCmd::SendTurn(t)) => assert_eq!(t, "hello again"),
            other => panic!("expected SendTurn, got {other:?}"),
        }
        assert!(matches!(
            rx.recv().unwrap(),
            Msg::Cmd(ControlCmd::Interrupt)
        ));
        match rx.recv().unwrap() {
            Msg::Cmd(ControlCmd::Steer(t)) => assert_eq!(t, "while you're at it"),
            other => panic!("expected Steer, got {other:?}"),
        }
        match rx.recv().unwrap() {
            Msg::Cmd(ControlCmd::FollowUp(t)) => assert_eq!(t, "then do this"),
            other => panic!("expected FollowUp, got {other:?}"),
        }
    }

    #[test]
    fn control_handle_send_after_close_errors() {
        let (tx, rx) = mpsc::channel::<Msg>();
        let h = PiControlHandle { tx };
        drop(rx);
        assert!(matches!(
            h.send_turn("x").err(),
            Some(PiSessionError::Closed)
        ));
        assert!(matches!(h.interrupt().err(), Some(PiSessionError::Closed)));
    }

    #[test]
    fn dropping_control_handle_queues_disconnect() {
        let (tx, rx) = mpsc::channel::<Msg>();
        let h = PiControlHandle { tx };
        drop(h);
        match rx.recv().unwrap() {
            Msg::Cmd(ControlCmd::Disconnect) => {}
            other => panic!("expected Disconnect on drop, got {other:?}"),
        }
    }

    #[test]
    fn stdin_framers_match_the_rpc_protocol() {
        // Each command is one JSONL record terminated by `\n`; the object's
        // field set matters, not its key order (`serde_json` emits keys
        // alphabetically, and Pi parses the JSON regardless of order).
        fn parse(cmd: String) -> Value {
            assert!(cmd.ends_with('\n'), "command must be newline-terminated");
            serde_json::from_str(cmd.trim_end()).unwrap()
        }

        assert_eq!(parse(abort_command()), json(r#"{"type":"abort"}"#));
        assert_eq!(
            parse(steer_command("stop now")),
            json(r#"{"type":"steer","message":"stop now"}"#)
        );
        assert_eq!(
            parse(follow_up_command("afterwards")),
            json(r#"{"type":"follow_up","message":"afterwards"}"#)
        );
        // The shared prompt framer uses the `message` field (PR #128).
        assert_eq!(
            parse(pi::prompt_command("hi")),
            json(r#"{"type":"prompt","message":"hi"}"#)
        );
    }

    #[test]
    fn prompt_response_outcome_classifies_frames() {
        assert!(matches!(
            prompt_response_outcome(&json(
                r#"{"type":"response","command":"prompt","success":true}"#
            )),
            Some(Ok(()))
        ));
        assert!(matches!(
            prompt_response_outcome(&json(
                r#"{"type":"response","command":"prompt","success":false,"error":"no auth"}"#
            )),
            Some(Err(msg)) if msg == "no auth"
        ));
        // Non-prompt responses and non-response frames are not readiness signals.
        assert!(prompt_response_outcome(&json(
            r#"{"type":"response","command":"steer","success":true}"#
        ))
        .is_none());
        assert!(prompt_response_outcome(&json(r#"{"type":"agent_start"}"#)).is_none());
    }

    #[test]
    fn decoded_to_payload_maps_every_variant() {
        let s = decoded_to_payload(Decoded::Stream(crate::runner::types::StreamMessage {
            text: "hi".into(),
            phase: crate::runner::types::MessagePhase::Delta,
            role: crate::runner::types::MessageRole::Assistant,
            kind: MessageKind::Message,
            source: AgentEventStream::Stdout,
            raw_line_seq: 0,
            turn_id: None,
        }));
        assert!(matches!(s, AgentEventPayload::StreamMessage(_)));

        let u = decoded_to_payload(Decoded::ToolUse {
            call_id: "c1".into(),
            tool_name: "bash".into(),
            input: serde_json::json!({"command":"ls"}),
        });
        match u {
            AgentEventPayload::ToolUse { tool_name, .. } => assert_eq!(tool_name, "bash"),
            other => panic!("expected ToolUse, got {other:?}"),
        }

        let r = decoded_to_payload(Decoded::ToolResult {
            call_id: "c1".into(),
            output: serde_json::json!("ok"),
            is_error: false,
        });
        assert!(matches!(
            r,
            AgentEventPayload::ToolResult {
                is_error: false,
                ..
            }
        ));
    }

    #[test]
    fn emit_frame_surfaces_decoded_usage_and_step_finish() {
        let (tx, rx) = mpsc::channel::<AgentEvent>();
        let mut seq = 0u64;

        // A streaming text delta → one StreamMessage.
        assert!(emit_frame(
            &tx,
            &mut seq,
            &json(
                r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"He"}}"#
            )
        ));
        // agent_settled → a StepFinish frame.
        assert!(emit_frame(
            &tx,
            &mut seq,
            &json(r#"{"type":"agent_settled"}"#)
        ));
        drop(tx);

        let events: Vec<_> = rx.into_iter().collect();
        use crate::runner::types::{MessagePhase, MessageRole};
        match &events[0].payload {
            AgentEventPayload::StreamMessage(m) => {
                assert_eq!(m.text, "He");
                assert_eq!(m.phase, MessagePhase::Delta);
                assert_eq!(m.role, MessageRole::Assistant);
            }
            other => panic!("expected StreamMessage, got {other:?}"),
        }
        assert!(matches!(
            events[1].payload,
            AgentEventPayload::AikitStepFinish { ref finish_reason, .. }
                if finish_reason == "turn_completed"
        ));
    }

    #[test]
    fn emit_frame_stops_when_the_caller_drops() {
        let (tx, rx) = mpsc::channel::<AgentEvent>();
        drop(rx);
        let mut seq = 0u64;
        assert!(!emit_frame(
            &tx,
            &mut seq,
            &json(r#"{"type":"agent_settled"}"#)
        ));
    }

    #[test]
    fn options_debug_lists_fields() {
        let opts = PiSessionOptions {
            model: Some("anthropic/claude".into()),
            cwd: Some(PathBuf::from("/tmp")),
            session_id: Some("abc".into()),
        };
        let s = format!("{opts:?}");
        assert!(s.contains("PiSessionOptions"));
        assert!(s.contains("anthropic/claude"));
        assert!(s.contains("/tmp"));
        assert!(s.contains("abc"));
    }

    #[test]
    fn error_display_names_the_tier() {
        assert_eq!(
            PiSessionError::Connect("no auth".into()).to_string(),
            "pi session connect error: no auth"
        );
        assert_eq!(
            PiSessionError::Closed.to_string(),
            "pi session control channel closed"
        );
    }

    // Live end-to-end smoke test against a real `pi`. Ignored by default (needs
    // the CLI + credentials); run with
    // `cargo test -p aikit-sdk --lib pi_session::tests::live_ -- --ignored`.
    //
    // Proves the session lifecycle: open + readiness handshake, turn 1 streams
    // text and settles (`agent_settled`), then a second `prompt` (send_turn)
    // drives turn 2 on the *same* session — which only works because stdin is
    // held open for the session lifetime. A bare `prompt` sent while Pi is
    // still streaming is rejected by the RPC, so the follow-up is queued only
    // after turn 1's StepFinish (Pi is idle then).
    #[test]
    #[ignore = "requires a real `pi` CLI on PATH and credentials"]
    fn live_session_streams_and_multi_turns() {
        use std::time::{Duration, Instant};

        let session = open_pi_session(
            "Reply with exactly one word: pong. Do not use any tools.",
            PiSessionOptions::default(),
        )
        .expect("connect to live pi session");

        let mut turn1_text = false;
        let mut sent_turn2 = false;
        let mut turn2_text = false;
        let mut turn2_settled = false;
        let deadline = Instant::now() + Duration::from_secs(90);
        while Instant::now() < deadline {
            match session.events.recv_timeout(Duration::from_secs(5)) {
                Ok(ev) => {
                    eprintln!("LIVE PI EVENT seq={} {:?}", ev.seq, ev.payload);
                    if let AgentEventPayload::StreamMessage(m) = &ev.payload {
                        if !m.text.trim().is_empty() {
                            if !sent_turn2 {
                                turn1_text = true;
                            } else {
                                turn2_text = true;
                            }
                        }
                    }
                    if matches!(ev.payload, AgentEventPayload::AikitStepFinish { .. }) {
                        if !sent_turn2 {
                            // Turn 1 settled → Pi is idle → a bare second
                            // prompt is accepted (no `streamingBehavior` needed).
                            sent_turn2 = true;
                            let _ = session
                                .control
                                .send_turn("Now reply with exactly one word: ping.");
                        } else {
                            turn2_settled = true;
                            break;
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let _ = session.control.disconnect();
        assert!(turn1_text, "turn 1 should have streamed text");
        assert!(
            turn2_text,
            "turn 2 (multi-turn follow-up) should have streamed text"
        );
        assert!(turn2_settled, "turn 2 should have settled");
    }
}
