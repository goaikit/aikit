//! Phase B2: bidirectional Claude sessions over a tokio bridge.
//!
//! aikit's runner is synchronous; `claude-agent-sdk`'s client is async. This
//! module bridges them: a dedicated thread runs a current-thread tokio runtime
//! that drives the SDK's lower-level [`Query`] (the same object
//! `ClaudeSDKClient` uses internally), forwarding typed messages as canonical
//! [`AgentEvent`]s into a sync channel the caller reads, while a [`ControlHandle`]
//! sends interrupt / permission-mode / model commands back to the session.
//!
//! Concurrency: `Query::take_receiver()` yields the inbound half for the read
//! task; the `Query` itself (wrapped in `Arc<Mutex>`) serves the control task —
//! the two never contend (mirrors `ClaudeSDKClient::connect`). See spec 007 (B2).

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use claude_agent_sdk::{
    parse_message, ClaudeAgentOptions, Query, QueryConfig, SubprocessCLITransport, Transport,
};

use crate::runner::backend::Decoded;
use crate::runner::backends::claude::map_message;
use crate::runner::types::{AgentEvent, AgentEventPayload, AgentEventStream};

/// The Claude permission mode, re-exported from `claude-agent-sdk`.
pub use claude_agent_sdk::PermissionMode as ClaudePermissionMode;

/// Options for opening a Claude session. A focused subset of the SDK's options;
/// extend as B2/B3 needs grow.
#[derive(Debug, Clone, Default)]
pub struct ClaudeSessionOptions {
    /// Model identifier (e.g. `claude-opus-4-5`). `None` = CLI default.
    pub model: Option<String>,
    /// Working directory for the spawned `claude` process.
    pub cwd: Option<PathBuf>,
    /// Resume an existing session by id.
    pub resume: Option<String>,
    /// Initial permission mode.
    pub permission_mode: Option<ClaudePermissionMode>,
}

/// Errors opening or driving a Claude session.
#[derive(Debug)]
pub enum ClaudeSessionError {
    /// The tokio runtime could not be created.
    Runtime(String),
    /// The control channel is closed (the session ended).
    Closed,
}

impl std::fmt::Display for ClaudeSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClaudeSessionError::Runtime(e) => write!(f, "session runtime error: {e}"),
            ClaudeSessionError::Closed => write!(f, "session control channel closed"),
        }
    }
}

impl std::error::Error for ClaudeSessionError {}

/// Commands forwarded from a [`ControlHandle`] to the session bridge.
enum ControlCmd {
    Interrupt,
    SetPermissionMode(ClaudePermissionMode),
    SetModel(Option<String>),
    Disconnect,
}

/// A sync handle to drive a live Claude session. Commands are forwarded to the
/// async bridge; methods return once the command is queued (fire-and-forget),
/// not once the CLI has acted on it.
pub struct ControlHandle {
    tx: tokio::sync::mpsc::UnboundedSender<ControlCmd>,
}

impl ControlHandle {
    fn send(&self, cmd: ControlCmd) -> Result<(), ClaudeSessionError> {
        self.tx.send(cmd).map_err(|_| ClaudeSessionError::Closed)
    }

    /// Interrupt the current turn.
    pub fn interrupt(&self) -> Result<(), ClaudeSessionError> {
        self.send(ControlCmd::Interrupt)
    }

    /// Change the permission mode mid-session.
    pub fn set_permission_mode(
        &self,
        mode: ClaudePermissionMode,
    ) -> Result<(), ClaudeSessionError> {
        self.send(ControlCmd::SetPermissionMode(mode))
    }

    /// Switch the model mid-session (`None` resets to default).
    pub fn set_model(&self, model: Option<String>) -> Result<(), ClaudeSessionError> {
        self.send(ControlCmd::SetModel(model))
    }

    /// Disconnect the session.
    pub fn disconnect(&self) -> Result<(), ClaudeSessionError> {
        self.send(ControlCmd::Disconnect)
    }
}

/// A live Claude session: a [`ControlHandle`] and the stream of canonical
/// events. The event channel closes when the session ends; the bridge thread is
/// joined on drop of [`ClaudeSession`].
pub struct ClaudeSession {
    pub control: ControlHandle,
    pub events: mpsc::Receiver<AgentEvent>,
    join: Option<thread::JoinHandle<()>>,
}

impl Drop for ClaudeSession {
    fn drop(&mut self) {
        // Best-effort: ask the bridge to stop, then join.
        let _ = self.control.disconnect();
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Open a bidirectional Claude session, sending `prompt` as the first turn.
///
/// Returns immediately; connection/handshake happen on the bridge thread.
/// Connection failures surface as a final stderr `RawLine` event followed by
/// channel close (rather than a synchronous error), so callers drive the same
/// event loop regardless.
pub fn open_claude_session(
    prompt: impl Into<String>,
    options: ClaudeSessionOptions,
) -> Result<ClaudeSession, ClaudeSessionError> {
    let prompt = prompt.into();
    let (event_tx, event_rx) = mpsc::channel::<AgentEvent>();
    let (control_tx, control_rx) = tokio::sync::mpsc::unbounded_channel::<ControlCmd>();

    let join = thread::Builder::new()
        .name("aikit-claude-session".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    emit_error(&event_tx, format!("failed to build runtime: {e}"));
                    return;
                }
            };
            rt.block_on(run_session(prompt, options, event_tx, control_rx));
        })
        .map_err(|e| ClaudeSessionError::Runtime(e.to_string()))?;

    Ok(ClaudeSession {
        control: ControlHandle { tx: control_tx },
        events: event_rx,
        join: Some(join),
    })
}

