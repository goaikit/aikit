use super::adapters::*;
use aikit_sdk::{runner::session::*, AgentEventPayload};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{
    io::Read,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

pub fn open(
    req: &CreateSession,
    emit: Emit,
    ask: Ask,
) -> anyhow::Result<(Box<dyn Driver>, SessionCapabilities)> {
    let base = std::env::var("AIKIT_OPENCODE_URL")
        .map_err(|_| {
            anyhow::anyhow!("Configure AIKIT_OPENCODE_URL for an operator-managed OpenCode server")
        })?
        .trim_end_matches('/')
        .to_string();
    open_at(req, emit, ask, base)
}
fn open_at(
    req: &CreateSession,
    emit: Emit,
    ask: Ask,
    base: String,
) -> anyhow::Result<(Box<dyn Driver>, SessionCapabilities)> {
    let url = reqwest::Url::parse(&base)?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https"),
        "invalid OpenCode URL"
    );
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()?;
    let cwd = req.cwd.to_string_lossy().to_string();
    let mut driver = OpenCode {
        base,
        client,
        cwd,
        session: String::new(),
        model: req.model.clone(),
        active: Arc::new(AtomicBool::new(false)),
        seen_busy: Arc::new(AtomicBool::new(false)),
        closed: Arc::new(AtomicBool::new(false)),
    };
    driver.session = if let Some(id) = &req.resume {
        super::validate_id(id)?;
        driver.get(&format!("/session/{id}"))?;
        id.clone()
    } else {
        driver.post("/session", json!({}))?["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing OpenCode session id"))?
            .into()
    };
    emit_agent(
        &emit,
        SessionBackend::OpenCode,
        AgentEventPayload::SessionStarted {
            session_id: driver.session.clone(),
        },
    );
    let session = driver.session.clone();
    let active = driver.active.clone();
    let busy = driver.seen_busy.clone();
    let closed = driver.closed.clone();
    let peer = driver.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        let Ok(runtime) = runtime else {
            let _ = ready_tx.send(Err("stream runtime failed".to_string()));
            return;
        };
        runtime.block_on(async move {
            let stream_client=reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).build().unwrap();
            let mut request=stream_client.get(format!("{}/event",peer.base)).query(&[("directory",&peer.cwd)]);
            if let Ok(password)=std::env::var("OPENCODE_SERVER_PASSWORD"){request=request.basic_auth(std::env::var("OPENCODE_SERVER_USERNAME").unwrap_or("opencode".into()),Some(password));}
            let mut response=match request.send().await.and_then(|r|r.error_for_status()){Ok(r)=>r,Err(e)=>{let _=ready_tx.send(Err(e.to_string()));return;}};
            if ready_tx.send(Ok(())).is_err(){return;}
            let mut buffer=Vec::new();let mut data=String::new();let mut parts=std::collections::HashMap::<String,Value>::new();
            loop {
                if closed.load(Ordering::SeqCst){return;}
                let chunk=tokio::select!{chunk=response.chunk()=>chunk,_=tokio::time::sleep(Duration::from_millis(250))=>continue};
                let chunk=match chunk{Ok(Some(c))=>c,_=>{emit(SessionEventKind::Error{code:"opencode_stream_lost".into(),message:"Native stream ended; turn outcome may be unknown".into()});emit(SessionEventKind::State(SessionStatus::Interrupted));return;}};
                buffer.extend_from_slice(&chunk);
                if buffer.len()>1024*1024{emit(SessionEventKind::State(SessionStatus::Failed));return;}
                while let Some(end)=buffer.iter().position(|b|*b==b'\n') {
                    let line=String::from_utf8_lossy(&buffer[..end]).trim_end_matches('\r').to_string();buffer.drain(..=end);
                    if let Some(part)=line.strip_prefix("data:"){data.push_str(part.trim_start());data.push('\n');if data.len()>1024*1024{emit(SessionEventKind::State(SessionStatus::Failed));return;}continue;}
                    if !line.is_empty()||data.is_empty(){continue;}
                    let parsed=serde_json::from_str::<Value>(&data);data.clear();let Ok(event)=parsed else {continue};
            let p = &event["properties"];
            let sid = p["sessionID"]
                .as_str()
                .or_else(|| p.pointer("/part/sessionID").and_then(Value::as_str))
                .or_else(|| p.pointer("/info/sessionID").and_then(Value::as_str));
            if sid != Some(session.as_str()) {
                continue;
            }
            match event["type"].as_str().unwrap_or("") {
                "message.part.delta" | "message.part.updated" => {map_part(&emit,&event,&mut parts);}
                "message.updated" => {
                    if p.pointer("/info/role").and_then(Value::as_str) == Some("user") {
                        busy.store(true, Ordering::SeqCst);
                    }
                }
                "session.status" => {
                    let status = p
                        .pointer("/status/type")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if status == "busy" {
                        busy.store(true, Ordering::SeqCst);
                    }
                    if status == "idle"
                        && busy.load(Ordering::SeqCst)
                        && active.swap(false, Ordering::SeqCst)
                    {
                        terminal(
                            &emit,
                            SessionBackend::OpenCode,
                            Ok(json!({"status":"idle"})),
                        );
                    }
                }
                "session.error" => {
                    if active.swap(false, Ordering::SeqCst) {
                        terminal(
                            &emit,
                            SessionBackend::OpenCode,
                            Err(anyhow::anyhow!("{}", p["error"])),
                        );
                    }
                }
                "permission.asked" | "question.asked" => {
                    let kind = if event["type"] == "question.asked" {
                        "question"
                    } else {
                        "permission"
                    };
                    let ask = ask.clone();
                    let peer = peer.clone();
                    let input = p.clone();
                    let emit = emit.clone();
                    std::thread::spawn(move || {
                        let response = ask(kind, input.clone());
                        let Some(id) = input["id"].as_str() else {
                            return;
                        };
                        if super::validate_id(id).is_err(){return;}
                        let result = if kind == "permission" {
                            peer.post(&format!("/permission/{id}/reply"),json!({"reply":if matches!(response,RequestResponse::Allow{..}){"once"}else{"reject"}}))
                        } else {
                            match response {
                                RequestResponse::Answers { answers } => peer.post(
                                    &format!("/question/{id}/reply"),
                                    json!({"answers":answers}),
                                ),
                                _ => peer.post(&format!("/question/{id}/reject"), json!({})),
                            }
                        };
                        if let Err(e) = result {
                            emit(SessionEventKind::Error {
                                code: "request_reply_failed".into(),
                                message: e.to_string(),
                            });
                        }
                    });
                }
                _ => {}
            }
            emit(SessionEventKind::Native {
                protocol: "opencode".into(),
                value: event,
            });
                }
            }
        });
    });
    ready_rx
        .recv_timeout(Duration::from_secs(15))?
        .map_err(anyhow::Error::msg)?;
    if let Err(e) = driver.prompt(&req.prompt) {
        driver.closed.store(true, Ordering::SeqCst);
        return Err(e);
    }
    Ok((Box::new(driver), capabilities(SessionBackend::OpenCode)))
}
#[derive(Clone)]
struct OpenCode {
    base: String,
    client: Client,
    cwd: String,
    session: String,
    model: Option<String>,
    active: Arc<AtomicBool>,
    seen_busy: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
}
impl OpenCode {
    fn request(&self, path: &str, body: Option<Value>) -> anyhow::Result<Value> {
        let mut request = if let Some(body) = body {
            self.client.post(format!("{}{path}", self.base)).json(&body)
        } else {
            self.client.get(format!("{}{path}", self.base))
        }
        .query(&[("directory", &self.cwd)]);
        if let Ok(password) = std::env::var("OPENCODE_SERVER_PASSWORD") {
            request = request.basic_auth(
                std::env::var("OPENCODE_SERVER_USERNAME").unwrap_or("opencode".into()),
                Some(password),
            );
        }
        let response = request.send()?.error_for_status()?;
        if response.status() == 204 {
            return Ok(Value::Null);
        }
        let mut bytes = Vec::new();
        response.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() <= 1024 * 1024, "OpenCode response too large");
        if bytes.is_empty() {
            Ok(Value::Null)
        } else {
            Ok(serde_json::from_slice(&bytes)?)
        }
    }
    fn get(&self, path: &str) -> anyhow::Result<Value> {
        self.request(path, None)
    }
    fn post(&self, path: &str, body: Value) -> anyhow::Result<Value> {
        self.request(path, Some(body))
    }
    fn prompt(&mut self, text: &str) -> anyhow::Result<()> {
        let mut body = json!({"parts":[{"type":"text","text":text}]});
        if let Some(model) = &self.model {
            let (provider, model) = model
                .split_once('/')
                .ok_or_else(|| anyhow::anyhow!("OpenCode model must be provider/model"))?;
            body["model"] = json!({"providerID":provider,"modelID":model});
        }
        anyhow::ensure!(
            !self.active.swap(true, Ordering::SeqCst),
            "turn_already_running"
        );
        self.seen_busy.store(false, Ordering::SeqCst);
        let result = self.post(&format!("/session/{}/prompt_async", self.session), body);
        if result.is_err() {
            self.active.store(false, Ordering::SeqCst);
        }
        result.map(|_| ())
    }
}
impl Driver for OpenCode {
    fn execute(&mut self, a: &SessionAction) -> anyhow::Result<Option<Value>> {
        match a {
            SessionAction::SendTurn { text } => self.prompt(text)?,
            SessionAction::Interrupt => {
                self.post(&format!("/session/{}/abort", self.session), json!({}))?;
            }
            SessionAction::Close => {
                self.post(&format!("/session/{}/abort", self.session), json!({}))?;
                self.closed.store(true, Ordering::SeqCst);
            }
            _ => anyhow::bail!("unsupported_operation"),
        };
        Ok(None)
    }
}

