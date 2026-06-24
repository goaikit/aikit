//! Codex app-server sessions over a tokio bridge.
//!
//! The Codex analogue of [`claude_session`](super::claude_session): a dedicated
//! thread runs a current-thread tokio runtime driving `aikit-agent-codex`'s
//! [`CodexClient`] (JSON-RPC over `codex app-server` stdio) — spawn →
//! `initialize` → `thread/start` → `turn/start` — forwarding server
//! notifications as canonical [`AgentEvent`]s, while a [`CodexControlHandle`]
//! issues interrupt / steer / disconnect.
//!
//! `CodexClient::spawn` already hands back the inbound notification receiver
//! separately from the control client, so a single-task `tokio::select!`
//! multiplexes the two without a second task. Server→client approval requests
//! are auto-approved for now (a permission-callback seam, like the Claude
//! session's, is a follow-up). See spec 007.

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use aikit_agent_codex::{
    CodexClient, ServerMessage, ServerNotificationKind, SpawnOptions, ThreadId, TurnId,
};
use serde_json::{json, Value};

use crate::runner::backend::Decoded;
use crate::runner::types::{
    AgentEvent, AgentEventPayload, AgentEventStream, MessageKind, MessagePhase, MessageRole,
    StreamMessage,
};

/// Options for opening a Codex session.
#[derive(Debug, Clone)]
pub struct CodexSessionOptions {
    /// Working directory for the thread (and the spawned `codex` process).
    pub cwd: PathBuf,
    /// Approval policy: `never` (auto), `on-request`, `on-failure`, `untrusted`.
    pub approval_policy: String,
    /// Sandbox mode: e.g. `read-only`, `workspace-write`, `danger-full-access`.
    pub sandbox: String,
}

impl Default for CodexSessionOptions {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            // Default to non-interactive auto-approval (matches how aikit runs
            // codex via `--yolo` today); approvals never block the stream.
            approval_policy: "never".to_string(),
            sandbox: "workspace-write".to_string(),
        }
    }
}

/// Errors opening or driving a Codex session.
#[derive(Debug)]
pub enum CodexSessionError {
    /// The tokio runtime could not be created.
    Runtime(String),
    /// Spawning / handshaking with `codex app-server` failed.
    Connect(String),
    /// The control channel is closed (the session ended).
    Closed,
}

impl std::fmt::Display for CodexSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodexSessionError::Runtime(e) => write!(f, "codex session runtime error: {e}"),
            CodexSessionError::Connect(e) => write!(f, "codex session connect error: {e}"),
            CodexSessionError::Closed => write!(f, "codex session control channel closed"),
        }
    }
}

impl std::error::Error for CodexSessionError {}

/// Commands forwarded from a [`CodexControlHandle`] to the session bridge.
enum ControlCmd {
    Interrupt,
    Steer(String),
    Disconnect,
}

/// A sync handle to drive a live Codex session.
pub struct CodexControlHandle {
    tx: tokio::sync::mpsc::UnboundedSender<ControlCmd>,
}

impl CodexControlHandle {
    fn send(&self, cmd: ControlCmd) -> Result<(), CodexSessionError> {
        self.tx.send(cmd).map_err(|_| CodexSessionError::Closed)
    }

    /// Interrupt the in-flight turn.
    pub fn interrupt(&self) -> Result<(), CodexSessionError> {
        self.send(ControlCmd::Interrupt)
    }

    /// Steer the in-flight turn by appending input.
    pub fn steer(&self, text: impl Into<String>) -> Result<(), CodexSessionError> {
        self.send(ControlCmd::Steer(text.into()))
    }

    /// End the session.
    pub fn disconnect(&self) -> Result<(), CodexSessionError> {
        self.send(ControlCmd::Disconnect)
    }
}

/// A live Codex session: a [`CodexControlHandle`] and the canonical event
/// stream. The event channel closes when the session ends; the bridge thread is
/// joined on drop.
pub struct CodexSession {
    pub control: CodexControlHandle,
    pub events: mpsc::Receiver<AgentEvent>,
    join: Option<thread::JoinHandle<()>>,
}

