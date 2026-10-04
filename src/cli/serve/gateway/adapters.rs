use aikit_sdk::runner::{approval::*, session::*, LiveSession};
use aikit_sdk::{
    AgentEvent, AgentEventPayload, AgentEventStream, MessageKind, MessagePhase, MessageRole,
    StreamMessage, TerminalOutcome,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

pub type Emit = Arc<dyn Fn(SessionEventKind) + Send + Sync>;
pub type Ask = Arc<dyn Fn(&str, Value) -> RequestResponse + Send + Sync>;
pub trait Driver: Send {
    fn execute(&mut self, action: &SessionAction) -> anyhow::Result<Option<Value>>;
}

pub fn capabilities(backend: SessionBackend) -> SessionCapabilities {
    let mut c = SessionCapabilities {
        send_turn: true,
        interrupt: true,
        resume: true,
        ..Default::default()
    };
    match backend {
        SessionBackend::Claude => {
            c.permissions = true;
            c.set_model = true;
            c.context_usage = true;
        }
        SessionBackend::Codex => {
            c.permissions = true;
            c.questions = true;
            c.steer = true;
        }
        SessionBackend::Pi => {
            c.steer = true;
            c.follow_up = true;
            c.set_model = true;
            c.context_usage = true;
        }
        SessionBackend::OpenCode => {
            c.permissions = true;
            c.questions = true;
        }
        SessionBackend::Aikit => {
            c.interrupt = false;
        }
        _ => {
            c.permissions = true;
            c.resume = false;
        } // ACP capabilities updated after negotiation.
    }
    c
}

pub fn emit_agent(emit: &Emit, backend: SessionBackend, payload: AgentEventPayload) {
    emit(SessionEventKind::Agent(AgentEvent {
        agent_key: backend.key().into(),
        seq: 0,
        stream: AgentEventStream::Stdout,
        payload,
    }));
}
pub fn text(emit: &Emit, backend: SessionBackend, value: &str, reasoning: bool) {
    emit_agent(
        emit,
        backend,
        AgentEventPayload::StreamMessage(StreamMessage {
            text: value.into(),
            phase: MessagePhase::Delta,
            role: MessageRole::Assistant,
            kind: if reasoning {
                MessageKind::Reasoning
            } else {
                MessageKind::Message
            },
            source: AgentEventStream::Stdout,
            raw_line_seq: 0,
            turn_id: None,
        }),
    );
}
pub fn terminal(emit: &Emit, backend: SessionBackend, result: anyhow::Result<Value>) {
    let (outcome, reason, message) = match result {
        Ok(v) => (TerminalOutcome::Success, Some(v.to_string()), None),
        Err(e) => (TerminalOutcome::Error, None, Some(e.to_string())),
    };
    emit_agent(
        emit,
        backend,
        AgentEventPayload::Terminal {
            outcome,
            reason,
            message,
            cost_usd: None,
        },
    );
}

pub fn open(
    req: &CreateSession,
    emit: Emit,
    ask: Ask,
) -> anyhow::Result<(Box<dyn Driver>, SessionCapabilities)> {
    let caps = capabilities(req.backend);
    match req.backend {
        SessionBackend::Cursor
        | SessionBackend::Grok
        | SessionBackend::Antigravity
        | SessionBackend::Gemini => super::acp::open(req, emit, ask),
        SessionBackend::OpenCode => super::opencode::open(req, emit, ask),
        SessionBackend::Aikit => {
            anyhow::ensure!(req.permission_policy==PermissionPolicy::Allow,"aikit requires explicit allow policy; interactive permission routing is unsupported");
            let mut driver = InProcess {
                request: req.clone(),
                emit,
                active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                native: Arc::new(Mutex::new(req.resume.clone())),
            };
            driver.start(req.prompt.clone())?;
            Ok((Box::new(driver), caps))
        }
        _ => {
            let permission: PermissionCallback = Arc::new({
                let ask = ask.clone();
                move |r| match ask(
                    "permission",
                    json!({"tool":r.tool_name,"input":r.input,"tool_use_id":r.tool_use_id}),
                ) {
                    RequestResponse::Allow { .. } => ToolDecision::Allow,
                    _ => ToolDecision::Deny {
                        message: "Denied by host policy or client".into(),
                    },
                }
            });
            let (control, events): (Box<dyn LiveSession>, _) = match req.backend {
                SessionBackend::Claude => {
                    let options = aikit_sdk::runner::ClaudeSessionOptions {
                        cwd: Some(req.cwd.clone()),
                        model: req.model.clone(),
                        resume: req.resume.clone(),
                        on_tool_permission: Some(permission),
                        ..Default::default()
                    };
                    let s = aikit_sdk::runner::open_claude_session(&req.prompt, options)?;
                    let (c, e) = s.into_parts();
                    (Box::new(c), e)
                }
                SessionBackend::Codex => {
                    let options = aikit_sdk::runner::CodexSessionOptions {
                        cwd: req.cwd.clone(),
                        model: req.model.clone(),
                        resume: req.resume.clone(),
                        approval_policy: "on-request".into(),
                        on_request: Some(Arc::new(move |method, input| {
                            codex_response(
                                &method,
                                &input,
                                ask(
                                    if method.contains("UserInput")
                                        || method.contains("elicitation")
                                    {
                                        "question"
                                    } else {
                                        "permission"
                                    },
                                    json!({"method":method,"input":input}),
                                ),
                            )
                        })),
                        ..Default::default()
                    };
                    let s = aikit_sdk::runner::open_codex_session(&req.prompt, options)?;
                    let (c, e) = s.into_parts();
                    (Box::new(c), e)
                }
                SessionBackend::Pi => {
                    anyhow::ensure!(req.permission_policy==PermissionPolicy::Allow,"pi requires explicit allow policy; interactive permission routing is unsupported");
                    let options = aikit_sdk::runner::PiSessionOptions {
                        cwd: Some(req.cwd.clone()),
                        model: req.model.clone(),
                        session_id: req.resume.clone(),
                    };
                    let s = aikit_sdk::runner::open_pi_session(&req.prompt, options)?;
                    let (c, e) = s.into_parts();
                    (Box::new(c), e)
                }
                _ => unreachable!(),
            };
            std::thread::spawn(move || {
                while let Ok(event) = events.recv() {
                    emit(SessionEventKind::Agent(event));
                }
                emit(SessionEventKind::State(SessionStatus::Closed));
            });
            Ok((Box::new(Native(control)), caps))
        }
    }
}

pub fn codex_response(method: &str, input: &Value, response: RequestResponse) -> Value {
    match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            json!({"decision":if matches!(response,RequestResponse::Allow{..}){"accept"}else{"decline"}})
        }
        "item/permissions/requestApproval" => {
            json!({"permissions":if matches!(response,RequestResponse::Allow{..}) {input.get("permissions").cloned().unwrap_or(json!({}))}else{json!({})},"scope":"turn"})
        }
        "item/tool/requestUserInput" => {
            json!({"answers":match response {RequestResponse::Answers{answers}=>answers,_=>json!({})}})
        }
        "mcpServer/elicitation/request" => match response {
            RequestResponse::Answers { answers } => json!({"action":"accept","content":answers}),
            _ => json!({"action":"decline"}),
        },
        _ => json!({"decision":"denied"}),
    }
}
struct Native(Box<dyn LiveSession>);
impl Driver for Native {
    fn execute(&mut self, a: &SessionAction) -> anyhow::Result<Option<Value>> {
        match a {
            SessionAction::SendTurn { text } => self.0.send_turn(text.clone())?,
            SessionAction::Interrupt => self.0.interrupt()?,
            SessionAction::Close => self.0.disconnect()?,
            SessionAction::SetModel { model } => self.0.set_model(Some(model.clone()))?,
            SessionAction::ContextUsage => return Ok(Some(self.0.get_context_usage()?)),
            SessionAction::Steer { text } => self.0.steer(text.clone())?,
            SessionAction::FollowUp { text } => self.0.follow_up(text.clone())?,
            _ => anyhow::bail!("unsupported_operation"),
        };
        Ok(None)
    }
}
struct InProcess {
    request: CreateSession,
    emit: Emit,
    active: Arc<std::sync::atomic::AtomicBool>,
    native: Arc<Mutex<Option<String>>>,
}
impl InProcess {
    fn start(&mut self, prompt: String) -> anyhow::Result<()> {
        use std::sync::atomic::Ordering;
        anyhow::ensure!(
            !self.active.swap(true, Ordering::SeqCst),
            "turn_already_running"
        );
        let req = self.request.clone();
        let emit = self.emit.clone();
        let active = self.active.clone();
        let native = self.native.clone();
        std::thread::spawn(move || {
            let mut options = aikit_sdk::RunOptions::default();
            options.model = req.model;
            options.current_dir = Some(req.cwd);
            options.session_id = native.lock().unwrap().clone();
            options.stream = true;
            let result = aikit_sdk::runner::run_agent_events("aikit", &prompt, options, |event| {
                if let AgentEventPayload::SessionStarted { session_id } = &event.payload {
                    *native.lock().unwrap() = Some(session_id.clone());
                }
                emit(SessionEventKind::Agent(event));
            });
            active.store(false, Ordering::SeqCst);
            terminal(
                &emit,
                SessionBackend::Aikit,
                result
                    .map(|_| json!({"completed":true}))
                    .map_err(|e| anyhow::anyhow!(e.to_string())),
            );
        });
        Ok(())
    }
}
impl Driver for InProcess {
    fn execute(&mut self, a: &SessionAction) -> anyhow::Result<Option<Value>> {
        match a {
            SessionAction::SendTurn { text } => self.start(text.clone())?,
            SessionAction::Close => anyhow::ensure!(
                !self.active.load(std::sync::atomic::Ordering::SeqCst),
                "cannot_close_active_in_process_turn"
            ),
            _ => anyhow::bail!("unsupported_operation"),
        };
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approvals_preserve_method_specific_shapes() {
        assert_eq!(
            codex_response(
                "item/fileChange/requestApproval",
                &json!({}),
                RequestResponse::Deny
            ),
            json!({"decision":"decline"})
        );
        assert_eq!(
            codex_response(
                "item/permissions/requestApproval",
                &json!({"permissions":{"network":true}}),
                RequestResponse::Deny
            ),
            json!({"permissions":{},"scope":"turn"})
        );
        assert_eq!(
            codex_response(
                "item/tool/requestUserInput",
                &json!({}),
                RequestResponse::Answers {
                    answers: json!({"q":{"answers":["a"]}})
                }
            ),
            json!({"answers":{"q":{"answers":["a"]}}})
        );
    }
}
