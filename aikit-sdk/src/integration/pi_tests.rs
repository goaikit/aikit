use super::*;
use crate::integration::{
    DecisionFuture, HookCommand, HookHandler, InstallSpec, InstallationStatus,
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
                application_id: "pi-fixture".into(),
                agent_key: "pi".into(),
                workspace,
                handler: HookCommand {
                    executable: std::env::current_exe().unwrap(),
                    arguments: vec!["hook-pi".into()],
                },
                events: vec![
                    HookEvent::SessionStarted,
                    HookEvent::InputSubmitted,
                    HookEvent::BeforeTool,
                    HookEvent::AfterTool,
                    HookEvent::ToolFailed,
                    HookEvent::CompletionProposed,
                    HookEvent::CompletionFailed,
                    HookEvent::SessionEnded,
                ],
                timeout_seconds: 2,
            })
            .unwrap();
        assert!(!plan.config_path.exists());
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
        json!({"aikit_hook_version":1,"hook_event_name":event,"session_id":"session","cwd":self.installation.spec.workspace})
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
fn pi_owned_extension_plans_preserve_unrelated_files_and_refuse_drift_or_adoption() {
    let f = Fixture::new();
    let path = &f.installation.config_path;
    let original = std::fs::read(path).unwrap();
    let other = path.with_file_name("other.js");
    std::fs::write(&other, "unrelated extension").unwrap();
    let plan = f.service.plan_install(f.installation.spec.clone()).unwrap();
    std::fs::write(path, "owner edited this file").unwrap();
    assert!(matches!(
        f.service.apply_install(&plan.id),
        Err(IntegrationError::Conflict(_))
    ));
    assert!(matches!(
        f.service.installation_status(&f.installation.id).unwrap(),
        InstallationStatus::Drifted { .. }
    ));
    assert!(f.service.plan_remove(&f.installation.id).is_err());
    assert!(f.service.plan_install(f.installation.spec.clone()).is_err());
    std::fs::write(path, &original).unwrap();
    f.service.apply_install(&plan.id).unwrap();
    let removal = f.service.plan_remove(&f.installation.id).unwrap();
    assert!(path.exists());
    assert!(matches!(
        f.service.apply_install(&removal.id).unwrap(),
        InstallationStatus::Absent
    ));
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_to_string(other).unwrap(),
        "unrelated extension"
    );
    std::fs::write(path, &original).unwrap();
    assert!(matches!(
        f.service.plan_install(f.installation.spec.clone()),
        Err(IntegrationError::Conflict(_))
    ));
}