impl Drop for CodexSession {
    fn drop(&mut self) {
        let _ = self.control.disconnect();
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Open a Codex session, sending `prompt` as the first turn.
///
/// Blocks until connected and the first turn is started, so spawn/handshake
/// failures return [`CodexSessionError::Connect`] rather than only closing the
/// event stream.
pub fn open_codex_session(
    prompt: impl Into<String>,
    options: CodexSessionOptions,
) -> Result<CodexSession, CodexSessionError> {
    let prompt = prompt.into();
    let (event_tx, event_rx) = mpsc::channel::<AgentEvent>();
    let (control_tx, control_rx) = tokio::sync::mpsc::unbounded_channel::<ControlCmd>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

    let join = thread::Builder::new()
        .name("aikit-codex-session".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("failed to build runtime: {e}")));
                    return;
                }
            };
            rt.block_on(run_session(prompt, options, event_tx, control_rx, ready_tx));
        })
        .map_err(|e| CodexSessionError::Runtime(e.to_string()))?;

    match ready_rx.recv() {
        Ok(Ok(())) => Ok(CodexSession {
            control: CodexControlHandle { tx: control_tx },
            events: event_rx,
            join: Some(join),
        }),
        Ok(Err(msg)) => {
            let _ = join.join();
            Err(CodexSessionError::Connect(msg))
        }
        Err(_) => {
            let _ = join.join();
            Err(CodexSessionError::Connect(
                "session thread terminated before ready".to_string(),
            ))
        }
    }
}

async fn run_session(
    prompt: String,
    options: CodexSessionOptions,
    event_tx: mpsc::Sender<AgentEvent>,
    mut control_rx: tokio::sync::mpsc::UnboundedReceiver<ControlCmd>,
    ready_tx: mpsc::Sender<Result<(), String>>,
) {
    let spawn_opts = SpawnOptions {
        cwd: Some(options.cwd.clone()),
        ..SpawnOptions::default()
    };
    let (client, mut events) = match CodexClient::spawn_with(spawn_opts).await {
        Ok(pair) => pair,
        Err(e) => {
            let _ = ready_tx.send(Err(format!("spawn failed: {e}")));
            return;
        }
    };

    if let Err(e) = client
        .initialize("aikit", "aikit", env!("CARGO_PKG_VERSION"))
        .await
    {
        let _ = ready_tx.send(Err(format!("initialize failed: {e}")));
        return;
    }

    let thread_id: ThreadId = match client
        .thread_start_simple(&options.cwd, &options.approval_policy, &options.sandbox)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            let _ = ready_tx.send(Err(format!("thread/start failed: {e}")));
            return;
        }
    };

    let mut turn_id: TurnId = match client.turn_start(&thread_id, &prompt).await {
        Ok(id) => id,
        Err(e) => {
            let _ = ready_tx.send(Err(format!("turn/start failed: {e}")));
            return;
        }
    };

    let _ = ready_tx.send(Ok(()));

    let mut seq: u64 = 0;
    let mut closed = false;
    loop {
        tokio::select! {
            msg = events.recv() => {
                match msg {
                    Some(ServerMessage::Notification(n)) => {
                        for frame in map_notification(&n.method, &n.params, seq) {
                            if event_tx
                                .send(AgentEvent {
                                    agent_key: "codex".to_string(),
                                    seq,
                                    stream: AgentEventStream::Stdout,
                                    payload: decoded_to_payload(frame),
                                })
                                .is_err()
                            {
                                closed = true;
                                break;
                            }
                            seq += 1;
                        }
                    }
                    // Server→client request (e.g. an approval prompt). Auto-approve
                    // for now — a permission-callback seam is a follow-up.
                    Some(ServerMessage::ServerRequest(req)) => {
                        let _ = client
                            .reply_server_request(req.id, json!({ "outcome": "approved" }))
                            .await;
                    }
                    None => break, // app-server closed the stream
                }
                if closed {
                    break;
                }
            }
            cmd = control_rx.recv() => {
                match cmd {
                    Some(ControlCmd::Interrupt) => {
                        let _ = client.turn_interrupt(&thread_id, &turn_id).await;
                    }
                    Some(ControlCmd::Steer(text)) => {
                        if let Ok(id) = client.turn_steer(&thread_id, &text).await {
                            turn_id = id;
                        }
                    }
                    Some(ControlCmd::Disconnect) | None => break,
                }
            }
        }
    }

    let _ = client.shutdown().await;
}

