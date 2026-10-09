use super::*;
use crate::integration::{
    DecisionFuture, HookCommand, HookHandler, InstallSpec, InstallationStatus,
    IntegrationCapability, IntegrationService, SessionMode, Support,
};

struct Fixture {
    _dir: tempfile::TempDir,
    service: IntegrationService,
    installation: Installation,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace with spaces");
        std::fs::create_dir(&workspace).unwrap();
        let service = IntegrationService::open(dir.path().join("state")).unwrap();
        let plan = service
            .plan_install(InstallSpec {
                application_id: "cursor-fixture".into(),
                agent_key: "cursor".into(),
                workspace,
                handler: HookCommand {
                    executable: std::env::current_exe().unwrap(),
                    arguments: vec!["hook".into(), "argument with ' quotes $ and ;".into()],
                },
                events: vec![
                    HookEvent::SessionStarted,
                    HookEvent::InputSubmitted,
                    HookEvent::BeforeTool,
                    HookEvent::AfterTool,
                    HookEvent::ToolFailed,
                    HookEvent::SessionEnded,
                ],
                timeout_seconds: 2,
            })
            .unwrap();
        let InstallationStatus::Configured { installation } =
            service.apply_install(&plan.id).unwrap()
        else {
            panic!()
        };
        Self {
            _dir: dir,
            service,
            installation,
        }
    }
    fn input(&self, name: &str) -> Value {
        json!({"hook_event_name":name,"conversation_id":"conversation","generation_id":"generation","workspace_roots":[self.installation.spec.workspace],"user_email":"private@example.test","transcript_path":"private-transcript"})
    }
    async fn run(&self, input: Value, decision: Decision) -> super::super::HookResponse {
        self.service
            .handle_hook(
                &self.installation.id,
                input.to_string().as_bytes(),
                &Fixed(decision),
            )
            .await
    }
}
struct Fixed(Decision);

struct CaptureTool {
    decision: Decision,
    input: std::sync::Mutex<Option<Value>>,
}
impl HookHandler for CaptureTool {
    fn decide<'a>(&'a self, request: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(async move {
            let AgentEventPayload::ToolUse { input, .. } = &request.payload else {
                panic!("expected native tool request")
            };
            *self.input.lock().unwrap() = Some(input.clone());
            Ok(self.decision.clone())
        })
    }
}

#[tokio::test]
async fn native_utf8_bom_preserves_tool_payload_and_records_the_policy_decision() {
    let f = Fixture::new();
    let start = [
        b"\xef\xbb\xbf".as_slice(),
        f.input("sessionStart").to_string().as_bytes(),
    ]
    .concat();
    let response = f
        .service
        .handle_hook(&f.installation.id, &start, &Fixed(Decision::Allow))
        .await;
    assert_eq!(response.stdout, "{}");
    let mut tool = f.input("preToolUse");
    tool["tool_name"] = json!("Write");
    tool["tool_use_id"] = json!("bom-native-call");
    tool["tool_input"] = json!({"content":"\u{feff}literal payload é Ω"});
    let input = [b"\xef\xbb\xbf".as_slice(), tool.to_string().as_bytes()].concat();
    let decision = Decision::Block {
        reason: "review pending".into(),
    };
    let handler = CaptureTool {
        decision: decision.clone(),
        input: std::sync::Mutex::new(None),
    };
    let response = f
        .service
        .handle_hook(&f.installation.id, &input, &handler)
        .await;
    assert_eq!(
        serde_json::from_str::<Value>(&response.stdout).unwrap()["permission"],
        "deny"
    );
    let page = f.service.events(&f.installation.id, 0, 100).unwrap();
    assert!(page.records.iter().any(|record| {
        record.request.event == HookEvent::BeforeTool && record.decision.as_ref() == Some(&decision)
    }));
    assert_eq!(
        handler.input.lock().unwrap().as_ref(),
        Some(&tool["tool_input"])
    );
    // Durable records retain their existing privacy boundary.
    assert!(!serde_json::to_string(&page)
        .unwrap()
        .contains("literal payload"));
}

