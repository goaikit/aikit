use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::{json, Value};

use crate::{error::ToolsError, schema, state::ToolsState, tool::AgentTool};

pub async fn list_tools_handler(
    State(state): State<ToolsState>,
) -> Result<Json<Value>, ToolsError> {
    let tools = state.registry.list();
    Ok(Json(json!({ "tools": tools })))
}

pub async fn get_schema_handler(
    State(state): State<ToolsState>,
    Path((ns, tool)): Path<(String, String)>,
) -> Result<Json<Value>, ToolsError> {
    let tool = state
        .registry
        .resolve(&ns, &tool)
        .ok_or_else(|| ToolsError::ToolNotFound(format!("no tool registered at {ns}/{tool}")))?;
    Ok(Json(tool.schema_doc()))
}

pub async fn invoke_handler(
    State(state): State<ToolsState>,
    Path((ns, tool_name)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ToolsError> {
    let tool = state.registry.resolve(&ns, &tool_name).ok_or_else(|| {
        ToolsError::ToolNotFound(format!("no tool registered at {ns}/{tool_name}"))
    })?;

    // Validate input
    schema::validate(&tool.input_validator, &body).map_err(ToolsError::InputInvalid)?;

    let system_prompt = tool.system_prompt.clone();
    let user_prompt = compose_prompt(&tool, &body);
    let output_schema = tool.output_schema.clone();
    let runner = state.runner.clone();

    let events = tokio::task::spawn_blocking(move || {
        runner.run(&system_prompt, &user_prompt, &output_schema)
    })
    .await
    .map_err(|e| ToolsError::Internal(format!("spawn_blocking join error: {e}")))??;

    let draft = crate::runner::capture_output(&events)?;

    // Validate output
    schema::validate(&tool.output_validator, &draft).map_err(ToolsError::OutputInvalid)?;

    Ok(Json(json!({ "draft": draft })))
}

fn compose_prompt(tool: &AgentTool, input: &Value) -> String {
    format!(
        "You are a {} assistant. Your task: {}\n\nInput data:\n{}\n\nProcess the input and call emit_output exactly once with a conforming result.",
        tool.name,
        tool.description,
        serde_json::to_string_pretty(input).unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
        Router,
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use crate::{
        reference::draft_contact_tool,
        registry::ToolRegistry,
        runner::{AgentRunner, MockRunner},
        state::ToolsState,
        AgentInternalEvent,
    };

    fn make_state(runner: impl AgentRunner + 'static) -> ToolsState {
        let mut registry = ToolRegistry::new();
        registry.register(draft_contact_tool());
        ToolsState {
            registry: Arc::new(registry),
            runner: Arc::new(runner),
        }
    }

    fn make_app(state: ToolsState) -> Router {
        crate::router(state)
    }

    async fn do_request(
        app: Router,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let req = if let Some(b) = body {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&b).unwrap()))
                .unwrap()
        } else {
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap()
        };

        let response = app.oneshot(req).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    fn valid_emit_output() -> AgentInternalEvent {
        AgentInternalEvent::ToolUse {
            tool_name: "emit_output".to_string(),
            tool_input: json!({
                "first_name": "Jane",
                "last_name": "Doe",
                "email": "jane@acme.com",
                "company": "Acme",
                "title": "VP Sales",
                "notes": "Met at conference"
            }),
            call_id: "c1".to_string(),
        }
    }

    #[tokio::test]
    async fn list_tools_returns_draft_contact() {
        let runner = MockRunner { canned: vec![] };
        let app = make_app(make_state(runner));
        let (status, body) = do_request(app, Method::GET, "/aitools", None).await;
        assert_eq!(status, StatusCode::OK);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["namespace"], "crm");
        assert_eq!(tools[0]["name"], "draft_contact");
        assert!(tools[0]["description"].as_str().is_some());
    }

    #[tokio::test]
    async fn get_schema_returns_200() {
        let runner = MockRunner { canned: vec![] };
        let app = make_app(make_state(runner));
        let (status, body) =
            do_request(app, Method::GET, "/aitools/crm/draft_contact/schema", None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.get("inputSchema").is_some());
        assert!(body.get("outputSchema").is_some());
        assert!(body.get("description").is_some());
        assert!(body.get("namespace").is_some());
        assert!(body.get("name").is_some());
    }

    #[tokio::test]
    async fn get_schema_unknown_returns_404() {
        let runner = MockRunner { canned: vec![] };
        let app = make_app(make_state(runner));
        let (status, body) =
            do_request(app, Method::GET, "/aitools/crm/unknown_tool/schema", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "tool_not_found");
    }

    #[tokio::test]
    async fn invoke_valid_returns_draft() {
        let runner = MockRunner {
            canned: vec![valid_emit_output()],
        };
        let app = make_app(make_state(runner));
        let (status, body) = do_request(
            app,
            Method::POST,
            "/aitools/crm/draft_contact",
            Some(json!({"note": "test", "partial": {}})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["draft"]["first_name"].as_str().is_some());
        assert!(body["draft"]["last_name"].as_str().is_some());
    }

    #[tokio::test]
    async fn invoke_missing_note_returns_400() {
        let runner = MockRunner { canned: vec![] };
        let app = make_app(make_state(runner));
        let (status, body) = do_request(
            app,
            Method::POST,
            "/aitools/crm/draft_contact",
            Some(json!({"partial": {}})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "input_invalid");
    }

    #[tokio::test]
    async fn invoke_output_missing_last_name_returns_502() {
        let runner = MockRunner {
            canned: vec![AgentInternalEvent::ToolUse {
                tool_name: "emit_output".to_string(),
                tool_input: json!({"first_name": "Jane"}),
                call_id: "c1".to_string(),
            }],
        };
        let app = make_app(make_state(runner));
        let (status, body) = do_request(
            app,
            Method::POST,
            "/aitools/crm/draft_contact",
            Some(json!({"note": "test"})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"]["code"], "output_invalid");
    }

    #[tokio::test]
    async fn invoke_agent_error_returns_502() {
        let runner = MockRunner {
            canned: vec![AgentInternalEvent::Error {
                code: "err".to_string(),
                message: "agent failed".to_string(),
            }],
        };
        let app = make_app(make_state(runner));
        let (status, body) = do_request(
            app,
            Method::POST,
            "/aitools/crm/draft_contact",
            Some(json!({"note": "test"})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"]["code"], "agent_failed");
    }

    struct InternalErrorRunner;

    impl AgentRunner for InternalErrorRunner {
        fn run(
            &self,
            _system_prompt: &str,
            _user_prompt: &str,
            _output_schema: &Value,
        ) -> Result<Vec<AgentInternalEvent>, crate::error::ToolsError> {
            Err(crate::error::ToolsError::Internal(
                "test internal error".to_string(),
            ))
        }
    }

    #[tokio::test]
    async fn invoke_runner_internal_error_returns_500() {
        let app = make_app(make_state(InternalErrorRunner));
        let (status, body) = do_request(
            app,
            Method::POST,
            "/aitools/crm/draft_contact",
            Some(json!({"note": "test"})),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["code"], "internal");
    }
}
