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

#[tokio::test]
async fn binding_replay_is_scoped_and_detach_leaves_native_hooks_and_history_intact() {
    use crate::integration::SessionStatus;
    let f = Fixture::new(10);
    assert!(matches!(
        f.service.observed_session(&f.id, "native-session"),
        Err(IntegrationError::NotFound)
    ));
    f.service
        .handle_hook(
            &f.id,
            f.input("SessionStart").to_string().as_bytes(),
            &Never,
        )
        .await;
    let reference = f.service.observed_session(&f.id, "native-session").unwrap();
    let binding = f.service.bind_existing(&reference).unwrap();
    assert_eq!(binding.status().unwrap(), SessionStatus::Observed);
    assert_eq!(
        f.service.bind_existing(&reference).unwrap().id(),
        binding.id()
    );
    let reopen = IntegrationService::open(f._dir.path().join("state")).unwrap();
    let reopened = reopen.binding(binding.id()).unwrap();
    let mut other = f.input("SessionStart");
    other["session_id"] = json!("another-session");
    f.service
        .handle_hook(&f.id, other.to_string().as_bytes(), &Never)
        .await;
    let first = reopened.events(0, 1).unwrap();
    assert_eq!(first.records.len(), 1);
    let skipped = reopened.events(first.next_cursor, 1).unwrap();
    assert!(skipped.records.is_empty());
    assert!(skipped.next_cursor > first.next_cursor);
    let config = std::fs::read(f.workspace.join(".claude/settings.local.json")).unwrap();
    reopened.detach().unwrap();
    reopened.detach().unwrap();
    assert_eq!(binding.status().unwrap(), SessionStatus::Detached);
    assert!(matches!(
        binding.events(0, 10),
        Err(IntegrationError::Detached)
    ));
    assert_eq!(
        std::fs::read(f.workspace.join(".claude/settings.local.json")).unwrap(),
        config
    );
    // The detached application subscription has no power to disable native hooks.
    assert_eq!(
        f.service
            .handle_hook(
                &f.id,
                f.input("Stop").to_string().as_bytes(),
                &Fixed(Decision::Allow)
            )
            .await
            .exit_code,
        0
    );
    assert_eq!(f.service.events(&f.id, 0, 10).unwrap().records.len(), 4);
    assert_ne!(
        f.service.bind_existing(&reference).unwrap().id(),
        binding.id()
    );
}

#[tokio::test]
async fn ended_sessions_stay_readable_and_resuming_the_same_native_id_invalidates_old_bindings() {
    use crate::integration::SessionStatus;
    let f = Fixture::new(10);
    f.service
        .handle_hook(
            &f.id,
            f.input("SessionStart").to_string().as_bytes(),
            &Never,
        )
        .await;
    let reference = f.service.observed_session(&f.id, "native-session").unwrap();
    let binding = f.service.bind_existing(&reference).unwrap();
    f.service
        .handle_hook(&f.id, f.input("SessionEnd").to_string().as_bytes(), &Never)
        .await;
    assert_eq!(binding.status().unwrap(), SessionStatus::Ended);
    assert_eq!(binding.events(0, 10).unwrap().records.len(), 2);
    assert!(matches!(
        f.service.bind_existing(&reference),
        Err(IntegrationError::SessionEnded)
    ));
    f.service
        .handle_hook(
            &f.id,
            f.input("SessionStart").to_string().as_bytes(),
            &Never,
        )
        .await;
    assert_eq!(binding.status().unwrap(), SessionStatus::Stale);
    assert!(matches!(
        binding.events(0, 10),
        Err(IntegrationError::StaleSession)
    ));
    let resumed = f.service.observed_session(&f.id, "native-session").unwrap();
    assert_ne!(resumed.start_cursor, reference.start_cursor);
    assert_eq!(
        f.service
            .bind_existing(&resumed)
            .unwrap()
            .events(0, 10)
            .unwrap()
            .records
            .len(),
        1
    );
    let mut forged = resumed.clone();
    forged.native_session_id = "unobserved".into();
    assert!(matches!(
        f.service.bind_existing(&forged),
        Err(IntegrationError::StaleSession)
    ));
}