#[tokio::test]
async fn pi_decisions_tool_outcomes_failure_and_replay_preserve_shared_contracts() {
    let f = Fixture::new();
    assert_eq!(
        f.run(f.input("session_start"), Decision::Allow)
            .await
            .stdout,
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
    assert_eq!(
        serde_json::from_str::<Value>(&f.run(f.input("input"), deny.clone()).await.stdout).unwrap(),
        json!({"decision":"block","reason":"review pending"})
    );
    let mut tool = f.input("tool_call");
    tool["tool_name"] = json!("write");
    tool["tool_use_id"] = json!("call");
    tool["tool_input"] = json!({"content":"private content"});
    assert_eq!(
        f.run(tool, Decision::Allow).await.stdout,
        "{\"decision\":\"allow\"}"
    );
    for is_error in [false, true] {
        let mut result = f.input("tool_result");
        result["tool_use_id"] = json!("call");
        result["is_error"] = json!(is_error);
        result["tool_response"] = json!("private output");
        assert_eq!(f.run(result, Decision::Allow).await.stdout, "{}");
    }
    let mut stop = f.input("agent_before_settle");
    stop["outcome"] = json!("completed");
    stop["last_assistant_message"] = json!("answer");
    stop["stop_hook_active"] = json!(true);
    assert_eq!(
        serde_json::from_str::<Value>(&f.run(stop, deny).await.stdout).unwrap()["decision"],
        "block"
    );
    let mut failure = f.input("agent_settled");
    failure["outcome"] = json!("error");
    failure["last_assistant_message"] = json!("private error");
    assert_eq!(f.run(failure, Decision::Allow).await.stdout, "{}");
    assert_eq!(binding.status().unwrap(), SessionStatus::Observed);
    let page = binding.events(0, 100).unwrap();
    assert!(!serde_json::to_string(&page).unwrap().contains("private"));
    assert!(page
        .records
        .iter()
        .any(|r| r.request.event == HookEvent::ToolFailed));
    assert!(page.records.iter().any(
        |r| r.request.event == HookEvent::CompletionFailed && r.request.final_answer.is_none()
    ));
    assert_eq!(
        f.run(f.input("session_shutdown"), Decision::Allow)
            .await
            .stdout,
        "{}"
    );
    assert_eq!(binding.status().unwrap(), SessionStatus::Ended);
    binding.detach().unwrap();
    assert!(f.installation.config_path.exists());
}

#[tokio::test]
async fn pi_refuses_foreign_scope_wire_versions_ambiguous_results_and_changed_extension() {
    let f = Fixture::new();
    for (field, value) in [
        ("aikit_hook_version", json!(2)),
        ("cwd", json!(f._dir.path())),
        ("session_id", json!("")),
        ("hook_event_name", json!("agent_settled")),
    ] {
        let mut invalid = f.input("input");
        invalid[field] = value;
        assert_eq!(f.run(invalid, Decision::Allow).await.exit_code, 2);
    }
    let mut tool = f.input("tool_result");
    tool["tool_use_id"] = json!("call");
    tool["tool_response"] = json!("result");
    tool["is_error"] = json!("false");
    assert_eq!(f.run(tool, Decision::Allow).await.exit_code, 2);
    let mut completed = f.input("agent_settled");
    completed["outcome"] = json!("completed");
    assert_eq!(f.run(completed, Decision::Allow).await.exit_code, 2);
    std::fs::write(&f.installation.config_path, "changed").unwrap();
    assert_eq!(f.run(f.input("input"), Decision::Allow).await.exit_code, 2);
    let page = f.service.events(&f.installation.id, 0, 100).unwrap();
    assert_eq!(page.records.len(), 1);
    assert!(page.records[0].decision.is_none());
}

#[test]
fn pi_capabilities_do_not_infer_native_qualification_from_generated_extension() {
    let f = Fixture::new();
    let report = f.service.capabilities("pi", SessionMode::Print).unwrap();
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
            IntegrationCapability::SuccessfulCompletionObservation,
        ])
        .unwrap_err();
    assert_eq!(unmet.unmet[0].support, Support::Unknown);
    assert_eq!(unmet.unmet[1].support, Support::Unknown);
    assert_eq!(unmet.unmet[2].support, Support::Unsupported);
}

#[test]
fn generated_pi_extension_executes_real_native_bridge_and_preserves_boundary_semantics() {
    let f = Fixture::new();
    let node = std::process::Command::new("node")
        .args(["-p", "process.execPath"])
        .output()
        .expect("Pi bridge contract tests require Node.js on PATH");
    assert!(node.status.success());
    let executable = std::path::PathBuf::from(String::from_utf8(node.stdout).unwrap().trim());
    let handler = f._dir.path().join("handler ' $ é.mjs");
    std::fs::write(&handler, include_str!("pi_test_handler.mjs")).unwrap();
    let mut installation = f.installation.clone();
    installation.spec.handler = HookCommand {
        executable: executable.clone(),
        arguments: vec![
            handler.to_str().unwrap().into(),
            "".into(),
            "quote\" slash\\ ' $ ; é".into(),
        ],
    };
    installation.spec.timeout_seconds = 1;
    let extension_path = f._dir.path().join("extension.mjs");
    std::fs::write(&extension_path, extension(&installation).unwrap()).unwrap();
    let harness = f._dir.path().join("harness.mjs");
    std::fs::write(&harness, include_str!("pi_bridge_tests.mjs")).unwrap();
    let output = std::process::Command::new(executable)
        .arg(harness)
        .arg(extension_path)
        .arg(&installation.spec.workspace)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "Pi bridge contract passed"
    );
}
