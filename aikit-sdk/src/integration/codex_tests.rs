use super::*;
use crate::integration::{
    Decision, DecisionFuture, HookCommand, HookHandler, InstallSpec, InstallationStatus,
    IntegrationCapability, IntegrationService, SessionMode, SessionStatus, Support,
};

struct Fixture {
    _dir: tempfile::TempDir,
    service: IntegrationService,
    installation: Installation,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace ' $ é");
        std::fs::create_dir(&workspace).unwrap();
        let service = IntegrationService::open(dir.path().join("state")).unwrap();
        let plan = service
            .plan_install(InstallSpec {
                application_id: "codex-fixture".into(),
                agent_key: "codex".into(),
                workspace,
                handler: HookCommand {
                    executable: std::env::current_exe().unwrap(),
                    arguments: vec!["hook-codex".into(), "quote ' $ ;".into()],
                },
                events: vec![
                    HookEvent::SessionStarted,
                    HookEvent::InputSubmitted,
                    HookEvent::BeforeTool,
                    HookEvent::AfterTool,
                    HookEvent::CompletionProposed,
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
    fn input(&self, event: &str) -> Value {
        json!({"hook_event_name":event,"session_id":"session","turn_id":"turn",
            "cwd":self.installation.spec.workspace,"transcript_path":"private-transcript"})
    }
    async fn run(&self, input: Value, decision: Decision) -> crate::integration::HookResponse {
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
impl HookHandler for Fixed {
    fn decide<'a>(&'a self, _: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(async { Ok(self.0.clone()) })
    }
}

#[test]
fn codex_owned_config_preserves_native_trust_and_rejects_unsupported_contracts_atomically() {
    let f = Fixture::new();
    let path = &f.installation.config_path;
    assert!(path.ends_with(".codex/hooks.json"));
    let mut doc: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let handler = &doc["hooks"]["PreToolUse"][0]["hooks"][0];
    assert!(handler["command"]
        .as_str()
        .unwrap()
        .contains(if cfg!(windows) {
            "EncodedCommand"
        } else {
            "hook-codex"
        }));
    assert!(handler.get("args").is_none());
    assert!(handler.get("async").is_none());
    doc["hooks"]["PreToolUse"]
        .as_array_mut()
        .unwrap()
        .push(json!({"hooks":[{"type":"command","command":"other-owner"}]}));
    doc["description"] = json!("preserved");
    std::fs::write(path, doc.to_string()).unwrap();
    let config = path.parent().unwrap().join("config.toml");
    std::fs::write(
        &config,
        "# native user trust/config must remain unchanged\n",
    )
    .unwrap();
    let original = std::fs::read(path).unwrap();
    for missing in [HookEvent::ToolFailed, HookEvent::CompletionFailed] {
        let mut spec = f.installation.spec.clone();
        spec.events.push(missing);
        assert!(matches!(
            f.service.plan_install(spec),
            Err(IntegrationError::Unsupported(_))
        ));
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
    let mut spec = f.installation.spec.clone();
    spec.timeout_seconds = 4;
    assert!(matches!(
        f.service.plan_install(spec),
        Err(IntegrationError::Unsupported(_))
    ));
    let removal = f.service.plan_remove(&f.installation.id).unwrap();
    f.service.apply_install(&removal.id).unwrap();
    let remaining: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        remaining,
        json!({"description":"preserved","hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"other-owner"}]}]}})
    );
    assert_eq!(
        std::fs::read_to_string(config).unwrap(),
        "# native user trust/config must remain unchanged\n"
    );
}

#[tokio::test]
async fn codex_prompt_tool_and_repeated_stop_decisions_preserve_permissions_and_bindings() {
    let f = Fixture::new();
    assert_eq!(
        f.run(f.input("SessionStart"), Decision::Allow).await.stdout,
        "{}"
    );
    let reference = f
        .service
        .observed_session(&f.installation.id, "session")
        .unwrap();
    let binding = f.service.bind_existing(&reference).unwrap();
    let deny = Decision::Block {
        reason: "review pending".into(),
    };
    let blocked = f.run(f.input("UserPromptSubmit"), deny.clone()).await;
    assert_eq!(
        serde_json::from_str::<Value>(&blocked.stdout).unwrap(),
        json!({"decision":"block","reason":"review pending"})
    );
    let mut tool = f.input("PreToolUse");
    tool["tool_use_id"] = json!("call");
    tool["tool_name"] = json!("apply_patch");
    tool["tool_input"] = json!({"command":"private patch"});
    let decoded = decode(&f.installation, tool.to_string().as_bytes()).unwrap();
    assert_eq!(decoded.prompt_id.as_deref(), Some("turn"));
    assert_eq!(
        f.service.tool_effect(&decoded).unwrap(),
        crate::integration::ToolEffect::Unknown
    );
    assert_eq!(f.run(tool.clone(), Decision::Allow).await.stdout, "{}");
    let blocked: Value = serde_json::from_str(&f.run(tool, deny.clone()).await.stdout).unwrap();
    assert_eq!(blocked["hookSpecificOutput"]["permissionDecision"], "deny");
    let mut stop = f.input("Stop");
    stop["last_assistant_message"] = json!("answer");
    for active in [false, true, true] {
        stop["stop_hook_active"] = json!(active);
        let response: Value =
            serde_json::from_str(&f.run(stop.clone(), deny.clone()).await.stdout).unwrap();
        assert_eq!(response["decision"], "block");
        assert!(response.get("continue").is_none());
    }
    assert_eq!(f.run(stop, Decision::Allow).await.stdout, "{}");
    assert_eq!(binding.status().unwrap(), SessionStatus::Observed);
    let page = binding.events(0, 100).unwrap();
    assert!(!serde_json::to_string(&page).unwrap().contains("private"));
    assert_eq!(
        page.records
            .iter()
            .filter(|r| r.request.event == HookEvent::CompletionProposed && r.decision.is_none())
            .count(),
        4
    );
    binding.detach().unwrap();
    assert!(matches!(
        f.service.installation_status(&f.installation.id).unwrap(),
        InstallationStatus::Configured { .. }
    ));
    assert_eq!(
        f.run(f.input("SessionEnd"), Decision::Allow).await.stdout,
        "{}"
    );
}

#[tokio::test]
async fn codex_post_tool_is_unknown_outcome_without_output_or_failure_inference() {
    let f = Fixture::new();
    for output in [
        json!("private: exited with status 1"),
        json!({"isError":true,"secret":"private"}),
        Value::Null,
    ] {
        let mut post = f.input("PostToolUse");
        post["tool_name"] = json!("Bash");
        post["tool_use_id"] = json!("call");
        post["tool_input"] = json!({"command":"private command"});
        post["tool_response"] = output;
        let request = decode(&f.installation, post.to_string().as_bytes()).unwrap();
        let AgentEventPayload::Hook {
            phase: HookPhase::AfterTool,
            payload: Some(metadata),
            ..
        } = request.payload
        else {
            panic!("must not fabricate success/failure")
        };
        assert_eq!(
            metadata,
            json!({"call_id":"call","tool_name":"Bash","outcome":"unknown"})
        );
        assert_eq!(f.run(post, Decision::Allow).await.stdout, "{}");
    }
    let page = f.service.events(&f.installation.id, 0, 100).unwrap();
    assert!(page.records.iter().all(|r| r.tool_payload_omitted));
    assert!(!serde_json::to_string(&page).unwrap().contains("private"));
}

#[tokio::test]
async fn codex_validates_scope_turn_identity_nullable_answer_and_owned_configuration() {
    let f = Fixture::new();
    let mut stop = f.input("Stop");
    stop["stop_hook_active"] = json!(false);
    stop["last_assistant_message"] = Value::Null;
    assert!(decode(&f.installation, stop.to_string().as_bytes())
        .unwrap()
        .final_answer
        .is_none());
    for (field, value) in [
        ("turn_id", Value::Null),
        ("stop_hook_active", json!("false")),
        ("last_assistant_message", json!({})),
        ("cwd", json!(f._dir.path())),
        ("hook_event_name", json!("Interrupt")),
    ] {
        let mut invalid = stop.clone();
        invalid[field] = value;
        assert_eq!(f.run(invalid, Decision::Allow).await.exit_code, 2);
    }
    let path = &f.installation.config_path;
    let mut config: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    config["hooks"]["Stop"][0]["hooks"][0]["command"] = json!("changed");
    std::fs::write(path, config.to_string()).unwrap();
    assert_eq!(f.run(stop, Decision::Allow).await.exit_code, 2);
    let page = f.service.events(&f.installation.id, 0, 100).unwrap();
    assert_eq!(page.records.len(), 1);
    assert!(page.records[0].decision.is_none());
}

#[test]
fn codex_adapter_fixtures_do_not_qualify_native_enforcement_or_delivery() {
    let f = Fixture::new();
    let report = f.service.capabilities("codex", SessionMode::Print).unwrap();
    report
        .require(&[
            IntegrationCapability::HookInstallation,
            IntegrationCapability::DurableReplay,
            IntegrationCapability::ObservationBinding,
            IntegrationCapability::SafeDetach,
        ])
        .unwrap();
    let unmet = report
        .require(&[
            IntegrationCapability::CompletionDecision,
            IntegrationCapability::FailedCompletionObservation,
            IntegrationCapability::MessageSubmission,
        ])
        .unwrap_err();
    assert_eq!(unmet.unmet[0].support, Support::Unknown);
    assert_eq!(unmet.unmet[1].support, Support::Unsupported);
    assert_eq!(unmet.unmet[2].support, Support::Unsupported);
}
