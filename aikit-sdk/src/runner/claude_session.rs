//! Phase B2/B3: bidirectional Claude sessions over a tokio bridge.
//!
//! aikit's runner is synchronous; `claude-agent-sdk`'s client is async. This
//! module bridges them: a dedicated thread runs a current-thread tokio runtime
//! that drives the SDK's lower-level [`Query`] (the same object
//! `ClaudeSDKClient` uses internally), forwarding typed messages as canonical
//! [`AgentEvent`]s into a sync channel the caller reads, while a [`ControlHandle`]
//! sends interrupt / permission-mode / model commands back to the session.
//!
//! Concurrency: `Query::take_receiver()` yields the inbound half; a single-task
//! `tokio::select!` multiplexes that receiver and the control channel against
//! the (non-`Send`) `Query` — no second task, no `Arc<Mutex>`. Connection
//! readiness is reported back synchronously so `open_claude_session` returns
//! connect/handshake errors as a `Result`. See spec 007 (B2/B3).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use claude_agent_sdk::{
    parse_message, CanUseToolCallback, ClaudeAgentOptions, HookMatcherConfig, McpServers,
    PermissionResult, Query, QueryConfig, SdkMcpServerConfig, SubprocessCLITransport, Transport,
};

use crate::runner::backend::Decoded;
use crate::runner::backends::claude::map_message;
use crate::runner::types::{AgentEvent, AgentEventPayload, AgentEventStream};

// Re-export so callers can reach them via `claude_session::` as before.
pub use crate::runner::approval::{PermissionCallback, ToolApprovalRequest, ToolDecision};

/// The Claude permission mode, re-exported from `claude-agent-sdk`.
pub use claude_agent_sdk::PermissionMode as ClaudePermissionMode;

/// Options for opening a Claude session. Covers B2 (control) and B3 (hooks,
/// in-process MCP, fork/resume).
#[derive(Clone, Default)]
pub struct ClaudeSessionOptions {
    /// Model identifier (e.g. `claude-opus-4-5`). `None` = CLI default.
    pub model: Option<String>,
    /// Working directory for the spawned `claude` process.
    pub cwd: Option<PathBuf>,
    /// Resume an existing session by id.
    pub resume: Option<String>,
    /// Initial permission mode.
    pub permission_mode: Option<ClaudePermissionMode>,
    /// Tool-approval callback. When set, the CLI routes permission prompts to
    /// the SDK control protocol and this callback decides each one.
    pub on_tool_permission: Option<PermissionCallback>,
    /// External MCP server map forwarded to `claude --mcp-config`. Keys are
    /// server names; values are the full server config objects.
    pub mcp_servers: BTreeMap<String, serde_json::Map<String, serde_json::Value>>,
    /// When `true`, passes `--fork-session` to the CLI so the new turn forks
    /// rather than continues the resumed session.
    pub fork_session: bool,
    /// Hook matchers registered during the control-protocol initialize
    /// handshake. Keys are lifecycle event names (e.g. `"PreToolUse"`,
    /// `"PostToolUse"`); values are ordered lists of matchers with callbacks.
    pub hooks: HashMap<String, Vec<HookMatcherConfig>>,
    /// In-process SDK MCP servers. Each entry is built via
    /// [`claude_agent_sdk::create_sdk_mcp_server`] and registered with the CLI
    /// through the control protocol. The server name is stored in the config.
    pub sdk_mcp_servers: Vec<SdkMcpServerConfig>,
}

impl ClaudeSessionOptions {
    /// Set the tool-approval callback (builder style).
    pub fn with_tool_permission<F>(mut self, callback: F) -> Self
    where
        F: Fn(ToolApprovalRequest) -> ToolDecision + Send + Sync + 'static,
    {
        self.on_tool_permission = Some(Arc::new(callback));
        self
    }

    /// Register a hook callback for a lifecycle event (builder style).
    ///
    /// `event` is the lifecycle event name (`"PreToolUse"`, `"PostToolUse"`,
    /// etc.). Multiple calls with the same event append to its matcher list.
    pub fn with_hook(mut self, event: impl Into<String>, config: HookMatcherConfig) -> Self {
        self.hooks.entry(event.into()).or_default().push(config);
        self
    }

