use crate::error::{CodexError, Result};
use crate::events::{ServerMessage, ServerNotification, ServerRequest};
use crate::protocol::RequestId;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex};
use tokio::time::timeout;

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<std::result::Result<Value, CodexError>>>>>;

/// Options for spawning `codex app-server`.
#[derive(Debug, Clone)]
pub struct SpawnOptions {
    /// Binary name or path used to locate `codex`. Defaults to `"codex"`.
    pub codex_bin: String,

    /// Extra args appended after `app-server` (e.g. `["--listen", "stdio://"]`).
    pub extra_args: Vec<String>,

    /// Working directory for the spawned process. Defaults to inherited.
    pub cwd: Option<std::path::PathBuf>,

    /// Per-request timeout applied by `request()`. Defaults to 60s.
    pub default_request_timeout: Duration,
}

impl Default for SpawnOptions {
    fn default() -> Self {
        Self {
            codex_bin: "codex".to_string(),
            extra_args: Vec::new(),
            cwd: None,
            default_request_timeout: Duration::from_secs(60),
        }
    }
}

/// Async client for the Codex `app-server` JSON-RPC protocol over stdio.
///
/// Wire format: newline-delimited JSON-RPC 2.0 with the `"jsonrpc":"2.0"`
/// header omitted (per the app-server spec). The client spawns the server as
/// a child process, owns its stdin/stdout, routes request/response pairs by
/// id, and forwards notifications and server-initiated requests to a channel.
pub struct CodexClient {
    next_id: AtomicU64,
    pending: PendingMap,
    stdin: AsyncMutex<ChildStdin>,
    child: AsyncMutex<Option<Child>>,
    initialized: AtomicBool,
    default_request_timeout: Duration,
}

impl CodexClient {
    /// Spawn `codex app-server` with default options.
    ///
    /// Returns the client plus a receiver of inbound server messages
    /// (notifications and server-initiated requests).
    pub async fn spawn() -> Result<(Self, mpsc::Receiver<ServerMessage>)> {
        Self::spawn_with(SpawnOptions::default()).await
    }