/// Map one app-server notification to canonical [`Decoded`] frames.
fn map_notification(method: &str, params: &Value, raw_line_seq: u64) -> Vec<Decoded> {
    let mk = |text: String, phase: MessagePhase, role: MessageRole, kind: MessageKind| {
        Decoded::Stream(StreamMessage {
            text,
            phase,
            role,
            kind,
            source: AgentEventStream::Stdout,
            raw_line_seq,
            turn_id: None,
        })
    };

    match ServerNotificationKind::from_method(method) {
        ServerNotificationKind::AgentMessageDelta => params
            .get("delta")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| {
                vec![mk(
                    s.to_string(),
                    MessagePhase::Delta,
                    MessageRole::Assistant,
                    MessageKind::Message,
                )]
            })
            .unwrap_or_default(),
        ServerNotificationKind::CommandExecutionOutputDelta => params
            .get("delta")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| {
                vec![mk(
                    s.to_string(),
                    MessagePhase::Delta,
                    MessageRole::Tool,
                    MessageKind::ToolOutput,
                )]
            })
            .unwrap_or_default(),
        ServerNotificationKind::ItemCompleted => {
            // params may wrap the item under `item`, or carry it inline.
            let item = params.get("item").unwrap_or(params);
            map_item(item, raw_line_seq)
        }
        _ => Vec::new(),
    }
}