    /// Register an in-process SDK MCP server (builder style).
    pub fn with_sdk_mcp_server(mut self, server: SdkMcpServerConfig) -> Self {
        self.sdk_mcp_servers.push(server);
        self
    }
}

impl std::fmt::Debug for ClaudeSessionOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeSessionOptions")
            .field("model", &self.model)
            .field("cwd", &self.cwd)
            .field("resume", &self.resume)
            .field("permission_mode", &self.permission_mode)
            .field(
                "on_tool_permission",
                &self.on_tool_permission.as_ref().map(|_| "<callback>"),
            )
            .field("mcp_servers_count", &self.mcp_servers.len())
            .field("fork_session", &self.fork_session)
            .field("hooks_events", &self.hooks.keys().collect::<Vec<_>>())
            .field("sdk_mcp_servers_count", &self.sdk_mcp_servers.len())
            .finish()
    }
}

/// Errors opening or driving a Claude session.
#[derive(Debug)]
pub enum ClaudeSessionError {
    /// The tokio runtime could not be created.
    Runtime(String),
    /// Connecting / handshaking with the `claude` CLI failed.
    Connect(String),
    /// The control channel is closed (the session ended).
    Closed,
}

impl std::fmt::Display for ClaudeSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClaudeSessionError::Runtime(e) => write!(f, "session runtime error: {e}"),
            ClaudeSessionError::Connect(e) => write!(f, "session connect error: {e}"),
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
    /// Send a follow-up user message (multi-turn).
    SendTurn(String),
    /// Request context-window usage stats; reply is sent on the oneshot.
    GetContextUsage(tokio::sync::oneshot::Sender<Result<serde_json::Value, ClaudeSessionError>>),
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

    /// Send a follow-up user message on the same session (multi-turn).
    pub fn send_turn(&self, text: impl Into<String>) -> Result<(), ClaudeSessionError> {
        self.send(ControlCmd::SendTurn(text.into()))
    }

    /// Request context-window usage statistics from the running Claude process.
    ///
    /// Blocks until the CLI responds (or the session closes). Returns the raw
    /// JSON payload from the control-protocol response.
    pub fn get_context_usage(&self) -> Result<serde_json::Value, ClaudeSessionError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.send(ControlCmd::GetContextUsage(tx))?;
        rx.blocking_recv()
            .unwrap_or(Err(ClaudeSessionError::Closed))
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

impl ClaudeSession {
    /// Dissolve the session into its control handle and event receiver.
    ///
    /// The bridge thread is detached rather than joined. It exits naturally
    /// when the returned [`ControlHandle`] is eventually dropped (control
    /// channel closes → bridge loop breaks → `query.close()` is called).
    pub fn into_parts(self) -> (ControlHandle, mpsc::Receiver<AgentEvent>) {
        // Prevent Drop from running (which would disconnect + join synchronously).
        let this = std::mem::ManuallyDrop::new(self);
        // SAFETY: ManuallyDrop prevents the destructor from running; we read
        // every field exactly once and explicitly handle the JoinHandle.
        let control = unsafe { std::ptr::read(&this.control) };
        let events = unsafe { std::ptr::read(&this.events) };
        let join = unsafe { std::ptr::read(&this.join) };
        if let Some(j) = join {
            drop(j); // detach — thread exits when control sender is dropped
        }
        (control, events)
    }
}