    /// Spawn `codex app-server` with custom options.
    pub async fn spawn_with(opts: SpawnOptions) -> Result<(Self, mpsc::Receiver<ServerMessage>)> {
        let default_request_timeout = opts.default_request_timeout;

        let mut cmd = Command::new(&opts.codex_bin);
        cmd.arg("app-server");
        for arg in &opts.extra_args {
            cmd.arg(arg);
        }
        if let Some(cwd) = &opts.cwd {
            cmd.current_dir(cwd);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            // Safety net: never leak a child if the caller forgets shutdown().
            .kill_on_drop(true);

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let (event_tx, event_rx) = mpsc::channel::<ServerMessage>(256);

        tokio::spawn(reader_loop(stdout, Arc::clone(&pending), event_tx));

        Ok((
            Self {
                next_id: AtomicU64::new(1),
                pending,
                stdin: AsyncMutex::new(stdin),
                child: AsyncMutex::new(Some(child)),
                initialized: AtomicBool::new(false),
                default_request_timeout,
            },
            event_rx,
        ))
    }

    /// Perform the required `initialize` + `initialized` handshake.
    ///
    /// Must be the first call on a fresh connection; subsequent requests are
    /// rejected by the server until this completes.
    pub async fn initialize(
        &self,
        client_name: &str,
        client_title: &str,
        version: &str,
    ) -> Result<Value> {
        if self.initialized.swap(true, Ordering::SeqCst) {
            return Err(CodexError::AlreadyInitialized);
        }
        let result = self
            .request(
                "initialize",
                json!({
                    "clientInfo": {
                        "name": client_name,
                        "title": client_title,
                        "version": version,
                    }
                }),
            )
            .await?;
        self.notify("initialized", json!({})).await?;
        Ok(result)
    }

    /// Create a new thread (conversation). Returns the full response object.
    ///
    /// Callers build the params object per the app-server spec; common fields
    /// are `cwd`, `approvalPolicy`, `sandbox`, `model`, etc.
    pub async fn thread_start(&self, params: Value) -> Result<Value> {
        self.request("thread/start", params).await
    }

    /// Convenience: start a thread with `cwd`, approval policy, and sandbox
    /// shorthand. Returns the new [`ThreadId`].
    pub async fn thread_start_simple(
        &self,
        cwd: impl AsRef<std::path::Path>,
        approval_policy: &str,
        sandbox: &str,
    ) -> Result<crate::ThreadId> {
        let params = thread_start_simple_params(cwd.as_ref(), approval_policy, sandbox)?;
        let res = self.thread_start(params).await?;
        let id = thread_id_from_response(&res, "thread/start")?;
        Ok(crate::ThreadId(id))
    }

    /// Resume an existing stored thread by id.
    pub async fn thread_resume(&self, thread_id: &crate::ThreadId) -> Result<Value> {
        self.request("thread/resume", json!({ "threadId": thread_id.0 }))
            .await
    }

    /// Send a text turn and return the new [`TurnId`].
    ///
    /// Streaming output for the turn arrives as notifications on the receiver
    /// returned from [`spawn`](Self::spawn); drain until
    /// [`ServerNotificationKind::TurnCompleted`] to collect the full response.
    pub async fn turn_start(
        &self,
        thread_id: &crate::ThreadId,
        text: &str,
    ) -> Result<crate::TurnId> {
        let res = self
            .request(
                "turn/start",
                json!({
                    "threadId": thread_id.0,
                    "input": [ { "type": "text", "text": text } ],
                }),
            )
            .await?;
        let id = turn_id_from_response(&res)?;
        Ok(crate::TurnId(id))
    }

    /// Append text to an already in-flight turn (steering).
    ///
    /// Returns the active [`TurnId`] that accepted the input.
    pub async fn turn_steer(
        &self,
        thread_id: &crate::ThreadId,
        text: &str,
    ) -> Result<crate::TurnId> {
        let res = self
            .request(
                "turn/steer",
                json!({
                    "threadId": thread_id.0,
                    "input": [ { "type": "text", "text": text } ],
                }),
            )
            .await?;
        let id = res
            .get("turnId")
            .and_then(Value::as_str)
            .ok_or_else(|| CodexError::Send("missing turnId in turn/steer response".into()))?
            .to_string();
        Ok(crate::TurnId(id))
    }

    /// Request cancellation of an in-flight turn.
    pub async fn turn_interrupt(
        &self,
        thread_id: &crate::ThreadId,
        turn_id: &crate::TurnId,
    ) -> Result<()> {
        let _ = self
            .request(
                "turn/interrupt",
                json!({ "threadId": thread_id.0, "turnId": turn_id.0 }),
            )
            .await?;
        Ok(())
    }

    /// Reply to a [`ServerRequest`] (e.g. an approval) with a `result` payload.
    pub async fn reply_server_request(&self, id: RequestId, result: Value) -> Result<()> {
        self.write_raw(&json!({ "id": id, "result": result })).await
    }

    /// Reject a [`ServerRequest`] with a JSON-RPC error.
    pub async fn reply_server_request_error(
        &self,
        id: RequestId,
        code: i64,
        message: &str,
    ) -> Result<()> {
        self.write_raw(&json!({ "id": id, "error": { "code": code, "message": message } }))
            .await
    }

    /// Send a JSON-RPC request and await its `result` using the default timeout.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.request_with_timeout(method, params, self.default_request_timeout)
            .await
    }

    /// Send a JSON-RPC request with an explicit timeout.
    ///
    /// On timeout the pending entry is removed and
    /// [`CodexError::RequestTimeout`] is returned.
    pub async fn request_with_timeout(
        &self,
        method: &str,
        params: Value,
        deadline: Duration,
    ) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);

        self.write_raw(&json!({ "method": method, "id": id, "params": params }))
            .await?;

        let method_owned = method.to_string();
        match timeout(deadline, rx).await {
            Ok(Ok(Ok(v))) => Ok(v),
            Ok(Ok(Err(e))) => match e {
                CodexError::ServerError {
                    code,
                    message,
                    data,
                    ..
                } => Err(CodexError::ServerError {
                    method: method_owned,
                    code,
                    message,
                    data,
                }),
                other => Err(other),
            },
            Ok(Err(_)) => Err(CodexError::Closed {
                method: method_owned,
            }),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(CodexError::RequestTimeout {
                    method: method_owned,
                    timeout_secs: deadline.as_secs(),
                })
            }
        }
    }

    /// Fire-and-forget JSON-RPC notification (no id, no response expected).
    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.write_raw(&json!({ "method": method, "params": params }))
            .await
    }

    async fn write_raw(&self, value: &Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(value)?;
        bytes.push(b'\n');
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(&bytes)
            .await
            .map_err(|e| CodexError::Send(e.to_string()))?;
        Ok(())
    }

    /// Close stdin (causing the server to exit cleanly) and reap the child.
    ///
    /// Force-kills after a 5-second grace period. Safe to call multiple times.
    pub async fn shutdown(&self) -> Result<()> {
        // Drop the stdin guard so the server sees EOF.
        {
            let _guard = self.stdin.lock().await;
        }
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = timeout(Duration::from_secs(5), child.wait()).await;
            let _ = child.kill().await;
        }
        Ok(())
    }
}

fn thread_id_from_response(res: &Value, method: &str) -> Result<String> {
    res.get("thread")
        .and_then(|t| t.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| CodexError::Send(format!("missing thread.id in {method} response")))
}