/// Map a completed `item` (agent_message / reasoning / command_execution /
/// file_change) to canonical frames. Mirrors the `codex exec` item schema but
/// emits structured tool frames where the app-server provides them.
fn map_item(item: &Value, raw_line_seq: u64) -> Vec<Decoded> {
    let mk = |text: String, phase: MessagePhase, role: MessageRole, kind: MessageKind| {
        Decoded::Stream(StreamMessage {
            text,
            phase,
            role,
            kind,
            source: AgentEventStream::Stdout,
            raw_line_seq,
            turn_id: None,
        })
    };
    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match item_type {
        "agent_message" => item
            .get("text")
            .and_then(|v| v.as_str())
            .map(|t| {
                vec![mk(
                    t.to_string(),
                    MessagePhase::Final,
                    MessageRole::Assistant,
                    MessageKind::Message,
                )]
            })
            .unwrap_or_default(),
        "reasoning" => item
            .get("text")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("summary").and_then(|v| v.as_str()))
            .map(|t| {
                vec![mk(
                    t.to_string(),
                    MessagePhase::Final,
                    MessageRole::Assistant,
                    MessageKind::Reasoning,
                )]
            })
            .unwrap_or_default(),
        "command_execution" => {
            let mut out = Vec::new();
            if let Some(cmd) = item.get("command").and_then(|v| v.as_str()) {
                out.push(Decoded::ToolUse {
                    call_id: item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    tool_name: "shell".to_string(),
                    input: json!({ "command": cmd }),
                });
            }
            if let Some(output) = item.get("aggregated_output").and_then(|v| v.as_str()) {
                if !output.trim().is_empty() {
                    out.push(Decoded::ToolResult {
                        call_id: item
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        output: json!(output),
                        is_error: item.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(0) != 0,
                    });
                }
            }
            out
        }
        "file_change" => {
            let summary = item
                .get("changes")
                .and_then(|c| c.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| {
                            let path = c.get("path").and_then(|v| v.as_str())?;
                            let kind = c.get("kind").and_then(|v| v.as_str()).unwrap_or("change");
                            Some(format!("{kind} {path}"))
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            if summary.is_empty() {
                Vec::new()
            } else {
                vec![mk(
                    format!("file_change: {summary}"),
                    MessagePhase::Final,
                    MessageRole::Tool,
                    MessageKind::Message,
                )]
            }
        }
        // Unknown item: surface any text it carries.
        _ => item
            .get("text")
            .and_then(|v| v.as_str())
            .map(|t| {
                vec![mk(
                    t.to_string(),
                    MessagePhase::Final,
                    MessageRole::Assistant,
                    MessageKind::Message,
                )]
            })
            .unwrap_or_default(),
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_agent_message_delta() {
        let out = map_notification("item/agentMessage/delta", &json!({"delta": "Hel"}), 3);
        assert_eq!(out.len(), 1);
        match &out[0] {
            Decoded::Stream(m) => {
                assert_eq!(m.text, "Hel");
                assert_eq!(m.phase, MessagePhase::Delta);
                assert_eq!(m.kind, MessageKind::Message);
                assert_eq!(m.raw_line_seq, 3);
            }
            other => panic!("expected Stream, got {other:?}"),
        }
    }

    #[test]
    fn maps_item_completed_reasoning_and_command() {
        let reasoning = map_notification(
            "item/completed",
            &json!({"item": {"type": "reasoning", "text": "thinking"}}),
            0,
        );
        assert!(matches!(
            &reasoning[0],
            Decoded::Stream(m) if m.kind == MessageKind::Reasoning && m.text == "thinking"
        ));

        let cmd = map_notification(
            "item/completed",
            &json!({"item": {
                "id": "i1", "type": "command_execution",
                "command": "ls -la", "aggregated_output": "file.txt\n", "exit_code": 0
            }}),
            0,
        );
        assert_eq!(cmd.len(), 2, "tool use + result; got {cmd:?}");
        assert!(matches!(&cmd[0], Decoded::ToolUse { tool_name, .. } if tool_name == "shell"));
        assert!(matches!(
            &cmd[1],
            Decoded::ToolResult {
                is_error: false,
                ..
            }
        ));
    }

    #[test]
    fn maps_command_failure_as_error_result() {
        let cmd = map_notification(
            "item/completed",
            &json!({"item": {
                "id": "i2", "type": "command_execution",
                "command": "false", "aggregated_output": "boom", "exit_code": 1
            }}),
            0,
        );
        assert!(matches!(
            cmd.last().unwrap(),
            Decoded::ToolResult { is_error: true, .. }
        ));
    }

    #[test]
    fn lifecycle_and_unknown_notifications_yield_nothing() {
        assert!(map_notification("turn/started", &json!({}), 0).is_empty());
        assert!(map_notification("turn/completed", &json!({}), 0).is_empty());
        assert!(map_notification("thread/started", &json!({}), 0).is_empty());
    }

    #[test]
    fn control_handle_send_after_close_errors() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ControlCmd>();
        let h = CodexControlHandle { tx };
        drop(rx);
        assert!(matches!(h.interrupt(), Err(CodexSessionError::Closed)));
    }

    // Live end-to-end smoke test against a real `codex`. Ignored by default;
    // run with `cargo test --features codex-app-server -- --ignored live_`.
    #[test]
    #[ignore = "requires a real `codex` CLI on PATH and credentials"]
    fn live_session_streams_and_interrupts() {
        use std::time::{Duration, Instant};

        let session = open_codex_session(
            "Say hello in exactly one word.",
            CodexSessionOptions::default(),
        )
        .expect("connect to live codex session");

        let deadline = Instant::now() + Duration::from_secs(60);
        let mut saw_output = false;
        let mut interrupted = false;
        while Instant::now() < deadline {
            match session.events.recv_timeout(Duration::from_secs(5)) {
                Ok(ev) => {
                    eprintln!("LIVE CODEX EVENT seq={} {:?}", ev.seq, ev.payload);
                    saw_output = true;
                    if !interrupted {
                        let _ = session.control.interrupt();
                        interrupted = true;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) if saw_output => break,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        assert!(
            saw_output,
            "expected at least one event from a live codex session"
        );
    }
}