#[tokio::test]
async fn reinstalling_identical_settings_requires_new_start_evidence_and_invalidates_bindings() {
    use crate::integration::SessionStatus;
    let f = Fixture::new(10);
    f.service
        .handle_hook(
            &f.id,
            f.input("SessionStart").to_string().as_bytes(),
            &Never,
        )
        .await;
    let reference = f.service.observed_session(&f.id, "native-session").unwrap();
    let binding = f.service.bind_existing(&reference).unwrap();
    let spec = f.service.installed(&f.id).unwrap().spec;
    let remove = f.service.plan_remove(&f.id).unwrap();
    f.service.apply_install(&remove.id).unwrap();
    let install = f.service.plan_install(spec).unwrap();
    f.service.apply_install(&install.id).unwrap();
    assert_eq!(binding.status().unwrap(), SessionStatus::Stale);
    assert!(matches!(
        f.service.observed_session(&f.id, "native-session"),
        Err(IntegrationError::NotFound)
    ));
    f.service
        .handle_hook(
            &f.id,
            f.input("SessionStart").to_string().as_bytes(),
            &Never,
        )
        .await;
    let new = f.service.observed_session(&f.id, "native-session").unwrap();
    assert_ne!(new.installation_revision, reference.installation_revision);
}

#[tokio::test]
async fn migration_keeps_old_events_readable_without_inventing_current_session_identity() {
    let f = Fixture::new(10);
    f.service
        .handle_hook(
            &f.id,
            f.input("SessionStart").to_string().as_bytes(),
            &Never,
        )
        .await;
    let connection = f.service.connection().unwrap();
    connection.execute_batch("ALTER TABLE hook_invocations DROP COLUMN installation_revision; DROP TABLE session_bindings; DROP TABLE installation_revisions; PRAGMA user_version=2;").unwrap();
    drop(connection);
    let upgraded = IntegrationService::open(f._dir.path().join("state")).unwrap();
    let events = upgraded.events(&f.id, 0, 10).unwrap();
    assert_eq!(events.records.len(), 1);
    assert!(events.records[0].installation_revision.is_none());
    assert!(matches!(
        upgraded.observed_session(&f.id, "native-session"),
        Err(IntegrationError::NotFound)
    ));
    assert_eq!(
        upgraded
            .handle_hook(
                &f.id,
                f.input("SessionStart").to_string().as_bytes(),
                &Never
            )
            .await
            .exit_code,
        0
    );
    let reference = upgraded.observed_session(&f.id, "native-session").unwrap();
    assert!(upgraded.bind_existing(&reference).is_ok());
}

struct ReconfigureDuringDecision<'a>(&'a IntegrationService);
impl HookHandler for ReconfigureDuringDecision<'_> {
    fn decide<'a>(&'a self, request: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(async move {
            let spec = self.0.installed(&request.installation_id).unwrap().spec;
            let plan = self.0.plan_install(spec).unwrap();
            self.0.apply_install(&plan.id).unwrap();
            Ok(Decision::Allow)
        })
    }
}

#[tokio::test]
async fn reapplying_identical_hook_settings_during_a_callback_cannot_deliver_the_old_allow() {
    let f = Fixture::new(10);
    let response = f
        .service
        .handle_hook(
            &f.id,
            f.input("Stop").to_string().as_bytes(),
            &ReconfigureDuringDecision(&f.service),
        )
        .await;
    assert_eq!(response.exit_code, 2);
    assert!(response.stdout.is_empty());
    let journal = f.service.events(&f.id, 0, 10).unwrap();
    assert_eq!(journal.records.len(), 1);
    assert!(journal.records[0].decision.is_none());
}