#[tokio::test]
async fn bom_compatibility_does_not_accept_extra_markers_oversize_or_foreign_roots() {
    let f = Fixture::new();
    let mut tool = f.input("preToolUse");
    tool["tool_name"] = json!("Write");
    tool["tool_use_id"] = json!("invalid-native-call");
    tool["tool_input"] = json!({"content":"body"});
    let duplicate = [
        b"\xef\xbb\xbf\xef\xbb\xbf".as_slice(),
        tool.to_string().as_bytes(),
    ]
    .concat();
    let mut oversized = [b"\xef\xbb\xbf".as_slice(), tool.to_string().as_bytes()].concat();
    oversized.resize(1024 * 1024 + 1, b' ');
    tool["workspace_roots"] = json!([f.installation.spec.workspace.parent().unwrap()]);
    let foreign = [b"\xef\xbb\xbf".as_slice(), tool.to_string().as_bytes()].concat();
    for input in [duplicate, oversized, foreign] {
        let response = f
            .service
            .handle_hook(&f.installation.id, &input, &Fixed(Decision::Allow))
            .await;
        assert_eq!(response.exit_code, 2);
        assert!(response.stdout.is_empty());
    }
    assert!(f
        .service
        .events(&f.installation.id, 0, 100)
        .unwrap()
        .records
        .is_empty());
}

impl HookHandler for Fixed {
    fn decide<'a>(&'a self, _: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(async { Ok(self.0.clone()) })
    }
}

#[test]
fn owned_cursor_config_preserves_other_hooks_and_refuses_drift_and_completion_substitution() {
    let f = Fixture::new();
    let path = &f.installation.config_path;
    let mut doc: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(doc["version"], 1);
    assert_eq!(doc["hooks"]["preToolUse"][0]["failClosed"], true);
    doc["hooks"]["afterFileEdit"] = json!([{"command":"existing-tool"}]);
    doc["custom"] = json!({"keep":true});
    std::fs::write(path, doc.to_string()).unwrap();
    let mut spec = f.installation.spec.clone();
    spec.events.push(HookEvent::CompletionProposed);
    assert!(matches!(
        f.service.plan_install(spec),
        Err(IntegrationError::Unsupported(_))
    ));
    let good = doc.clone();
    doc["version"] = json!(2);
    std::fs::write(path, doc.to_string()).unwrap();
    assert!(matches!(
        f.service.installation_status(&f.installation.id).unwrap(),
        InstallationStatus::Drifted { .. }
    ));
    assert!(f.service.plan_remove(&f.installation.id).is_err());
    std::fs::write(path, good.to_string()).unwrap();
    let removal = f.service.plan_remove(&f.installation.id).unwrap();
    f.service.apply_install(&removal.id).unwrap();
    let remaining: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        remaining,
        json!({"version":1,"hooks":{"afterFileEdit":[{"command":"existing-tool"}]},"custom":{"keep":true}})
    );
}

