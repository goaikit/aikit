use super::{adapters::*, rpc::Rpc};
use aikit_sdk::{runner::session::*, AgentEventPayload};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

pub fn launch(backend: SessionBackend) -> anyhow::Result<(String, Vec<String>)> {
    let prefix = format!("AIKIT_{}", backend.key().to_uppercase());
    let (default, args) = match backend {
        SessionBackend::Cursor => ("cursor-agent", vec!["acp"]),
        SessionBackend::Grok => (
            "grok",
            vec!["--permission-mode", "default", "agent", "stdio"],
        ),
        SessionBackend::Gemini => ("gemini", vec!["--acp"]),
        SessionBackend::Antigravity => (
            "",
            if cfg!(target_os = "linux") {
                vec!["--uid="]
            } else {
                vec![]
            },
        ),
        _ => anyhow::bail!("not an ACP backend"),
    };
    let program = std::env::var(format!("{prefix}_BIN")).unwrap_or(default.into());
    anyhow::ensure!(
        !program.is_empty(),
        "AIKIT_ANTIGRAVITY_BIN must point to the installed ACP harness executable"
    );
    let args = match std::env::var(format!("{prefix}_ARGS")) {
        Ok(value) => serde_json::from_str::<Vec<String>>(&value)?,
        Err(_) => args.into_iter().map(String::from).collect(),
    };
    Ok((program, args))
}
pub fn permission_reply(input: &Value, response: RequestResponse) -> Value {
    let chosen = match response {
        RequestResponse::Allow { option_id } => input["options"]
            .as_array()
            .and_then(|opts| {
                opts.iter().find(|o| {
                    o["kind"] == "allow_once"
                        && option_id
                            .as_ref()
                            .map_or(true, |id| o["optionId"].as_str() == Some(id))
                })
            })
            .and_then(|o| o["optionId"].as_str()),
        _ => None,
    };
    match chosen {
        Some(id) => json!({"outcome":{"outcome":"selected","optionId":id}}),
        None => json!({"outcome":{"outcome":"cancelled"}}),
    }
}
pub fn open(
    req: &CreateSession,
    emit: Emit,
    ask: Ask,
) -> anyhow::Result<(Box<dyn Driver>, SessionCapabilities)> {
    let (program, args) = launch(req.backend)?;
    open_with_launch(req, emit, ask, &program, &args)
}
fn open_with_launch(
    req: &CreateSession,
    emit: Emit,
    ask: Ask,
    program: &str,
    args: &[String],
) -> anyhow::Result<(Box<dyn Driver>, SessionCapabilities)> {
    let backend = req.backend;
    let emit_notify = emit.clone();
    let tools = Arc::new(Mutex::new(HashMap::<String, Value>::new()));
    let rpc = Rpc::spawn(
        program,
        args,
        &req.cwd,
        Arc::new(move |frame| {
            if frame["method"] == "aikit/transportClosed" {
                emit_notify(SessionEventKind::State(SessionStatus::Closed));
                return;
            }
            if frame["method"] == "session/update" {
                map_update(&emit_notify, backend, &frame["params"]["update"], &tools);
            }
            emit_notify(SessionEventKind::Native {
                protocol: "acp".into(),
                value: frame,
            });
        }),
        Arc::new(move |method, input| match method {
            "session/request_permission" => {
                Ok(permission_reply(&input, ask("permission", input.clone())))
            }
            _ => Err(format!("Unsupported client operation: {method}")),
        }),
    )?;
    let result = (|| {
        let init=rpc.request("initialize",json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"aikit","version":env!("CARGO_PKG_VERSION")}}),Duration::from_secs(30))?;
        anyhow::ensure!(
            init["protocolVersion"].as_u64() == Some(1),
            "unsupported ACP protocol version"
        );
        if let Ok(method) = std::env::var(format!(
            "AIKIT_{}_AUTH_METHOD",
            backend.key().to_uppercase()
        )) {
            rpc.request(
                "authenticate",
                json!({"methodId":method}),
                Duration::from_secs(60),
            )?;
        }
        let load = init
            .pointer("/agentCapabilities/loadSession")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let resume = init
            .pointer("/agentCapabilities/sessionCapabilities/resume")
            .is_some();
        let session = if let Some(id) = &req.resume {
            anyhow::ensure!(load || resume, "resume unsupported by this ACP agent");
            rpc.request(
                if load {
                    "session/load"
                } else {
                    "session/resume"
                },
                json!({"sessionId":id,"cwd":req.cwd,"mcpServers":[]}),
                Duration::from_secs(60),
            )?;
            id.clone()
        } else {
            rpc.request(
                "session/new",
                json!({"cwd":req.cwd,"mcpServers":[]}),
                Duration::from_secs(60),
            )?["sessionId"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing ACP session id"))?
                .into()
        };
        let mut caps = capabilities(backend);
        caps.resume = load || resume;
        // Model selection is optional in ACP; use only an explicitly supplied model.
        if let Some(model) = &req.model {
            rpc.request(
                "session/set_model",
                json!({"sessionId":session,"modelId":model}),
                Duration::from_secs(30),
            )?;
        }
        emit_agent(
            &emit,
            backend,
            AgentEventPayload::SessionStarted {
                session_id: session.clone(),
            },
        );
        let mut driver = Acp {
            rpc: rpc.clone(),
            session,
            emit,
            backend,
            active: Arc::new(AtomicBool::new(false)),
        };
        driver.prompt(req.prompt.clone())?;
        Ok((Box::new(driver) as Box<dyn Driver>, caps))
    })();
    if result.is_err() {
        rpc.close();
    }
    result
}
struct Acp {
    rpc: Rpc,
    session: String,
    emit: Emit,
    backend: SessionBackend,
    active: Arc<AtomicBool>,
}
impl Acp {
    fn prompt(&mut self, text: String) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.active.swap(true, Ordering::SeqCst),
            "turn_already_running"
        );
        let rpc = self.rpc.clone();
        let sid = self.session.clone();
        let emit = self.emit.clone();
        let backend = self.backend;
        let active = self.active.clone();
        std::thread::spawn(move || {
            let result = rpc.request(
                "session/prompt",
                json!({"sessionId":sid,"prompt":[{"type":"text","text":text}]}),
                Duration::from_secs(3600),
            );
            active.store(false, Ordering::SeqCst);
            terminal(&emit, backend, result);
        });
        Ok(())
    }
}
impl Driver for Acp {
    fn execute(&mut self, a: &SessionAction) -> anyhow::Result<Option<Value>> {
        match a {
            SessionAction::SendTurn { text } => self.prompt(text.clone())?,
            SessionAction::Interrupt => self
                .rpc
                .notify("session/cancel", json!({"sessionId":self.session}))?,
            SessionAction::Close => self.rpc.close(),
            _ => anyhow::bail!("unsupported_operation"),
        };
        Ok(None)
    }
}
impl Drop for Acp {
    fn drop(&mut self) {
        self.rpc.close();
    }
}
fn map_update(
    emit: &Emit,
    backend: SessionBackend,
    update: &Value,
    tools: &Mutex<HashMap<String, Value>>,
) {
    match update["sessionUpdate"].as_str().unwrap_or("") {
        "agent_message_chunk" | "agent_thought_chunk" => {
            if let Some(t) = update.pointer("/content/text").and_then(Value::as_str) {
                text(
                    emit,
                    backend,
                    t,
                    update["sessionUpdate"] == "agent_thought_chunk",
                );
            }
        }
        "tool_call" | "tool_call_update" => {
            if let Some(id) = update["toolCallId"].as_str() {
                let mut map = tools.lock().unwrap();
                if map.len() > 1024 {
                    map.clear();
                }
                let item = map.entry(id.into()).or_insert(json!({}));
                let previous = item["status"].clone();
                if let Some(patch) = update.as_object() {
                    for (k, v) in patch {
                        item[k] = v.clone();
                    }
                }
                if update["sessionUpdate"] == "tool_call" {
                    emit_agent(
                        emit,
                        backend,
                        AgentEventPayload::ToolUse {
                            call_id: id.into(),
                            tool_name: item["title"].as_str().unwrap_or("tool").into(),
                            input: item.get("rawInput").cloned().unwrap_or(Value::Null),
                        },
                    );
                }
                if (item["status"] == "completed" || item["status"] == "failed")
                    && previous != item["status"]
                {
                    emit_agent(
                        emit,
                        backend,
                        AgentEventPayload::ToolResult {
                            call_id: id.into(),
                            output: item.clone(),
                            is_error: item["status"] == "failed",
                            duration_ms: None,
                            started_at_ms: None,
                        },
                    );
                }
            }
        }
        _ => {}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn peer() -> &'static std::path::Path {
        static PEER: std::sync::OnceLock<(tempfile::TempDir, std::path::PathBuf)> =
            std::sync::OnceLock::new();
        &PEER
            .get_or_init(|| {
                let dir = tempfile::tempdir().unwrap();
                let exe = dir.path().join(if cfg!(windows) {
                    "acp-peer.exe"
                } else {
                    "acp-peer"
                });
                let status = std::process::Command::new("rustc")
                    .args(["--edition=2021", "tests/fixtures/acp_peer.rs", "-o"])
                    .arg(&exe)
                    .status()
                    .unwrap();
                assert!(status.success());
                (dir, exe)
            })
            .1
    }
    #[test]
    fn four_acp_backends_complete_native_permission_round_trip() {
        for backend in [
            SessionBackend::Cursor,
            SessionBackend::Grok,
            SessionBackend::Gemini,
            SessionBackend::Antigravity,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (events, rx) = std::sync::mpsc::channel();
            let emit: Emit = Arc::new(move |e| {
                let _ = events.send(e);
            });
            let req = CreateSession {
                command_id: "c".into(),
                backend,
                cwd: dir.path().into(),
                prompt: "read".into(),
                model: Some("model".into()),
                resume: None,
                permission_policy: PermissionPolicy::Ask,
            };
            let (mut driver, caps) = open_with_launch(
                &req,
                emit,
                Arc::new(|kind, input| {
                    assert_eq!(kind, "permission");
                    assert_eq!(input["options"][0]["optionId"], "native-once");
                    RequestResponse::Allow { option_id: None }
                }),
                peer().to_str().unwrap(),
                &[],
            )
            .unwrap();
            assert!(caps.resume);
            let mut text = false;
            loop {
                let event = rx.recv_timeout(Duration::from_secs(5)).unwrap();
                if let SessionEventKind::Agent(a) = event {
                    match a.payload {
                        AgentEventPayload::StreamMessage(m) => text |= m.text == "fixture response",
                        AgentEventPayload::Terminal { .. } => break,
                        _ => {}
                    }
                }
            }
            assert!(text);
            driver.execute(&SessionAction::Close).unwrap();
        }
    }
    #[test]
    fn cancel_is_processed_while_permission_handler_waits() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let emit: Emit = Arc::new(move |e| {
            let _ = tx.send(e);
        });
        let (asked, ask_rx) = std::sync::mpsc::sync_channel(1);
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let copy = gate.clone();
        let ask: Ask = Arc::new(move |_, _| {
            asked.send(()).unwrap();
            let (lock, wake) = &*copy;
            let mut done = lock.lock().unwrap();
            while !*done {
                done = wake.wait(done).unwrap();
            }
            RequestResponse::Deny
        });
        let req = CreateSession {
            command_id: "c".into(),
            backend: SessionBackend::Cursor,
            cwd: dir.path().into(),
            prompt: "read".into(),
            model: None,
            resume: None,
            permission_policy: PermissionPolicy::Ask,
        };
        let (mut driver, _) =
            open_with_launch(&req, emit, ask, peer().to_str().unwrap(), &[]).unwrap();
        ask_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        driver.execute(&SessionAction::Interrupt).unwrap();
        loop {
            if let SessionEventKind::Agent(a) = rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                if let AgentEventPayload::Terminal { reason, .. } = a.payload {
                    assert!(reason.unwrap().contains("cancelled"));
                    break;
                }
            }
        }
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        driver.execute(&SessionAction::Close).unwrap();
    }
    #[test]
    fn approval_uses_native_once_id() {
        let request = json!({"options":[{"kind":"allow_always","optionId":"all"},{"kind":"allow_once","optionId":"native-57"}]});
        assert_eq!(
            permission_reply(&request, RequestResponse::Allow { option_id: None })["outcome"]
                ["optionId"],
            "native-57"
        );
        assert_eq!(
            permission_reply(
                &request,
                RequestResponse::Allow {
                    option_id: Some("invented".into())
                }
            )["outcome"]["outcome"],
            "cancelled"
        );
    }
    #[test]
    fn partial_tool_updates_merge() {
        let result = Arc::new(Mutex::new(Vec::new()));
        let copy = result.clone();
        let emit: Emit = Arc::new(move |e| copy.lock().unwrap().push(e));
        let tools = Mutex::new(HashMap::new());
        map_update(
            &emit,
            SessionBackend::Cursor,
            &json!({"sessionUpdate":"tool_call","toolCallId":"t","title":"Read","rawInput":{"path":"a"},"status":"pending"}),
            &tools,
        );
        map_update(
            &emit,
            SessionBackend::Cursor,
            &json!({"sessionUpdate":"tool_call_update","toolCallId":"t","status":"completed"}),
            &tools,
        );
        assert_eq!(tools.lock().unwrap()["t"]["title"], "Read");
        assert_eq!(result.lock().unwrap().len(), 2);
    }
}