/// Open a bidirectional Claude session, sending `prompt` as the first turn.
///
/// Blocks until the session is connected and the first turn has been sent, so
/// connection/handshake failures are returned as [`ClaudeSessionError::Connect`]
/// rather than surfacing only as a closed event stream.
pub fn open_claude_session(
    prompt: impl Into<String>,
    options: ClaudeSessionOptions,
) -> Result<ClaudeSession, ClaudeSessionError> {
    let prompt = prompt.into();
    let (event_tx, event_rx) = mpsc::channel::<AgentEvent>();
    let (control_tx, control_rx) = tokio::sync::mpsc::unbounded_channel::<ControlCmd>();
    // Readiness handshake: the bridge reports Ok once connected + first turn
    // sent, or Err(msg) if setup failed.
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

    let join = thread::Builder::new()
        .name("aikit-claude-session".into())
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
        .map_err(|e| ClaudeSessionError::Runtime(e.to_string()))?;

    match ready_rx.recv() {
        Ok(Ok(())) => Ok(ClaudeSession {
            control: ControlHandle { tx: control_tx },
            events: event_rx,
            join: Some(join),
        }),
        Ok(Err(msg)) => {
            let _ = join.join();
            Err(ClaudeSessionError::Connect(msg))
        }
        Err(_) => {
            let _ = join.join();
            Err(ClaudeSessionError::Connect(
                "session thread terminated before ready".to_string(),
            ))
        }
    }
}

fn build_options(options: &ClaudeSessionOptions) -> ClaudeAgentOptions {
    ClaudeAgentOptions {
        model: options.model.clone(),
        cwd: options.cwd.clone(),
        resume: options.resume.clone(),
        permission_mode: options.permission_mode.clone(),
        // Mirror ClaudeSDKClient::connect: routing permission prompts through the
        // control protocol requires the "stdio" prompt tool.
        permission_prompt_tool_name: options
            .on_tool_permission
            .as_ref()
            .map(|_| "stdio".to_string()),
        mcp_servers: if options.mcp_servers.is_empty() {
            McpServers::None
        } else {
            McpServers::Map(options.mcp_servers.clone())
        },
        fork_session: options.fork_session,
        ..ClaudeAgentOptions::default()
    }
}

fn build_query_config(options: &ClaudeSessionOptions) -> QueryConfig {
    let mut config = QueryConfig {
        can_use_tool: options
            .on_tool_permission
            .clone()
            .map(wrap_permission_callback),
        hooks: options.hooks.clone(),
        ..QueryConfig::default()
    };
    for server in &options.sdk_mcp_servers {
        let name = server
            .config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("sdk-mcp")
            .to_string();
        config = config.with_sdk_mcp_server(name, server.clone());
    }
    config
}

/// Adapt aikit's synchronous [`PermissionCallback`] into the SDK's async
/// [`CanUseToolCallback`].
fn wrap_permission_callback(cb: PermissionCallback) -> CanUseToolCallback {
    Arc::new(move |tool_name, input, ctx| {
        let cb = cb.clone();
        Box::pin(async move {
            let req = ToolApprovalRequest {
                tool_name,
                input,
                tool_use_id: ctx.tool_use_id,
            };
            match cb(req) {
                ToolDecision::Allow => PermissionResult::Allow {
                    updated_input: None,
                    updated_permissions: None,
                },
                ToolDecision::AllowWith { input } => PermissionResult::Allow {
                    updated_input: Some(input),
                    updated_permissions: None,
                },
                ToolDecision::Deny { message } => PermissionResult::Deny {
                    message,
                    interrupt: false,
                },
            }
        })
    })
}