fn build_options(options: &ClaudeSessionOptions) -> ClaudeAgentOptions {
    ClaudeAgentOptions {
        model: options.model.clone(),
        cwd: options.cwd.clone(),
        resume: options.resume.clone(),
        permission_mode: options.permission_mode.clone(),
        ..ClaudeAgentOptions::default()
    }
}

async fn run_session(
    prompt: String,
    options: ClaudeSessionOptions,
    event_tx: mpsc::Sender<AgentEvent>,
    mut control_rx: tokio::sync::mpsc::UnboundedReceiver<ControlCmd>,
) {
    let opts = build_options(&options);
    let mut transport = SubprocessCLITransport::new(opts.clone());
    if let Err(e) = transport.connect().await {
        emit_error(&event_tx, format!("connect failed: {e}"));
        return;
    }

    let mut query = match Query::new(Box::new(transport), QueryConfig::default()) {
        Ok(q) => q,
        Err(e) => {
            emit_error(&event_tx, format!("query init failed: {e}"));
            return;
        }
    };
    query.start();
    if let Err(e) = query.initialize().await {
        emit_error(&event_tx, format!("initialize failed: {e}"));
        return;
    }
    if let Err(e) = query.write_user_message(&prompt).await {
        emit_error(&event_tx, format!("send prompt failed: {e}"));
        return;
    }
    let mut rx = match query.take_receiver() {
        Some(rx) => rx,
        None => {
            emit_error(&event_tx, "message receiver unavailable".to_string());
            return;
        }
    };

    // Single task owns both `query` (control) and `rx` (reading); a `select!`
    // multiplexes inbound messages and control commands. `Query` is not `Send`,
    // so we deliberately avoid spawning a second task — `rx` and `query` are
    // distinct bindings, so neither borrow conflicts in the select.
    let mut seq: u64 = 0;
    let mut closed = false;
    loop {
        tokio::select! {
            item = rx.recv() => {
                match item {
                    Some(Ok(value)) => {
                        let ty = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if ty == "control_request" || ty == "control_response" {
                            continue;
                        }
                        if let Ok(Some(msg)) = parse_message(&value) {
                            for frame in map_message(msg, AgentEventStream::Stdout, seq) {
                                let payload = decoded_to_payload(frame);
                                if event_tx
                                    .send(AgentEvent {
                                        agent_key: "claude".to_string(),
                                        seq,
                                        stream: AgentEventStream::Stdout,
                                        payload,
                                    })
                                    .is_err()
                                {
                                    closed = true;
                                    break;
                                }
                                seq += 1;
                            }
                        }
                    }
                    Some(Err(e)) => {
                        emit_error(&event_tx, format!("stream error: {e}"));
                        break;
                    }
                    None => break, // stream ended
                }
                if closed {
                    break;
                }
            }
            cmd = control_rx.recv() => {
                match cmd {
                    Some(ControlCmd::Interrupt) => {
                        let _ = query.interrupt().await;
                    }
                    Some(ControlCmd::SetPermissionMode(mode)) => {
                        let _ = query.set_permission_mode(mode).await;
                    }
                    Some(ControlCmd::SetModel(model)) => {
                        let _ = query.set_model(model.as_deref()).await;
                    }
                    Some(ControlCmd::Disconnect) | None => break,
                }
            }
        }
    }

    let _ = query.close().await;
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

fn emit_error(event_tx: &mpsc::Sender<AgentEvent>, message: String) {
    let _ = event_tx.send(AgentEvent {
        agent_key: "claude".to_string(),
        seq: u64::MAX,
        stream: AgentEventStream::Stderr,
        payload: AgentEventPayload::RawLine(message),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_to_payload_maps_variants() {
        let p = decoded_to_payload(Decoded::ToolUse {
            call_id: "c1".into(),
            tool_name: "Bash".into(),
            input: serde_json::json!({"x": 1}),
        });
        assert!(matches!(p, AgentEventPayload::ToolUse { .. }));

        let p = decoded_to_payload(Decoded::ToolResult {
            call_id: "c1".into(),
            output: serde_json::json!("ok"),
            is_error: false,
        });
        assert!(matches!(p, AgentEventPayload::ToolResult { .. }));
    }

    #[test]
    fn control_handle_send_after_close_errors() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ControlCmd>();
        let h = ControlHandle { tx };
        drop(rx);
        assert!(matches!(h.interrupt(), Err(ClaudeSessionError::Closed)));
    }

    // Live end-to-end smoke test: drives a real `claude` session, streams a
    // turn, and interrupts. Ignored by default (needs the CLI + credentials);
    // run with `cargo test --features claude-control -- --ignored live_`.
    #[test]
    #[ignore = "requires a real `claude` CLI on PATH and credentials"]
    fn live_session_streams_and_interrupts() {
        let session =
            open_claude_session("Say hello in one word.", ClaudeSessionOptions::default()).unwrap();
        let mut saw_text = false;
        while let Ok(ev) = session.events.recv() {
            if let AgentEventPayload::StreamMessage(m) = &ev.payload {
                if !m.text.trim().is_empty() {
                    saw_text = true;
                    let _ = session.control.interrupt();
                }
            }
        }
        assert!(
            saw_text,
            "expected at least one text frame from a live session"
        );
    }

    #[test]
    fn build_options_carries_fields() {
        let opts = build_options(&ClaudeSessionOptions {
            model: Some("claude-x".into()),
            cwd: Some("/tmp".into()),
            resume: Some("sess-1".into()),
            permission_mode: None,
        });
        assert_eq!(opts.model.as_deref(), Some("claude-x"));
        assert_eq!(opts.resume.as_deref(), Some("sess-1"));
    }
}
