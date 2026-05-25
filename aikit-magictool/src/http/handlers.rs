use super::error::ToolError;
use super::state::MagicToolState;
use crate::core::{
    executor::{ChatEvent, ExecError},
    tool::compose_prompt,
};
use axum::{
    extract::{Path, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

// ── content negotiation ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseMode {
    Sse,
    Sync,
    NotAcceptable,
}

fn resolve_response_mode(headers: &axum::http::HeaderMap) -> ResponseMode {
    let raw = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    if raw.is_empty() || raw.contains("*/*") {
        return ResponseMode::Sse;
    }

    let wants_sse = raw.contains("text/event-stream");
    let wants_json = raw.contains("application/json");

    match (wants_sse, wants_json) {
        (true, _) => ResponseMode::Sse,
        (false, true) => ResponseMode::Sync,
        (false, false) => ResponseMode::NotAcceptable,
    }
}

// ── SSE event conversion ───────────────────────────────────────────────────────

fn chat_event_to_sse(event: ChatEvent) -> Result<Event, std::convert::Infallible> {
    let sse_event = match event {
        ChatEvent::Started { session_id } => Event::default()
            .event("started")
            .data(json!({"session_id": session_id}).to_string()),
        ChatEvent::Delta(text) => Event::default()
            .event("delta")
            .data(json!({"text": text}).to_string()),
        ChatEvent::Final(text) => Event::default()
            .event("final")
            .data(json!({"reply": text}).to_string()),
        ChatEvent::Error(msg) => Event::default()
            .event("error")
            .data(json!({"message": msg}).to_string()),
    };
    Ok(sse_event)
}

fn exec_error_to_tool_error(e: ExecError) -> ToolError {
    match e {
        ExecError::AgentFailed(m) => ToolError::AgentFailed(m),
        ExecError::OutputInvalid(m) => ToolError::OutputInvalid(m),
        ExecError::Internal(m) => ToolError::Internal(m),
    }
}

// ── GET /aitools ───────────────────────────────────────────────────────────────

async fn list_handler(State(state): State<MagicToolState>) -> impl IntoResponse {
    let tools = state.registry.list();
    Json(json!({ "tools": tools }))
}

// ── GET /aitools/{ns}/{tool}/schema ───────────────────────────────────────────

async fn schema_handler(
    State(state): State<MagicToolState>,
    Path((ns, name)): Path<(String, String)>,
) -> Response {
    let tool = match state.registry.get(&ns, &name) {
        Some(t) => t,
        None => return ToolError::ToolNotFound.into_response(),
    };

    let modes = if state.chat.is_some() {
        json!(["one_shot", "multi_turn"])
    } else {
        json!(["one_shot"])
    };

    Json(json!({
        "namespace": tool.namespace,
        "name": tool.name,
        "description": tool.description,
        "inputSchema": tool.input_schema,
        "outputSchema": tool.output_schema,
        "modes": modes,
    }))
    .into_response()
}

// ── POST /aitools/{ns}/{tool} (one-shot) ──────────────────────────────────────

async fn invoke_handler(
    State(state): State<MagicToolState>,
    Path((ns, name)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    // Validate input and get a clone of needed data before spawn_blocking
    let input_errors = {
        let tool = match state.registry.get(&ns, &name) {
            Some(t) => t,
            None => return ToolError::ToolNotFound.into_response(),
        };
        tool.validate_input(&body).err()
    };

    if let Some(errors) = input_errors {
        return ToolError::InputInvalid(errors.join("; ")).into_response();
    }

    let registry = state.registry.clone();
    let executor = state.executor.clone();
    let ns_c = ns.clone();
    let name_c = name.clone();
    let body_c = body.clone();

    let result = tokio::task::spawn_blocking(move || {
        let tool = registry.get(&ns_c, &name_c).unwrap();
        executor.execute(tool, &body_c)
    })
    .await;

    let raw = match result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return exec_error_to_tool_error(e).into_response(),
        Err(_) => return ToolError::Internal("task join failed".to_owned()).into_response(),
    };

    // Validate output against output_schema (L2 validation for all backends)
    {
        let tool = match state.registry.get(&ns, &name) {
            Some(t) => t,
            None => return ToolError::ToolNotFound.into_response(),
        };
        if let Err(errors) = tool.validate_output(&raw) {
            return ToolError::OutputInvalid(errors.join("; ")).into_response();
        }
    }

    Json(json!({ "draft": raw })).into_response()
}

// ── POST /aitools/{ns}/{tool}/sessions (multi-turn start) ─────────────────────

async fn session_start_handler(
    State(state): State<MagicToolState>,
    Path((ns, name)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let chat = match &state.chat {
        Some(c) => c.clone(),
        None => return ToolError::ChatUnavailable.into_response(),
    };

    let tool_data = {
        let tool = match state.registry.get(&ns, &name) {
            Some(t) => t,
            None => return ToolError::ToolNotFound.into_response(),
        };
        if let Err(errors) = tool.validate_input(&body) {
            return ToolError::InputInvalid(errors.join("; ")).into_response();
        }
        (tool.clone(), compose_prompt(tool, &body))
    };

    let (tool_def, prompt) = tool_data;
    let mode = resolve_response_mode(&headers);

    if mode == ResponseMode::NotAcceptable {
        return (axum::http::StatusCode::NOT_ACCEPTABLE, "Not Acceptable").into_response();
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<ChatEvent>(64);
    let tx_clone = tx.clone();
    drop(tx);

    tokio::task::spawn_blocking(move || {
        let mut sink = |event: ChatEvent| {
            let _ = tx_clone.blocking_send(event);
        };
        chat.run_turn(&tool_def, None, &prompt, &mut sink)
    });

    match mode {
        ResponseMode::Sse => {
            let stream = ReceiverStream::new(rx).map(chat_event_to_sse);
            Sse::new(stream)
                .keep_alive(KeepAlive::default())
                .into_response()
        }
        ResponseMode::Sync => {
            let mut session_id = String::new();
            let mut reply = String::new();
            let mut rx = rx;
            while let Some(event) = rx.recv().await {
                match event {
                    ChatEvent::Started { session_id: sid } => session_id = sid,
                    ChatEvent::Final(text) => reply = text,
                    _ => {}
                }
            }
            Json(json!({ "session_id": session_id, "reply": reply })).into_response()
        }
        ResponseMode::NotAcceptable => unreachable!(),
    }
}

// ── POST /aitools/{ns}/{tool}/sessions/{id}/messages ─────────────────────────

#[derive(Deserialize)]
struct MessageRequest {
    content: String,
}

async fn session_message_handler(
    State(state): State<MagicToolState>,
    Path((ns, name, session_id)): Path<(String, String, String)>,
    headers: axum::http::HeaderMap,
    Json(body): Json<MessageRequest>,
) -> Response {
    let chat = match &state.chat {
        Some(c) => c.clone(),
        None => return ToolError::ChatUnavailable.into_response(),
    };

    let tool_def = match state.registry.get(&ns, &name) {
        Some(t) => t.clone(),
        None => return ToolError::ToolNotFound.into_response(),
    };

    let mode = resolve_response_mode(&headers);

    if mode == ResponseMode::NotAcceptable {
        return (axum::http::StatusCode::NOT_ACCEPTABLE, "Not Acceptable").into_response();
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<ChatEvent>(64);
    let tx_clone = tx.clone();
    drop(tx);
    let msg = body.content.clone();
    let sid = session_id.clone();

    tokio::task::spawn_blocking(move || {
        let mut sink = |event: ChatEvent| {
            let _ = tx_clone.blocking_send(event);
        };
        chat.run_turn(&tool_def, Some(&sid), &msg, &mut sink)
    });

    match mode {
        ResponseMode::Sse => {
            let stream = ReceiverStream::new(rx).map(chat_event_to_sse);
            Sse::new(stream)
                .keep_alive(KeepAlive::default())
                .into_response()
        }
        ResponseMode::Sync => {
            let mut reply = String::new();
            let mut rx = rx;
            while let Some(event) = rx.recv().await {
                if let ChatEvent::Final(text) = event {
                    reply = text;
                }
            }
            Json(json!({ "reply": reply })).into_response()
        }
        ResponseMode::NotAcceptable => unreachable!(),
    }
}

// ── POST /aitools/{ns}/{tool}/sessions/{id}/finalize ─────────────────────────

async fn session_finalize_handler(
    State(state): State<MagicToolState>,
    Path((ns, name, session_id)): Path<(String, String, String)>,
) -> Response {
    let chat = match &state.chat {
        Some(c) => c.clone(),
        None => return ToolError::ChatUnavailable.into_response(),
    };

    let tool_def = match state.registry.get(&ns, &name) {
        Some(t) => t.clone(),
        None => return ToolError::ToolNotFound.into_response(),
    };

    let result = tokio::task::spawn_blocking(move || chat.finalize(&tool_def, &session_id)).await;

    let draft = match result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return exec_error_to_tool_error(e).into_response(),
        Err(_) => return ToolError::Internal("task join failed".to_owned()).into_response(),
    };

    Json(json!({ "draft": draft })).into_response()
}

// ── router ─────────────────────────────────────────────────────────────────────

pub fn router(state: MagicToolState) -> Router {
    Router::new()
        .route("/aitools", get(list_handler))
        .route("/aitools/{ns}/{tool}/schema", get(schema_handler))
        .route("/aitools/{ns}/{tool}", post(invoke_handler))
        .route("/aitools/{ns}/{tool}/sessions", post(session_start_handler))
        .route(
            "/aitools/{ns}/{tool}/sessions/{id}/messages",
            post(session_message_handler),
        )
        .route(
            "/aitools/{ns}/{tool}/sessions/{id}/finalize",
            post(session_finalize_handler),
        )
        .with_state(state)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        executor::{ExecError, ToolChat, ToolExecutor},
        mock::{MockChat, MockExecutor},
        registry::ToolRegistry,
        tool::ToolDef,
    };
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use serde_json::json;
    use std::sync::Arc;
    use tower::ServiceExt;

    fn test_tool() -> ToolDef {
        ToolDef::new(
            "test",
            "echo",
            "Test tool",
            "Echo the input.",
            json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"}
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            json!({
                "type": "object",
                "properties": {
                    "result": {"type": "string"}
                },
                "required": ["result"],
                "additionalProperties": false
            }),
        )
    }

    fn make_state(
        executor: impl ToolExecutor + 'static,
        chat: Option<impl ToolChat + 'static>,
    ) -> MagicToolState {
        let mut registry = ToolRegistry::new();
        registry.register(test_tool());
        MagicToolState {
            registry: Arc::new(registry),
            executor: Arc::new(executor),
            chat: chat.map(|c| Arc::new(c) as Arc<dyn ToolChat>),
        }
    }

    fn make_state_no_chat(executor: impl ToolExecutor + 'static) -> MagicToolState {
        let mut registry = ToolRegistry::new();
        registry.register(test_tool());
        MagicToolState {
            registry: Arc::new(registry),
            executor: Arc::new(executor),
            chat: None,
        }
    }

    async fn send(app: Router, req: Request<Body>) -> (StatusCode, serde_json::Value) {
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(json!(null));
        (status, json)
    }

    async fn send_raw(app: Router, req: Request<Body>) -> (StatusCode, Vec<u8>) {
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, bytes.to_vec())
    }

    // AC6: list
    #[tokio::test]
    async fn test_list_tools() {
        let state = make_state_no_chat(MockExecutor::ok(json!({"result": "ok"})));
        let app = router(state);
        let req = Request::builder()
            .uri("/aitools")
            .body(Body::empty())
            .unwrap();
        let (status, body) = send(app, req).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["tools"].is_array());
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["namespace"], "test");
        assert_eq!(tools[0]["name"], "echo");
    }

    // schema found
    #[tokio::test]
    async fn test_schema_found() {
        let state = make_state_no_chat(MockExecutor::ok(json!({"result": "ok"})));
        let app = router(state);
        let req = Request::builder()
            .uri("/aitools/test/echo/schema")
            .body(Body::empty())
            .unwrap();
        let (status, body) = send(app, req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["namespace"], "test");
        assert_eq!(body["name"], "echo");
        assert!(body["inputSchema"].is_object());
        assert!(body["outputSchema"].is_object());
        let modes = body["modes"].as_array().unwrap();
        assert_eq!(modes, &[json!("one_shot")]);
    }

    // schema not found
    #[tokio::test]
    async fn test_schema_not_found() {
        let state = make_state_no_chat(MockExecutor::ok(json!({"result": "ok"})));
        let app = router(state);
        let req = Request::builder()
            .uri("/aitools/test/missing/schema")
            .body(Body::empty())
            .unwrap();
        let (status, body) = send(app, req).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "tool_not_found");
    }

    // schema modes includes multi_turn when chat is Some
    #[tokio::test]
    async fn test_schema_modes_multi_turn() {
        let chat = MockChat::new(
            vec![
                ChatEvent::Started {
                    session_id: "s1".to_owned(),
                },
                ChatEvent::Final("ok".to_owned()),
            ],
            json!({"result": "ok"}),
        );
        let state = make_state(MockExecutor::ok(json!({"result": "ok"})), Some(chat));
        let app = router(state);
        let req = Request::builder()
            .uri("/aitools/test/echo/schema")
            .body(Body::empty())
            .unwrap();
        let (status, body) = send(app, req).await;
        assert_eq!(status, StatusCode::OK);
        let modes = body["modes"].as_array().unwrap();
        assert!(modes.contains(&json!("multi_turn")));
    }

    // AC6: one-shot valid → 200 {"draft":...}
    #[tokio::test]
    async fn test_invoke_valid() {
        let state = make_state_no_chat(MockExecutor::ok(json!({"result": "hello"})));
        let app = router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/aitools/test/echo")
            .header("content-type", "application/json")
            .body(Body::from(json!({"name": "world"}).to_string()))
            .unwrap();
        let (status, body) = send(app, req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["draft"]["result"], "hello");
    }

    // AC7: missing required field → 400 input_invalid
    #[tokio::test]
    async fn test_invoke_input_invalid() {
        let state = make_state_no_chat(MockExecutor::ok(json!({"result": "ok"})));
        let app = router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/aitools/test/echo")
            .header("content-type", "application/json")
            .body(Body::from(json!({}).to_string()))
            .unwrap();
        let (status, body) = send(app, req).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "input_invalid");
    }

    // AC8: unknown tool → 404 tool_not_found
    #[tokio::test]
    async fn test_invoke_tool_not_found() {
        let state = make_state_no_chat(MockExecutor::ok(json!({"result": "ok"})));
        let app = router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/aitools/test/missing")
            .header("content-type", "application/json")
            .body(Body::from(json!({"name": "x"}).to_string()))
            .unwrap();
        let (status, body) = send(app, req).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "tool_not_found");
    }

    // AC9: executor returns bad output → 502 output_invalid
    #[tokio::test]
    async fn test_invoke_output_invalid() {
        // Return something that doesn't match outputSchema (missing "result" field)
        let state = make_state_no_chat(MockExecutor::ok(json!({"wrong_field": "value"})));
        let app = router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/aitools/test/echo")
            .header("content-type", "application/json")
            .body(Body::from(json!({"name": "x"}).to_string()))
            .unwrap();
        let (status, body) = send(app, req).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"]["code"], "output_invalid");
    }

    // AC10: executor returns AgentFailed → 502 agent_failed
    #[tokio::test]
    async fn test_invoke_agent_failed() {
        let state =
            make_state_no_chat(MockExecutor::err(ExecError::AgentFailed("boom".to_owned())));
        let app = router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/aitools/test/echo")
            .header("content-type", "application/json")
            .body(Body::from(json!({"name": "x"}).to_string()))
            .unwrap();
        let (status, body) = send(app, req).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"]["code"], "agent_failed");
    }

    // AC17: internal error → 500 internal
    #[tokio::test]
    async fn test_invoke_internal_error() {
        let state = make_state_no_chat(MockExecutor::err(ExecError::Internal("oops".to_owned())));
        let app = router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/aitools/test/echo")
            .header("content-type", "application/json")
            .body(Body::from(json!({"name": "x"}).to_string()))
            .unwrap();
        let (status, body) = send(app, req).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["code"], "internal");
    }

    // AC11: session start JSON mode
    #[tokio::test]
    async fn test_session_start_json() {
        let chat = MockChat::new(
            vec![
                ChatEvent::Started {
                    session_id: "sess-1".to_owned(),
                },
                ChatEvent::Delta("partial".to_owned()),
                ChatEvent::Final("full reply".to_owned()),
            ],
            json!({"result": "ok"}),
        );
        let state = make_state(MockExecutor::ok(json!({"result": "ok"})), Some(chat));
        let app = router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/aitools/test/echo/sessions")
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .body(Body::from(json!({"name": "x"}).to_string()))
            .unwrap();
        let (status, body) = send(app, req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["session_id"], "sess-1");
        assert_eq!(body["reply"], "full reply");
    }

    // AC12: session message JSON mode
    #[tokio::test]
    async fn test_session_message_json() {
        let chat = MockChat::new(
            vec![
                ChatEvent::Delta("partial".to_owned()),
                ChatEvent::Final("response".to_owned()),
            ],
            json!({"result": "ok"}),
        );
        let state = make_state(MockExecutor::ok(json!({"result": "ok"})), Some(chat));
        let app = router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/aitools/test/echo/sessions/sess-1/messages")
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .body(Body::from(json!({"content": "hello"}).to_string()))
            .unwrap();
        let (status, body) = send(app, req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["reply"], "response");
    }

    // AC13: session start SSE mode
    #[tokio::test]
    async fn test_session_start_sse() {
        let chat = MockChat::new(
            vec![
                ChatEvent::Started {
                    session_id: "sess-2".to_owned(),
                },
                ChatEvent::Delta("part".to_owned()),
                ChatEvent::Final("full".to_owned()),
            ],
            json!({"result": "ok"}),
        );
        let state = make_state(MockExecutor::ok(json!({"result": "ok"})), Some(chat));
        let app = router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/aitools/test/echo/sessions")
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .body(Body::from(json!({"name": "x"}).to_string()))
            .unwrap();
        let (status, bytes) = send_raw(app, req).await;
        assert_eq!(status, StatusCode::OK);
        let body = String::from_utf8(bytes).unwrap();
        assert!(body.contains("event: started"), "missing started: {body}");
        assert!(body.contains("event: delta"), "missing delta: {body}");
        assert!(body.contains("event: final"), "missing final: {body}");
    }

    // AC14: session message SSE mode
    #[tokio::test]
    async fn test_session_message_sse() {
        let chat = MockChat::new(
            vec![
                ChatEvent::Delta("part".to_owned()),
                ChatEvent::Final("done".to_owned()),
            ],
            json!({"result": "ok"}),
        );
        let state = make_state(MockExecutor::ok(json!({"result": "ok"})), Some(chat));
        let app = router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/aitools/test/echo/sessions/s1/messages")
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .body(Body::from(json!({"content": "hi"}).to_string()))
            .unwrap();
        let (status, bytes) = send_raw(app, req).await;
        assert_eq!(status, StatusCode::OK);
        let body = String::from_utf8(bytes).unwrap();
        assert!(body.contains("event: delta"), "missing delta: {body}");
        assert!(body.contains("event: final"), "missing final: {body}");
    }

    // AC15: finalize → 200 {"draft":...}
    #[tokio::test]
    async fn test_finalize() {
        let chat = MockChat::new(vec![], json!({"result": "finalized"}));
        let state = make_state(MockExecutor::ok(json!({"result": "ok"})), Some(chat));
        let app = router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/aitools/test/echo/sessions/sess-1/finalize")
            .body(Body::empty())
            .unwrap();
        let (status, body) = send(app, req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["draft"]["result"], "finalized");
    }

    // AC16: session route with chat=None → 501 chat_unavailable
    #[tokio::test]
    async fn test_session_no_chat() {
        let state = make_state_no_chat(MockExecutor::ok(json!({"result": "ok"})));
        let app = router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/aitools/test/echo/sessions")
            .header("content-type", "application/json")
            .body(Body::from(json!({"name": "x"}).to_string()))
            .unwrap();
        let (status, body) = send(app, req).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["error"]["code"], "chat_unavailable");
    }
}