fn turn_id_from_response(res: &Value) -> Result<String> {
    res.get("turn")
        .and_then(|t| t.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| CodexError::Send("missing turn.id in turn/start response".into()))
}

fn thread_start_simple_params(
    cwd: &std::path::Path,
    approval_policy: &str,
    sandbox: &str,
) -> Result<Value> {
    Ok(json!({
        "cwd": cwd.to_string_lossy(),
        "approvalPolicy": approval_policy,
        "sandbox": normalize_sandbox_mode(sandbox)?,
    }))
}

fn normalize_sandbox_mode(sandbox: &str) -> Result<&'static str> {
    match sandbox {
        "read-only" => Ok("read-only"),
        "workspace-write" | "workspaceWrite" => Ok("workspace-write"),
        "danger-full-access" | "dangerFullAccess" => Ok("danger-full-access"),
        other => Err(CodexError::InvalidParameter {
            name: "sandbox".to_string(),
            message: format!(
                "unknown value '{other}', expected one of 'read-only', 'workspace-write', 'danger-full-access'"
            ),
        }),
    }
}

async fn reader_loop(
    stdout: ChildStdout,
    pending: PendingMap,
    event_tx: mpsc::Sender<ServerMessage>,
) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            tracing::debug!(line = %line, "skipping non-JSON line from server");
            continue;
        };
        let Some(obj) = v.as_object() else {
            continue;
        };

        // 1. Response to one of our requests: { id, result | error }
        if obj.contains_key("result") || obj.contains_key("error") {
            let Some(id) = obj.get("id").and_then(Value::as_u64) else {
                continue;
            };
            let Some(sender) = pending.lock().unwrap().remove(&id) else {
                continue;
            };
            if let Some(err) = obj.get("error") {
                let _ = sender.send(Err(CodexError::ServerError {
                    method: String::new(),
                    code: err.get("code").and_then(Value::as_i64).unwrap_or(-1),
                    message: err
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error")
                        .to_string(),
                    data: err.get("data").cloned(),
                }));
            } else {
                let _ = sender.send(Ok(obj.get("result").cloned().unwrap_or(Value::Null)));
            }
            continue;
        }

        // 2. Notification or server-initiated request: { method, params[, id] }
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            continue;
        };
        let method = method.to_string();
        let params = obj.get("params").cloned().unwrap_or(Value::Null);

        let message = match obj.get("id") {
            Some(id_value) => {
                let id = match id_value {
                    Value::Number(n) if n.as_u64().is_some() => {
                        RequestId::Num(n.as_u64().expect("checked above"))
                    }
                    Value::String(s) => RequestId::Str(s.clone()),
                    _ => continue,
                };
                ServerMessage::ServerRequest(ServerRequest { id, method, params })
            }
            None => ServerMessage::Notification(ServerNotification { method, params }),
        };

        if event_tx.send(message).await.is_err() {
            break; // caller dropped the receiver
        }
    }

    // Transport closed: drop all pending senders so waiting requests see Closed.
    pending.lock().unwrap().clear();
}

#[cfg(test)]
mod tests {
    use super::{normalize_sandbox_mode, thread_start_simple_params};
    use crate::CodexError;
    use std::path::Path;

    #[test]
    fn sandbox_modes_are_serialized_as_protocol_kebab_case() {
        assert_eq!(normalize_sandbox_mode("read-only").unwrap(), "read-only");
        assert_eq!(
            normalize_sandbox_mode("workspace-write").unwrap(),
            "workspace-write"
        );
        assert_eq!(
            normalize_sandbox_mode("danger-full-access").unwrap(),
            "danger-full-access"
        );
    }

    #[test]
    fn legacy_camel_case_sandbox_modes_are_normalized() {
        assert_eq!(
            normalize_sandbox_mode("workspaceWrite").unwrap(),
            "workspace-write"
        );
        assert_eq!(
            normalize_sandbox_mode("dangerFullAccess").unwrap(),
            "danger-full-access"
        );
    }

    #[test]
    fn thread_start_simple_params_normalizes_workspace_write() {
        let params =
            thread_start_simple_params(Path::new("/tmp/repo"), "never", "workspaceWrite").unwrap();

        assert_eq!(params["cwd"], "/tmp/repo");
        assert_eq!(params["approvalPolicy"], "never");
        assert_eq!(params["sandbox"], "workspace-write");
    }

    #[test]
    fn unknown_sandbox_mode_is_rejected_locally() {
        let err = normalize_sandbox_mode("workspace_write").unwrap_err();

        assert!(matches!(
            err,
            CodexError::InvalidParameter { ref name, .. } if name == "sandbox"
        ));
        assert!(err
            .to_string()
            .contains("expected one of 'read-only', 'workspace-write', 'danger-full-access'"));
    }
}