async fn run_session(
    prompt: String,
    options: ClaudeSessionOptions,
    event_tx: mpsc::Sender<AgentEvent>,
    mut control_rx: tokio::sync::mpsc::UnboundedReceiver<ControlCmd>,
    ready_tx: mpsc::Sender<Result<(), String>>,
) {
    let opts = build_options(&options);
    let config = build_query_config(&options);

    let mut transport = SubprocessCLITransport::new(opts);
    if let Err(e) = transport.connect().await {
        let _ = ready_tx.send(Err(format!("connect failed: {e}")));
        return;
    }

    let mut query = match Query::new(Box::new(transport), config) {
        Ok(q) => q,
        Err(e) => {
            let _ = ready_tx.send(Err(format!("query init failed: {e}")));
            return;
        }
    };
    query.start();
    if let Err(e) = query.initialize().await {
        let _ = ready_tx.send(Err(format!("initialize failed: {e}")));
        return;
    }
    if let Err(e) = query.write_user_message(&prompt).await {
        let _ = ready_tx.send(Err(format!("send prompt failed: {e}")));
        return;
    }
    let mut rx = match query.take_receiver() {
        Some(rx) => rx,
        None => {
            let _ = ready_tx.send(Err("message receiver unavailable".to_string()));
            return;
        }
    };

    // Connected and first turn sent — unblock the caller.
    let _ = ready_tx.send(Ok(()));

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
                        // Emit SessionStarted on the first result message that
                        // carries a session_id so callers can observe it and
                        // use it for a subsequent `resume`.
                        if ty == "result" {
                            if let Some(sid) =
                                value.get("session_id").and_then(|v| v.as_str())
                            {
                                let _ = event_tx.send(AgentEvent {
                                    agent_key: "claude".to_string(),
                                    seq,
                                    stream: AgentEventStream::Stdout,
                                    payload: AgentEventPayload::SessionStarted {
                                        session_id: sid.to_string(),
                                    },
                                });
                                seq += 1;
                            }
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
                    Some(ControlCmd::SendTurn(text)) => {
                        if let Err(e) = query.write_user_message(&text).await {
                            emit_error(&event_tx, format!("send_turn error: {e}"));
                        }
                    }
                    Some(ControlCmd::GetContextUsage(reply_tx)) => {
                        let result = query
                            .get_context_usage()
                            .await
                            .map_err(|e| ClaudeSessionError::Connect(e.to_string()));
                        let _ = reply_tx.send(result);
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

    #[test]
    fn build_options_carries_fields_and_permission_tool() {
        let plain = build_options(&ClaudeSessionOptions {
            model: Some("claude-x".into()),
            cwd: Some("/tmp".into()),
            resume: Some("sess-1".into()),
            permission_mode: None,
            on_tool_permission: None,
            ..ClaudeSessionOptions::default()
        });
        assert_eq!(plain.model.as_deref(), Some("claude-x"));
        assert_eq!(plain.resume.as_deref(), Some("sess-1"));
        // No callback → no permission prompt tool routing.
        assert_eq!(plain.permission_prompt_tool_name, None);

        let with_cb =
            ClaudeSessionOptions::default().with_tool_permission(|_req| ToolDecision::Allow);
        let opts = build_options(&with_cb);
        // Callback present → CLI routes permission prompts via "stdio".
        assert_eq!(opts.permission_prompt_tool_name.as_deref(), Some("stdio"));
        assert!(build_query_config(&with_cb).can_use_tool.is_some());
    }

    #[test]
    fn permission_callback_maps_decisions() {
        let cb: PermissionCallback = Arc::new(|req: ToolApprovalRequest| {
            if req.tool_name == "Bash" {
                ToolDecision::Deny {
                    message: "no shell".into(),
                }
            } else if req.tool_name == "Edit" {
                ToolDecision::AllowWith {
                    input: serde_json::json!({"sanitized": true}),
                }
            } else {
                ToolDecision::Allow
            }
        });
        let wrapped = wrap_permission_callback(cb);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let ctx = claude_agent_sdk::ToolPermissionContext::default();

        let deny = rt.block_on(wrapped("Bash".into(), serde_json::json!({}), ctx.clone()));
        assert!(matches!(
            deny,
            PermissionResult::Deny {
                interrupt: false,
                ..
            }
        ));

        let allow_with = rt.block_on(wrapped("Edit".into(), serde_json::json!({}), ctx.clone()));
        match allow_with {
            PermissionResult::Allow { updated_input, .. } => {
                assert_eq!(updated_input.unwrap()["sanitized"], true);
            }
            _ => panic!("expected Allow with updated input"),
        }

        let allow = rt.block_on(wrapped("Read".into(), serde_json::json!({}), ctx));
        assert!(matches!(
            allow,
            PermissionResult::Allow {
                updated_input: None,
                ..
            }
        ));
    }

    #[test]
    fn connect_error_is_synchronous() {
        // A nonexistent working directory makes the subprocess spawn fail, so
        // connect fails — and `open_claude_session` must surface that
        // synchronously as `Connect`, not as a silently-closed stream.
        let opts = ClaudeSessionOptions {
            cwd: Some("/nonexistent/aikit/session/path".into()),
            ..Default::default()
        };
        match open_claude_session("hi", opts) {
            Err(ClaudeSessionError::Connect(_)) => {}
            Err(e) => panic!("expected synchronous Connect error, got {e:?}"),
            Ok(_) => panic!("expected synchronous Connect error, got Ok(session)"),
        }
    }

    // Live end-to-end smoke test: drives a real `claude` session, streams a
    // turn, and interrupts. Ignored by default (needs the CLI + credentials);
    // run with `cargo test --features claude-control -- --ignored live_`.
    #[test]
    #[ignore = "requires a real `claude` CLI on PATH and credentials"]
    fn live_session_streams_and_interrupts() {
        use std::time::{Duration, Instant};

        let session = open_claude_session(
            "Say hello in exactly one word.",
            ClaudeSessionOptions::default(),
        )
        .expect("connect to live claude session");

        let deadline = Instant::now() + Duration::from_secs(60);
        let mut saw_text = false;
        let mut interrupted = false;
        while Instant::now() < deadline {
            match session.events.recv_timeout(Duration::from_secs(5)) {
                Ok(ev) => {
                    eprintln!("LIVE EVENT seq={} {:?}", ev.seq, ev.payload);
                    if let AgentEventPayload::StreamMessage(m) = &ev.payload {
                        if !m.text.trim().is_empty() {
                            saw_text = true;
                            if !interrupted {
                                let _ = session.control.interrupt();
                                interrupted = true;
                            }
                        }
                    }
                }
                // Quiet after we've seen output → end the probe.
                Err(mpsc::RecvTimeoutError::Timeout) if saw_text => break,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        assert!(
            saw_text,
            "expected at least one text frame from a live session"
        );
    }

    // ── B3 unit tests ─────────────────────────────────────────────────────────

    #[test]
    fn options_default_hooks_and_sdk_mcp_empty() {
        let opts = ClaudeSessionOptions::default();
        assert!(opts.hooks.is_empty());
        assert!(opts.sdk_mcp_servers.is_empty());
    }

    #[test]
    fn with_hook_builder_accumulates() {
        use claude_agent_sdk::HookCallback;
        let cb: HookCallback =
            Arc::new(|_input, _id| Box::pin(async { serde_json::json!({"decision": "approve"}) }));
        let matcher = HookMatcherConfig {
            matcher: Some("Bash".to_string()),
            hooks: vec![cb],
            timeout: None,
        };
        let opts = ClaudeSessionOptions::default()
            .with_hook("PreToolUse", matcher.clone())
            .with_hook("PreToolUse", matcher.clone());
        assert_eq!(opts.hooks.get("PreToolUse").map(|v| v.len()), Some(2));
    }

    #[test]
    fn with_sdk_mcp_server_builder_accumulates() {
        use claude_agent_sdk::{create_sdk_mcp_server, tool};
        let t = tool("ping", "Pong", serde_json::json!({}), |_| async move {
            serde_json::json!("pong")
        });
        let srv = create_sdk_mcp_server("test-server", "1.0.0", vec![t]);
        let opts = ClaudeSessionOptions::default().with_sdk_mcp_server(srv);
        assert_eq!(opts.sdk_mcp_servers.len(), 1);
        assert_eq!(
            opts.sdk_mcp_servers[0]
                .config
                .get("name")
                .and_then(|v| v.as_str()),
            Some("test-server")
        );
    }

    #[test]
    fn get_context_usage_queues_command_and_returns_on_reply() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ControlCmd>();
        let handle = ControlHandle { tx };

        let reply_val = serde_json::json!({"inputTokens": 500, "outputTokens": 200});
        let rv = reply_val.clone();

        // Simulate the bridge answering in a background thread.
        std::thread::spawn(move || {
            if let Some(ControlCmd::GetContextUsage(reply_tx)) = rx.blocking_recv() {
                let _ = reply_tx.send(Ok(rv));
            }
        });

        let result = handle.get_context_usage().unwrap();
        assert_eq!(result["inputTokens"], 500);
    }

    #[test]
    fn debug_shows_hook_events_and_sdk_mcp_count() {
        let opts = ClaudeSessionOptions::default();
        let dbg = format!("{opts:?}");
        assert!(dbg.contains("hooks_events"));
        assert!(dbg.contains("sdk_mcp_servers_count"));
    }
}