fn map_part(emit: &Emit, event: &Value, parts: &mut std::collections::HashMap<String, Value>) {
    let p = &event["properties"];
    let part = &p["part"];
    let Some(id) = p["partID"].as_str().or_else(|| part["id"].as_str()) else {
        return;
    };
    if !parts.contains_key(id) && parts.len() >= 1024 {
        emit(SessionEventKind::Error {
            code: "native_part_capacity".into(),
            message: "OpenCode part cache exhausted".into(),
        });
        emit(SessionEventKind::State(SessionStatus::Failed));
        return;
    }
    let old = parts.entry(id.into()).or_insert(json!({}));
    if event["type"] == "message.part.delta" {
        if p["field"] == "text" {
            if let Some(delta) = p["delta"].as_str() {
                let content = old["text"].as_str().unwrap_or("").to_owned() + delta;
                if content.len() > 1024 * 1024 {
                    emit(SessionEventKind::State(SessionStatus::Failed));
                    return;
                }
                old["text"] = json!(content);
                text(
                    emit,
                    SessionBackend::OpenCode,
                    delta,
                    old["type"] == "reasoning",
                );
            }
        }
        return;
    }
    match part["type"].as_str().unwrap_or("") {
        "text" | "reasoning" => {
            let current = part["text"].as_str().unwrap_or("");
            let previous = old["text"].as_str().unwrap_or("");
            if let Some(delta) = current.strip_prefix(previous) {
                if !delta.is_empty() {
                    text(
                        emit,
                        SessionBackend::OpenCode,
                        delta,
                        part["type"] == "reasoning",
                    );
                }
            }
        }
        "tool" => {
            let call_id = part["callID"].as_str().unwrap_or(id).to_string();
            if old["type"] != "tool" {
                emit_agent(
                    emit,
                    SessionBackend::OpenCode,
                    AgentEventPayload::ToolUse {
                        call_id: call_id.clone(),
                        tool_name: part["tool"].as_str().unwrap_or("tool").into(),
                        input: part.pointer("/state/input").cloned().unwrap_or(Value::Null),
                    },
                );
            }
            let state = &part["state"];
            if (state["status"] == "completed" || state["status"] == "error")
                && old.pointer("/state/status") != Some(&state["status"])
            {
                let start = state.pointer("/time/start").and_then(Value::as_i64);
                let end = state.pointer("/time/end").and_then(Value::as_i64);
                emit_agent(
                    emit,
                    SessionBackend::OpenCode,
                    AgentEventPayload::ToolResult {
                        call_id,
                        output: state.clone(),
                        is_error: state["status"] == "error",
                        duration_ms: start.zip(end).and_then(|(s, e)| u64::try_from(e - s).ok()),
                        started_at_ms: start,
                    },
                );
            }
        }
        _ => {}
    }
    *old = part.clone();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_snapshots_do_not_duplicate_deltas_and_tools_are_canonical() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let copy = events.clone();
        let emit: Emit = Arc::new(move |e| copy.lock().unwrap().push(e));
        let mut parts = std::collections::HashMap::new();
        map_part(
            &emit,
            &json!({"type":"message.part.updated","properties":{"part":{"id":"p","type":"text","text":"hello"}}}),
            &mut parts,
        );
        map_part(
            &emit,
            &json!({"type":"message.part.delta","properties":{"partID":"p","field":"text","delta":" world"}}),
            &mut parts,
        );
        map_part(
            &emit,
            &json!({"type":"message.part.updated","properties":{"part":{"id":"p","type":"text","text":"hello world"}}}),
            &mut parts,
        );
        map_part(
            &emit,
            &json!({"type":"message.part.updated","properties":{"part":{"id":"tool","type":"tool","callID":"t","tool":"read","state":{"status":"completed","output":"file"}}}}),
            &mut parts,
        );
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 4);
        assert!(
            matches!(&events[2],SessionEventKind::Agent(e)if matches!(e.payload,AgentEventPayload::ToolUse{..}))
        );
        assert!(
            matches!(&events[3],SessionEventKind::Agent(e)if matches!(e.payload,AgentEventPayload::ToolResult{..}))
        );
    }
    #[test]
    fn native_http_paths_and_model_are_wired() {
        let mut server = mockito::Server::new();
        let query = mockito::Matcher::UrlEncoded("directory".into(), "/workspace".into());
        let create = server
            .mock("POST", "/session")
            .match_query(query.clone())
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"native-session"}"#)
            .create();
        let stream = server
            .mock("GET", "/event")
            .match_query(query.clone())
            .with_header("content-type", "text/event-stream")
            .with_body("data: {\"type\":\"server.connected\",\"properties\":{}}\n\n")
            .create();
        let prompt=server.mock("POST","/session/native-session/prompt_async").match_query(query.clone()).match_body(mockito::Matcher::Json(json!({"model":{"providerID":"provider","modelID":"model"},"parts":[{"type":"text","text":"hello"}]}))).with_status(204).create();
        let abort = server
            .mock("POST", "/session/native-session/abort")
            .match_query(query)
            .with_status(204)
            .create();
        let req = CreateSession {
            command_id: "c".into(),
            backend: SessionBackend::OpenCode,
            cwd: "/workspace".into(),
            prompt: "hello".into(),
            model: Some("provider/model".into()),
            resume: None,
            permission_policy: PermissionPolicy::Ask,
        };
        let (mut driver, _) = open_at(
            &req,
            Arc::new(|_| {}),
            Arc::new(|_, _| RequestResponse::Deny),
            server.url(),
        )
        .unwrap();
        driver.execute(&SessionAction::Close).unwrap();
        create.assert();
        stream.assert();
        prompt.assert();
        abort.assert();
    }
}
