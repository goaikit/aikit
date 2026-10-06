use super::*;
use crate::integration::{HookCommand, InstallSpec};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct Fixture {
    _dir: tempfile::TempDir,
    service: IntegrationService,
    id: String,
    workspace: PathBuf,
}
impl Fixture {
    fn new(timeout_seconds: u32) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let service = IntegrationService::open(dir.path().join("state")).unwrap();
        let plan = service
            .plan_install(InstallSpec {
                application_id: "example".into(),
                agent_key: "claude".into(),
                workspace: workspace.clone(),
                handler: HookCommand {
                    executable: std::env::current_exe().unwrap(),
                    arguments: vec![],
                },
                events: vec![
                    HookEvent::SessionStarted,
                    HookEvent::InputSubmitted,
                    HookEvent::BeforeTool,
                    HookEvent::AfterTool,
                    HookEvent::ToolFailed,
                    HookEvent::CompletionProposed,
                    HookEvent::SessionEnded,
                ],
                timeout_seconds,
            })
            .unwrap();
        service.apply_install(&plan.id).unwrap();
        Self {
            _dir: dir,
            service,
            id: plan.installation_id,
            workspace,
        }
    }
    fn input(&self, event: &str) -> Value {
        json!({"hook_event_name":event,"session_id":"native-session","cwd":self.workspace,"prompt_id":"prompt","transcript_path":"not opened or persisted"})
    }
}

struct Fixed(Decision);
impl HookHandler for Fixed {
    fn decide<'a>(&'a self, _: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(async { Ok(self.0.clone()) })
    }
}
struct Fail;
impl HookHandler for Fail {
    fn decide<'a>(&'a self, _: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(async { Err(HandlerError("private diagnostic".into())) })
    }
}

struct Panic;
impl HookHandler for Panic {
    fn decide<'a>(&'a self, _: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(async { panic!("callback bug") })
    }
}

#[tokio::test]
async fn application_allow_does_not_override_native_permissions_and_tools_use_canonical_payloads() {
    let f = Fixture::new(10);
    let mut input = f.input("PreToolUse");
    input["tool_name"] = json!("Write");
    input["tool_use_id"] = json!("call1");
    input["tool_input"] = json!({"file_path":"/project/a.rs","content":"hello"});
    let response = f
        .service
        .handle_hook(&f.id, input.to_string().as_bytes(), &Fixed(Decision::Allow))
        .await;
    assert_eq!(response.exit_code, 0);
    assert_eq!(response.stdout, "{}");
    let page = f.service.events(&f.id, 0, 10).unwrap();
    assert_eq!(page.records.len(), 2);
    assert!(
        matches!(&page.records[0].request.payload, AgentEventPayload::ToolUse { call_id, tool_name, .. } if call_id == "call1" && tool_name == "Write")
    );
    assert!(page.records[0].decision.is_none());
    assert_eq!(page.records[1].decision, Some(Decision::Allow));
    assert_eq!(page.records[0].request.id, page.records[1].request.id);
    assert!(!serde_json::to_string(&page)
        .unwrap()
        .contains("transcript_path"));
}

#[tokio::test]
async fn block_encodings_match_the_native_decision_point() {
    let f = Fixture::new(10);
    let handler = Fixed(Decision::Block {
        reason: "current evidence required".into(),
    });
    for event in ["PreToolUse", "Stop", "UserPromptSubmit"] {
        let mut input = f.input(event);
        input["tool_name"] = json!("Bash");
        input["tool_use_id"] = json!("call");
        input["tool_input"] = json!({});
        let response = f
            .service
            .handle_hook(&f.id, input.to_string().as_bytes(), &handler)
            .await;
        let value: Value = serde_json::from_str(&response.stdout).unwrap();
        assert_eq!(response.exit_code, 0);
        if event == "PreToolUse" {
            assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
        } else {
            assert_eq!(value["decision"], "block");
        }
    }
}

struct Count {
    calls: Arc<AtomicUsize>,
}
impl HookHandler for Count {
    fn decide<'a>(&'a self, request: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(async move {
            assert_eq!(request.final_answer.as_deref(), Some("Final response"));
            assert!(request.stop_hook_active);
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Decision::Block {
                reason: "still incomplete".into(),
            })
        })
    }
}
#[tokio::test]
async fn every_repeated_stop_requires_fresh_validation_and_preserves_final_answer() {
    let f = Fixture::new(10);
    let mut input = f.input("Stop");
    input["last_assistant_message"] = json!("Final response");
    input["stop_hook_active"] = json!(true);
    let calls = Arc::new(AtomicUsize::new(0));
    let handler = Count {
        calls: calls.clone(),
    };
    for _ in 0..10 {
        let response = f
            .service
            .handle_hook(&f.id, input.to_string().as_bytes(), &handler)
            .await;
        assert!(response.stdout.contains("block"));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 10);
    assert_eq!(f.service.events(&f.id, 0, 100).unwrap().records.len(), 20);
    // This is callback behavior only; native continuation limits require live qualification.
}