#[tokio::test]
async fn cursor_admission_tool_decisions_and_session_replay_share_the_existing_sdk_contracts() {
    let f = Fixture::new();
    assert_eq!(
        f.run(f.input("sessionStart"), Decision::Allow).await.stdout,
        "{}"
    );
    let reference = f
        .service
        .observed_session(&f.installation.id, "conversation")
        .unwrap();
    let binding = f.service.bind_existing(&reference).unwrap();
    let blocked = Decision::Block {
        reason: "review pending".into(),
    };
    assert_eq!(
        serde_json::from_str::<Value>(
            &f.run(f.input("beforeSubmitPrompt"), blocked.clone())
                .await
                .stdout
        )
        .unwrap(),
        json!({"continue":false,"user_message":"review pending"})
    );
    assert_eq!(
        f.run(f.input("beforeSubmitPrompt"), Decision::Allow)
            .await
            .stdout,
        "{\"continue\":true}"
    );
    let mut tool = f.input("preToolUse");
    tool["tool_name"] = json!("Write");
    tool["tool_use_id"] = json!("call");
    tool["tool_input"] = json!({"content":"private tool content"});
    let denied: Value = serde_json::from_str(&f.run(tool.clone(), blocked).await.stdout).unwrap();
    assert_eq!(denied["permission"], "deny");
    assert_eq!(
        f.run(tool, Decision::Allow).await.stdout,
        "{\"permission\":\"allow\"}"
    );
    let mut result = f.input("postToolUse");
    result["tool_use_id"] = json!("call");
    result["tool_output"] = json!("{\"result\":\"private output\"}");
    result["duration"] = json!(15);
    let decoded = decode(&f.installation, result.to_string().as_bytes()).unwrap();
    assert!(matches!(
        decoded.payload,
        AgentEventPayload::ToolResult {
            duration_ms: Some(15),
            is_error: false,
            ..
        }
    ));
    assert_eq!(f.run(result, Decision::Allow).await.stdout, "{}");
    let mut failure = f.input("postToolUseFailure");
    failure["tool_use_id"] = json!("call2");
    failure["error_message"] = json!("private error");
    assert_eq!(f.run(failure, Decision::Allow).await.stdout, "{}");
    assert_eq!(
        f.run(f.input("sessionEnd"), Decision::Allow).await.stdout,
        "{}"
    );
    assert_eq!(
        binding.status().unwrap(),
        crate::integration::SessionStatus::Ended
    );
    let page = binding.events(0, 100).unwrap();
    let text = serde_json::to_string(&page).unwrap();
    assert!(!text.contains("private"));
    assert!(page
        .records
        .iter()
        .all(|r| r.request.final_answer.is_none()));
    assert!(page.records.iter().any(|r| matches!(
        r.request.payload,
        AgentEventPayload::ToolResult { is_error: true, .. }
    )));
    binding.detach().unwrap();
    assert!(matches!(
        f.service.installation_status(&f.installation.id).unwrap(),
        InstallationStatus::Configured { .. }
    ));
}

#[tokio::test]
async fn cursor_rejects_ambiguous_scope_foreign_protocol_bad_tool_json_and_drift() {
    let f = Fixture::new();
    let mut multi = f.input("beforeSubmitPrompt");
    multi["workspace_roots"] =
        json!([f.installation.spec.workspace, f.installation.spec.workspace]);
    let mut identity = f.input("sessionStart");
    identity["session_id"] = json!("foreign");
    let mut result = f.input("postToolUse");
    result["tool_use_id"] = json!("call");
    result["tool_output"] = json!("not JSON");
    for value in [
        multi,
        identity,
        result,
        f.input("PreToolUse"),
        f.input("stop"),
    ] {
        assert_eq!(f.run(value, Decision::Allow).await.exit_code, 2);
    }
    assert!(f
        .service
        .events(&f.installation.id, 0, 100)
        .unwrap()
        .records
        .is_empty());
    let mut doc: Value =
        serde_json::from_slice(&std::fs::read(&f.installation.config_path).unwrap()).unwrap();
    doc["hooks"]["preToolUse"][0]["failClosed"] = json!(false);
    std::fs::write(&f.installation.config_path, doc.to_string()).unwrap();
    assert_eq!(
        f.run(f.input("beforeSubmitPrompt"), Decision::Allow)
            .await
            .exit_code,
        2
    );
    assert_eq!(
        f.service
            .events(&f.installation.id, 0, 100)
            .unwrap()
            .records
            .len(),
        1
    );
}

#[test]
fn cursor_capabilities_do_not_promote_fixture_decisions_or_completion_followups() {
    let f = Fixture::new();
    let report = f
        .service
        .capabilities("cursor", SessionMode::Print)
        .unwrap();
    report
        .require(&[
            IntegrationCapability::HookInstallation,
            IntegrationCapability::ObservationBinding,
            IntegrationCapability::DurableReplay,
            IntegrationCapability::SafeDetach,
        ])
        .unwrap();
    let unmet = report
        .require(&[
            IntegrationCapability::PreToolDecision,
            IntegrationCapability::CompletionDecision,
            IntegrationCapability::MessageSubmission,
        ])
        .unwrap_err();
    assert_eq!(unmet.unmet[0].support, Support::Unknown);
    assert_eq!(unmet.unmet[1].support, Support::Unsupported);
    assert_eq!(unmet.unmet[2].support, Support::Unsupported);
}