struct Never;
impl HookHandler for Never {
    fn decide<'a>(&'a self, _: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(std::future::pending())
    }
}
#[tokio::test]
async fn callback_timeout_and_error_block_without_exposing_private_diagnostics() {
    let f = Fixture::new(1);
    let input = f.input("Stop").to_string();
    for handler in [&Never as &dyn HookHandler, &Fail, &Panic] {
        let response = f
            .service
            .handle_hook(&f.id, input.as_bytes(), handler)
            .await;
        assert!(response.stdout.contains("block"));
        assert!(!response.stdout.contains("private diagnostic"));
    }
}

#[tokio::test]
async fn tool_content_is_available_to_policy_but_not_retained_in_the_event_journal() {
    struct Policy;
    impl HookHandler for Policy {
        fn decide<'a>(&'a self, request: &'a HookRequest) -> DecisionFuture<'a> {
            Box::pin(async move {
                let AgentEventPayload::ToolUse { input, .. } = &request.payload else {
                    panic!()
                };
                assert_eq!(input["content"], "private-file-content");
                Ok(Decision::Allow)
            })
        }
    }
    let f = Fixture::new(10);
    let mut input = f.input("PreToolUse");
    input["tool_name"] = json!("Write");
    input["tool_use_id"] = json!("call1");
    input["tool_input"] = json!({"content":"private-file-content"});
    let response = f
        .service
        .handle_hook(&f.id, input.to_string().as_bytes(), &Policy)
        .await;
    assert_eq!(response.stdout, "{}");
    let page = f.service.events(&f.id, 0, 10).unwrap();
    assert!(page
        .records
        .iter()
        .all(|record| record.tool_payload_omitted));
    assert!(!serde_json::to_string(&page)
        .unwrap()
        .contains("private-file-content"));
}

#[tokio::test]
async fn malformed_missing_or_foreign_workspace_inputs_exit_with_blocking_status() {
    let f = Fixture::new(10);
    let mut outside = f.input("Stop");
    outside["cwd"] = json!(f._dir.path());
    for input in [
        b"{".to_vec(),
        f.input("UnregisteredEvent").to_string().into_bytes(),
        outside.to_string().into_bytes(),
        vec![b' '; 1024 * 1024 + 1],
    ] {
        let response = f
            .service
            .handle_hook(&f.id, &input, &Fixed(Decision::Allow))
            .await;
        assert_eq!(response.exit_code, 2);
        assert!(response.stdout.is_empty());
    }
    assert!(f.service.events(&f.id, 0, 10).unwrap().records.is_empty());
}

struct ObserveDuringDecision<'a> {
    service: &'a IntegrationService,
    id: &'a str,
    cursor: Arc<AtomicUsize>,
}
impl HookHandler for ObserveDuringDecision<'_> {
    fn decide<'a>(&'a self, _: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(async move {
            let page = self.service.events(self.id, 0, 10).unwrap();
            assert_eq!(page.records.len(), 1);
            self.cursor
                .store(page.next_cursor as usize, Ordering::SeqCst);
            Ok(Decision::Allow)
        })
    }
}
#[tokio::test]
async fn checkpointing_an_observation_does_not_lose_the_later_decision() {
    let f = Fixture::new(10);
    let cursor = Arc::new(AtomicUsize::new(0));
    let handler = ObserveDuringDecision {
        service: &f.service,
        id: &f.id,
        cursor: cursor.clone(),
    };
    f.service
        .handle_hook(&f.id, f.input("Stop").to_string().as_bytes(), &handler)
        .await;
    let decision = f
        .service
        .events(&f.id, cursor.load(Ordering::SeqCst) as u64, 10)
        .unwrap();
    assert_eq!(decision.records.len(), 1);
    assert_eq!(decision.records[0].decision, Some(Decision::Allow));
    let remove = f.service.plan_remove(&f.id).unwrap();
    f.service.apply_install(&remove.id).unwrap();
    assert_eq!(
        f.service.events(&f.id, 0, 10).unwrap().records.len(),
        2,
        "uninstallation preserves historical evidence"
    );
}

#[tokio::test]
async fn session_end_is_observation_not_successful_turn_completion() {
    let f = Fixture::new(10);
    let response = f
        .service
        .handle_hook(&f.id, f.input("SessionEnd").to_string().as_bytes(), &Never)
        .await;
    assert_eq!(response.stdout, "{}");
    let page = f.service.events(&f.id, 0, 10).unwrap();
    assert_eq!(page.records.len(), 1);
    assert!(page.records[0].decision.is_none());
    assert!(!matches!(
        page.records[0].request.payload,
        AgentEventPayload::Terminal { .. }
    ));
}
