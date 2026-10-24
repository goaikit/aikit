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

#[test]
fn native_configuration_check_waits_for_short_local_writer_then_checks_fresh_receipt() {
    let f = Fixture::new(10);
    let installed = f.service.installed(&f.id).unwrap();
    let held = f.service.state_lock().unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(40));
        drop(held);
    });
    f.service.require_configured(&installed).unwrap();
    release.join().unwrap();
}

#[test]
fn native_configuration_change_while_waiting_is_not_accepted() {
    let f = Fixture::new(10);
    let installed = f.service.installed(&f.id).unwrap();
    let held = f.service.state_lock().unwrap();
    let config = installed.config_path.clone();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(40));
        let mut value: Value = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
        value["disableAllHooks"] = json!(true);
        std::fs::write(config, value.to_string()).unwrap();
        drop(held);
    });
    assert!(f.service.require_configured(&installed).is_err());
    release.join().unwrap();
}

#[test]
fn native_configuration_lock_budget_exhaustion_does_not_change_configuration() {
    let f = Fixture::new(10);
    let installed = f.service.installed(&f.id).unwrap();
    let held = f.service.state_lock().unwrap();
    assert!(f.service.require_configured(&installed).is_err());
    drop(held);
    f.service.require_configured(&installed).unwrap();
}
impl Fixture {
    fn owned_module(&self) {
        let plan = self
            .service
            .plan_agent_tool(
                &self.id,
                super::super::AgentToolSpec {
                    application_id: "example".into(),
                    installation_id: self.id.clone(),
                    tool_name: "review".into(),
                    description: "Scoped fixture".into(),
                    input_schema: json!({"type":"object"}),
                    instructions: "Fixture".into(),
                    handler: HookCommand {
                        executable: std::env::current_exe().unwrap(),
                        arguments: vec![],
                    },
                    timeout_ms: 1000,
                    native_completion_bridge: false,
                },
            )
            .unwrap();
        self.service.apply_install(&plan.id).unwrap();
    }

    async fn owned_hook(
        &self,
        input: Value,
        invocation: Option<&str>,
    ) -> Result<HookResponse, IntegrationError> {
        let context = invocation.map(|invocation| {
            json!({"schema_version":1,
            "installation_id":self.id,"native_context":{"session_id":"native-session",
            "invocation_id":invocation,"cwd":self.workspace}})
            .to_string()
        });
        self.service
            .dispatch_hook_with_environment(
                &self.id,
                input.to_string().as_bytes(),
                &Fixed(Decision::Allow),
                |_| context,
            )
            .await
    }

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
                    HookEvent::CompletionFailed,
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

#[tokio::test]
async fn owned_claude_compaction_preserves_current_reference_without_a_synthetic_start() {
    let f = Fixture::new(10);
    f.owned_module();
    let epoch = uuid::Uuid::new_v4().to_string();
    f.owned_hook(f.input("SessionStart"), Some(&epoch))
        .await
        .unwrap();
    let reference = f.service.observed_session(&f.id, "native-session").unwrap();
    let binding = f.service.bind_existing(&reference).unwrap();
    let before = f.service.events(&f.id, 0, 50).unwrap();
    let mut compact = f.input("SessionStart");
    compact["source"] = json!("compact");
    for _ in 0..2 {
        let response = f.owned_hook(compact.clone(), Some(&epoch)).await.unwrap();
        assert_eq!(response.exit_code, 0);
        assert!(response.stdout.is_empty());
        assert!(response.request_id.is_none());
        let after = f.service.events(&f.id, 0, 50).unwrap();
        assert_eq!(after.next_cursor, before.next_cursor);
        assert_eq!(after.records.len(), 1);
        assert_eq!(
            f.service.observed_session(&f.id, "native-session").unwrap(),
            reference
        );
        assert_eq!(
            binding.status().unwrap(),
            super::super::SessionStatus::Observed
        );
    }
    for source in ["startup", "resume", "clear", "fork", "unknown"] {
        let mut input = f.input("SessionStart");
        input["source"] = json!(source);
        assert!(matches!(
            f.owned_hook(input, Some(&epoch)).await,
            Err(IntegrationError::StaleSession)
        ));
    }
}

#[tokio::test]
async fn owned_compaction_cannot_bootstrap_missing_foreign_replaced_or_ended_context() {
    let f = Fixture::new(10);
    f.owned_module();
    let first = uuid::Uuid::new_v4().to_string();
    let second = uuid::Uuid::new_v4().to_string();
    let mut compact = f.input("SessionStart");
    compact["source"] = json!("compact");
    assert!(f.owned_hook(compact.clone(), None).await.is_err());
    assert!(matches!(
        f.owned_hook(compact.clone(), Some(&first)).await,
        Err(IntegrationError::StaleSession)
    ));
    assert!(f.service.events(&f.id, 0, 50).unwrap().records.is_empty());
    f.owned_hook(f.input("SessionStart"), Some(&first))
        .await
        .unwrap();
    assert!(matches!(
        f.owned_hook(compact.clone(), Some(&second)).await,
        Err(IntegrationError::StaleSession)
    ));
    let mut foreign = compact.clone();
    foreign["session_id"] = json!("foreign");
    assert!(f.owned_hook(foreign, Some(&first)).await.is_err());
    f.owned_hook(f.input("SessionStart"), Some(&second))
        .await
        .unwrap();
    assert!(matches!(
        f.owned_hook(compact.clone(), Some(&first)).await,
        Err(IntegrationError::StaleSession)
    ));
    f.owned_hook(f.input("SessionEnd"), Some(&second))
        .await
        .unwrap();
    let cursor = f.service.events(&f.id, 0, 50).unwrap().next_cursor;
    assert!(matches!(
        f.owned_hook(compact, Some(&second)).await,
        Err(IntegrationError::SessionEnded)
    ));
    assert_eq!(f.service.events(&f.id, 0, 50).unwrap().next_cursor, cursor);
}

#[tokio::test]
async fn compaction_rechecks_owned_configuration_and_keeps_legacy_start_semantics() {
    let legacy = Fixture::new(10);
    let mut compact = legacy.input("SessionStart");
    compact["source"] = json!("compact");
    let response = legacy
        .service
        .handle_hook(
            &legacy.id,
            compact.to_string().as_bytes(),
            &Fixed(Decision::Allow),
        )
        .await;
    assert_eq!(response.exit_code, 0);
    assert_eq!(
        legacy
            .service
            .events(&legacy.id, 0, 50)
            .unwrap()
            .records
            .len(),
        1
    );
    let f = Fixture::new(10);
    f.owned_module();
    let epoch = uuid::Uuid::new_v4().to_string();
    f.owned_hook(f.input("SessionStart"), Some(&epoch))
        .await
        .unwrap();
    let path = f.service.installed(&f.id).unwrap().config_path;
    let mut config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["disableAllHooks"] = json!(true);
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut input = f.input("SessionStart");
    input["source"] = json!("compact");
    assert!(f.owned_hook(input, Some(&epoch)).await.is_err());
    assert_eq!(f.service.events(&f.id, 0, 50).unwrap().records.len(), 1);
}

struct Fixed(Decision);

struct CommitWriter {
    database: PathBuf,
    hold: Duration,
    disable_config: Option<PathBuf>,
    replace_revision: bool,
    calls: AtomicUsize,
    worker: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}
impl HookHandler for CommitWriter {
    fn decide<'a>(&'a self, _: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let database = self.database.clone();
            let hold = self.hold;
            let disable_config = self.disable_config.clone();
            let replace_revision = self.replace_revision;
            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let db = rusqlite::Connection::open(database).unwrap();
                db.busy_timeout(Duration::from_secs(1)).unwrap();
                db.execute_batch("BEGIN IMMEDIATE").unwrap();
                ready_tx.send(()).unwrap();
                if let Some(config) = disable_config {
                    std::thread::sleep(Duration::from_millis(150));
                    let mut value: Value =
                        serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
                    value["disableAllHooks"] = json!(true);
                    std::fs::write(config, serde_json::to_vec(&value).unwrap()).unwrap();
                    std::thread::sleep(hold.saturating_sub(Duration::from_millis(150)));
                } else {
                    std::thread::sleep(hold);
                }
                if replace_revision {
                    db.execute_batch("UPDATE installation_revisions SET revision='11111111-1111-4111-8111-111111111111'; COMMIT;").unwrap();
                } else {
                    db.execute_batch("ROLLBACK").unwrap();
                }
            });
            *self.worker.lock().unwrap() = Some(worker);
            ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            Ok(Decision::Allow)
        })
    }
}

#[tokio::test]
async fn short_journal_writer_after_policy_does_not_discard_a_valid_decision() {
    let f = Fixture::new(10);
    let handler = CommitWriter {
        database: f._dir.path().join("state/integration.db"),
        hold: Duration::from_millis(300),
        disable_config: None,
        replace_revision: false,
        calls: AtomicUsize::new(0),
        worker: std::sync::Mutex::new(None),
    };
    let response = f
        .service
        .handle_hook(&f.id, f.input("Stop").to_string().as_bytes(), &handler)
        .await;
    handler
        .worker
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    assert_eq!(response.exit_code, 0, "{}", response.stderr);
    assert_eq!(response.stdout, "{}");
    let events = f.service.events(&f.id, 0, 100).unwrap().records;
    assert_eq!(events.len(), 2);
    assert!(events[0].decision.is_none());
    assert!(matches!(events[1].decision, Some(Decision::Allow)));
    assert_eq!(events[0].request.id, events[1].request.id);
}

#[tokio::test]
async fn expired_journal_wait_never_commits_or_returns_allow() {
    let f = Fixture::new(1);
    let handler = CommitWriter {
        database: f._dir.path().join("state/integration.db"),
        hold: Duration::from_secs(2),
        disable_config: None,
        replace_revision: false,
        calls: AtomicUsize::new(0),
        worker: std::sync::Mutex::new(None),
    };
    let started = Instant::now();
    let response = f
        .service
        .handle_hook(&f.id, f.input("Stop").to_string().as_bytes(), &handler)
        .await;
    let elapsed = started.elapsed();
    handler
        .worker
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    assert_eq!(response.exit_code, 2);
    assert!(response.stdout.is_empty());
    assert!(response.stderr.contains("[state_busy]"));
    assert!(elapsed < Duration::from_millis(1250), "{elapsed:?}");
    let events = f.service.events(&f.id, 0, 100).unwrap().records;
    assert_eq!(events.len(), 1);
    assert!(events[0].decision.is_none());
}

#[tokio::test]
async fn config_disabled_while_waiting_for_decision_writer_never_allows() {
    let f = Fixture::new(10);
    let handler = CommitWriter {
        database: f._dir.path().join("state/integration.db"),
        hold: Duration::from_millis(300),
        disable_config: Some(f.workspace.join(".claude/settings.local.json")),
        replace_revision: false,
        calls: AtomicUsize::new(0),
        worker: std::sync::Mutex::new(None),
    };
    let response = f
        .service
        .handle_hook(&f.id, f.input("Stop").to_string().as_bytes(), &handler)
        .await;
    handler
        .worker
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    assert_eq!(response.exit_code, 2);
    assert!(response.stdout.is_empty());
    assert!(response.stderr.contains("[configuration_conflict]"));
    let events = f.service.events(&f.id, 0, 100).unwrap().records;
    assert_eq!(events.len(), 1);
    assert!(events[0].decision.is_none());
}

#[test]
fn hook_failure_diagnostics_are_bounded_fixed_codes_and_still_fail_closed() {
    let secret = "PRIVATE_CALLER_CONTENT_DO_NOT_PRINT";
    for (error, code) in [
        (
            IntegrationError::Invalid("hook journal deadline expired".into()),
            "request_expired",
        ),
        (
            IntegrationError::Invalid(
                "owned native invocation context is missing or inconsistent".into(),
            ),
            "owned_context_invalid",
        ),
        (
            IntegrationError::Invalid(secret.into()),
            "native_input_invalid",
        ),
        (
            IntegrationError::Conflict(secret.into()),
            "configuration_conflict",
        ),
        (
            IntegrationError::Sql(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                Some(secret.into()),
            )),
            "state_busy",
        ),
        (
            IntegrationError::Sql(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_LOCKED),
                Some(secret.into()),
            )),
            "state_busy",
        ),
        (
            IntegrationError::Sql(rusqlite::Error::InvalidParameterName(secret.into())),
            "state_storage_error",
        ),
        (
            IntegrationError::Io(std::io::Error::other(secret)),
            "local_io_error",
        ),
        (
            IntegrationError::RecoveryRequired(secret.into()),
            "installation_recovery_required",
        ),
        (IntegrationError::NotFound, "record_missing"),
        (IntegrationError::StaleSession, "session_replaced"),
        (IntegrationError::Detached, "binding_detached"),
        (IntegrationError::SessionEnded, "session_ended"),
        (IntegrationError::Unsupported(secret.into()), "unsupported"),
    ] {
        let response = HookResponse::failure(&error);
        assert_eq!(response.exit_code, 2);
        assert!(response.stdout.is_empty());
        assert!(response.request_id.is_none());
        assert!(response.stderr.contains(&format!("[{code}]")));
        assert!(!response.stderr.contains(secret));
        assert!(response.stderr.len() < 256);
    }
}

#[tokio::test]
async fn replacement_during_decision_writer_wait_refuses_even_without_invocation_claims() {
    let f = Fixture::new(10);
    let handler = CommitWriter {
        database: f._dir.path().join("state/integration.db"),
        hold: Duration::from_millis(300),
        disable_config: None,
        replace_revision: true,
        calls: AtomicUsize::new(0),
        worker: std::sync::Mutex::new(None),
    };
    let response = f
        .service
        .handle_hook(&f.id, f.input("Stop").to_string().as_bytes(), &handler)
        .await;
    handler
        .worker
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    assert_eq!(response.exit_code, 2);
    assert!(response.stdout.is_empty());
    assert!(response.stderr.contains("[session_replaced]"));
    let events = f.service.events(&f.id, 0, 100).unwrap().records;
    assert_eq!(events.len(), 1);
    assert!(events[0].decision.is_none());
}

#[tokio::test]
async fn explicit_legacy_upgrade_detachment_fences_old_unbound_starts_and_allows_fresh_start() {
    let f = Fixture::new(10);
    f.service
        .handle_hook(
            &f.id,
            f.input("SessionStart").to_string().as_bytes(),
            &Never,
        )
        .await;
    let first = f.service.observed_session(&f.id, "native-session").unwrap();
    let binding = f.service.bind_existing(&first).unwrap();
    let mut unseen = f.input("SessionStart");
    unseen["session_id"] = json!("unbound");
    f.service
        .handle_hook(&f.id, unseen.to_string().as_bytes(), &Never)
        .await;
    let old_unbound = f.service.observed_session(&f.id, "unbound").unwrap();
    f.service
        .connection()
        .unwrap()
        .execute_batch("DROP TABLE binding_admission_floors; PRAGMA user_version=14")
        .unwrap();
    IntegrationService::upgrade_existing_detaching(
        f._dir.path().join("state"),
        "example",
        "claude",
        &f.workspace,
        &[binding.id().into()],
    )
    .unwrap();
    assert!(matches!(
        f.service.bind_existing(&first),
        Err(IntegrationError::Detached)
    ));
    assert!(matches!(
        f.service.bind_existing(&old_unbound),
        Err(IntegrationError::Detached)
    ));
    let floor: i64 = f
        .service
        .connection()
        .unwrap()
        .query_row(
            "SELECT cursor FROM binding_admission_floors WHERE installation_id=?1",
            [&f.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(floor >= old_unbound.start_cursor as i64);
    f.service
        .handle_hook(&f.id, unseen.to_string().as_bytes(), &Never)
        .await;
    let fresh = f.service.observed_session(&f.id, "unbound").unwrap();
    assert!(fresh.start_cursor > old_unbound.start_cursor);
    f.service.bind_existing(&fresh).unwrap();
    IntegrationService::upgrade_existing_detaching(
        f._dir.path().join("state"),
        "example",
        "claude",
        &f.workspace,
        &[binding.id().into()],
    )
    .unwrap();
    assert_eq!(
        f.service
            .connection()
            .unwrap()
            .query_row::<i64, _, _>(
                "SELECT cursor FROM binding_admission_floors WHERE installation_id=?1",
                [&f.id],
                |r| r.get(0),
            )
            .unwrap(),
        floor
    );
}

#[tokio::test]
async fn scoped_owner_pages_and_detach_require_fresh_start_without_touching_other_handles() {
    let f = Fixture::new(10);
    f.service
        .handle_hook(
            &f.id,
            f.input("SessionStart").to_string().as_bytes(),
            &Never,
        )
        .await;
    let first = f.service.observed_session(&f.id, "native-session").unwrap();
    let a = f.service.bind_existing(&first).unwrap();
    let mut other = f.input("SessionStart");
    other["session_id"] = json!("other-session");
    f.service
        .handle_hook(&f.id, other.to_string().as_bytes(), &Never)
        .await;
    let second = f.service.observed_session(&f.id, "other-session").unwrap();
    let b = f.service.bind_existing(&second).unwrap();
    let mut unseen = f.input("SessionStart");
    unseen["session_id"] = json!("not-yet-bound");
    f.service
        .handle_hook(&f.id, unseen.to_string().as_bytes(), &Never)
        .await;
    let old_unbound = f.service.observed_session(&f.id, "not-yet-bound").unwrap();
    let page = f
        .service
        .attached_bindings("example", "claude", &f.workspace, None, 1)
        .unwrap();
    assert_eq!(page.records.len(), 1);
    let next = f
        .service
        .attached_bindings(
            "example",
            "claude",
            &f.workspace,
            page.next_id.as_deref(),
            1,
        )
        .unwrap();
    assert_eq!(next.records.len(), 1);
    assert_ne!(page.records[0].id, next.records[0].id);
    assert!(f
        .service
        .attached_bindings("other-app", "claude", &f.workspace, None, 50)
        .unwrap()
        .records
        .is_empty());
    assert!(f
        .service
        .detach_recorded_binding("other-app", "claude", &f.workspace, a.id())
        .is_err());
    let config = std::fs::read(f.workspace.join(".claude/settings.local.json")).unwrap();
    let receipt = f
        .service
        .detach_recorded_binding("example", "claude", &f.workspace, a.id())
        .unwrap();
    assert_eq!(a.status().unwrap(), super::super::SessionStatus::Detached);
    assert_eq!(f.service.bind_existing(&second).unwrap().id(), b.id());
    assert!(matches!(
        f.service.bind_existing(&first),
        Err(IntegrationError::Detached)
    ));
    assert!(matches!(
        f.service.bind_existing(&old_unbound),
        Err(IntegrationError::Detached)
    ));
    f.service
        .handle_hook(&f.id, unseen.to_string().as_bytes(), &Never)
        .await;
    let fresh = f.service.observed_session(&f.id, "not-yet-bound").unwrap();
    assert!(fresh.start_cursor > receipt.admission_after_cursor);
    let repeated = f
        .service
        .detach_recorded_binding("example", "claude", &f.workspace, a.id())
        .unwrap();
    assert_eq!(receipt, repeated);
    f.service.bind_existing(&fresh).unwrap();
    assert_eq!(
        std::fs::read(f.workspace.join(".claude/settings.local.json")).unwrap(),
        config
    );
    assert_eq!(
        f.service
            .attached_bindings("example", "claude", &f.workspace, None, 50)
            .unwrap()
            .records
            .len(),
        2
    );
    assert!(f
        .service
        .attached_bindings("example", "claude", &f.workspace, None, 0)
        .is_err());
}

#[test]
fn native_turn_observations_are_typed_bounded_and_match_inherited_turn() {
    let f = Fixture::new(10);
    let installation = f.service.installed(&f.id).unwrap();
    let invocation = uuid::Uuid::new_v4().to_string();
    let environment = json!({"schema_version":1,"installation_id":f.id,
        "native_context":{"session_id":"native-session","invocation_id":invocation,"cwd":f.workspace},
        "native_turn_id":"turn-native"});
    for reason in ["answer", "aborted", "refusal", "error"] {
        let mut value = f.input("AIKitNativeTurnFinished");
        value["aikit_native_turn_version"] = json!(1);
        value["native_turn"] = json!({"id":"turn-native","outcome":{"reason":reason,"is_aborted":reason == "aborted"}});
        value["last_assistant_message"] = json!("Exact answer 雪\n");
        let mut request = decode(&installation, value.to_string().as_bytes()).unwrap();
        assert_eq!(request.event, HookEvent::NativeTurnFinished);
        assert_eq!(request.final_answer.as_deref(), Some("Exact answer 雪\n"));
        super::super::agent_tool_session::bind_environment(
            &installation,
            &mut request,
            Some(&environment.to_string()),
        )
        .unwrap();
        assert_eq!(request.invocation_id.as_deref(), Some(invocation.as_str()));
        assert_eq!(request.native_turn.as_ref().unwrap().id, "turn-native");
        let mut foreign = environment.clone();
        foreign["native_turn_id"] = json!("previous-turn");
        assert!(super::super::agent_tool_session::bind_environment(
            &installation,
            &mut request,
            Some(&foreign.to_string())
        )
        .is_err());
        value["native_turn"]["outcome"]["reason"] = json!("unknown");
        assert!(decode(&installation, value.to_string().as_bytes()).is_err());
    }
    let mut start = f.input("AIKitNativeTurnStarted");
    start["aikit_native_turn_version"] = json!(1);
    start["native_turn"] = json!({"id":"turn-native"});
    assert!(matches!(
        decode(&installation, start.to_string().as_bytes())
            .unwrap()
            .payload,
        AgentEventPayload::Hook {
            phase: HookPhase::RunStart,
            ..
        }
    ));
    assert_eq!(
        decode(&installation, start.to_string().as_bytes())
            .unwrap()
            .event,
        HookEvent::NativeTurnStarted
    );
    start["native_turn"]["outcome"] = json!({"reason":"answer","is_aborted":false});
    assert!(decode(&installation, start.to_string().as_bytes()).is_err());
    start["aikit_native_turn_version"] = json!(2);
    assert!(decode(&installation, start.to_string().as_bytes()).is_err());
    let mut stop = decode(&installation, f.input("Stop").to_string().as_bytes()).unwrap();
    super::super::agent_tool_session::bind_environment(
        &installation,
        &mut stop,
        Some(&environment.to_string()),
    )
    .unwrap();
    assert_eq!(
        stop.native_turn,
        Some(NativeTurn {
            id: "turn-native".into(),
            outcome: None
        })
    );
}

#[tokio::test]
async fn native_turn_without_owned_module_is_refused_before_journal_and_policy() {
    let f = Fixture::new(10);
    let mut value = f.input("AIKitNativeTurnFinished");
    value["aikit_native_turn_version"] = json!(1);
    value["native_turn"] =
        json!({"id":"turn-native","outcome":{"reason":"answer","is_aborted":false}});
    let response = f
        .service
        .handle_hook(&f.id, value.to_string().as_bytes(), &Fixed(Decision::Allow))
        .await;
    assert_eq!(response.exit_code, 2);
    assert!(f.service.events(&f.id, 0, 100).unwrap().records.is_empty());
    let mut spec = f.service.installed(&f.id).unwrap().spec;
    spec.events.push(HookEvent::NativeTurnFinished);
    assert!(f.service.plan_install(spec).is_err());
}

#[test]
fn inherited_tool_generation_is_bounded_scoped_and_never_copied_from_hook_json() {
    let f = Fixture::new(10);
    let installation = f.service.installed(&f.id).unwrap();
    let mut input = f.input("SessionStart");
    input["invocation_id"] = json!("model-cannot-select-this");
    let mut request = decode(&installation, input.to_string().as_bytes()).unwrap();
    assert!(request.invocation_id.is_none());
    let invocation = uuid::Uuid::new_v4().to_string();
    let valid = json!({"schema_version":1,"installation_id":f.id,
        "native_context":{"session_id":"native-session","invocation_id":invocation,"cwd":f.workspace}});
    let bind = |request: &mut HookRequest, value: Option<&str>| {
        super::super::agent_tool_session::bind_environment(&installation, request, value)
    };
    bind(&mut request, Some(&valid.to_string())).unwrap();
    assert_eq!(request.invocation_id.as_deref(), Some(invocation.as_str()));
    assert!(bind(&mut request, None).is_err());
    assert!(bind(&mut request, Some(&"x".repeat(65537))).is_err());
    for (pointer, value) in [
        ("/schema_version", json!(2)),
        ("/installation_id", json!("foreign")),
        ("/native_context/session_id", json!("another-session")),
        ("/native_context/invocation_id", json!("invalid")),
        ("/native_context/cwd", json!(f._dir.path())),
    ] {
        let mut bad = valid.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        let failure = bind(&mut request, Some(&bad.to_string())).unwrap_err();
        assert!(!failure.to_string().contains("another-session"));
    }
}

#[tokio::test]
async fn owned_module_without_inherited_generation_refuses_before_journaling_a_start() {
    let f = Fixture::new(10);
    let plan = f
        .service
        .plan_agent_tool(
            &f.id,
            crate::integration::AgentToolSpec {
                application_id: "example".into(),
                installation_id: f.id.clone(),
                tool_name: "review".into(),
                description: "Selected application operations".into(),
                input_schema: json!({"type":"object"}),
                instructions: "Use selected operations".into(),
                handler: HookCommand {
                    executable: std::env::current_exe().unwrap(),
                    arguments: vec![],
                },
                timeout_ms: 5000,
                native_completion_bridge: false,
            },
        )
        .unwrap();
    f.service.apply_install(&plan.id).unwrap();
    let mut input = f.input("SessionStart");
    input["invocation_id"] = json!(uuid::Uuid::new_v4().to_string());
    let response = f
        .service
        .handle_hook(&f.id, input.to_string().as_bytes(), &Never)
        .await;
    assert_eq!(response.exit_code, 0); // deferred notification, no authority
    input["hook_event_name"] = json!("UserPromptSubmit");
    let response = f
        .service
        .handle_hook(&f.id, input.to_string().as_bytes(), &Never)
        .await;
    assert_ne!(response.exit_code, 0);
    assert!(f.service.events(&f.id, 0, 100).unwrap().records.is_empty());
}

#[tokio::test]
async fn recorded_admission_serializes_journal_detach_and_installation_writers_until_drop() {
    let f = Fixture::new(10);
    assert_eq!(
        f.service
            .handle_hook(
                &f.id,
                f.input("SessionStart").to_string().as_bytes(),
                &Never
            )
            .await
            .exit_code,
        0
    );
    let reference = f.service.observed_session(&f.id, "native-session").unwrap();
    let binding = f.service.bind_existing(&reference).unwrap();
    let cursor = binding.events(0, 100).unwrap().next_cursor;
    let guard = binding.admit_recorded(cursor).unwrap();
    assert!(binding.status().is_err()); // public status also takes the writer lock
    assert!(binding.admit_recorded(cursor).is_err());
    assert!(f.service.plan_remove(&f.id).is_err());
    let writer = f.service.connection().unwrap();
    writer.busy_timeout(std::time::Duration::ZERO).unwrap();
    assert!(writer.execute_batch("BEGIN IMMEDIATE").is_err());
    assert!(writer
        .execute(
            "UPDATE session_bindings SET detached=1 WHERE id=?1",
            [binding.id()]
        )
        .is_err());
    drop(guard);
    assert_eq!(
        f.service
            .handle_hook(
                &f.id,
                f.input("SessionStart").to_string().as_bytes(),
                &Never
            )
            .await
            .exit_code,
        0
    );
    assert!(matches!(
        binding.admit_recorded(cursor),
        Err(IntegrationError::StaleSession)
    ));
    let current = f.service.observed_session(&f.id, "native-session").unwrap();
    let replacement = f.service.bind_existing(&current).unwrap();
    replacement.detach().unwrap();
    assert!(matches!(
        replacement.admit_recorded(cursor),
        Err(IntegrationError::Detached)
    ));
    assert!(f.service.plan_remove(&f.id).is_ok());
}

#[tokio::test]
async fn recorded_admission_refuses_pending_observations_ahead_cursors_and_ended_sessions() {
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
    let cursor = binding.events(0, 100).unwrap().next_cursor;
    assert!(matches!(
        binding.admit_recorded(cursor + 1),
        Err(IntegrationError::Invalid(_))
    ));
    f.service
        .handle_hook(
            &f.id,
            f.input("UserPromptSubmit").to_string().as_bytes(),
            &Fixed(Decision::Allow),
        )
        .await;
    assert!(matches!(
        binding.admit_recorded(cursor),
        Err(IntegrationError::Conflict(_))
    ));
    let projected = binding.events(cursor, 100).unwrap().next_cursor;
    drop(binding.admit_recorded(projected).unwrap());
    f.service
        .handle_hook(&f.id, f.input("SessionEnd").to_string().as_bytes(), &Never)
        .await;
    assert!(matches!(
        binding.admit_recorded(projected),
        Err(IntegrationError::SessionEnded)
    ));
}

#[tokio::test]
async fn recorded_admission_releases_both_existing_locks_on_unwind() {
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
    let cursor = binding.events(0, 100).unwrap().next_cursor;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = binding.admit_recorded(cursor).unwrap();
        panic!("application fixture unwind");
    }));
    assert!(result.is_err());
    drop(binding.admit_recorded(cursor).unwrap());
    binding.detach().unwrap();
    assert!(f.service.plan_remove(&f.id).is_ok());
}

#[tokio::test]
async fn bounded_recorded_admission_waits_only_before_admission_and_preserves_fences() {
    use std::time::{Duration, Instant};
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
    let cursor = binding.events(0, 100).unwrap().next_cursor;
    assert!(matches!(
        binding.admit_recorded_with_wait(cursor, Duration::from_millis(251)),
        Err(IntegrationError::Invalid(_))
    ));
    let held = binding.admit_recorded(cursor).unwrap();
    assert!(binding.admit_recorded(cursor).is_err());
    let started = Instant::now();
    assert!(matches!(
        binding.admit_recorded_with_wait(cursor, Duration::from_millis(30)),
        Err(IntegrationError::Conflict(_))
    ));
    assert!(started.elapsed() >= Duration::from_millis(25));
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        drop(held);
    });
    let admitted = binding
        .admit_recorded_with_wait(cursor, Duration::from_millis(250))
        .unwrap();
    release.join().unwrap();
    assert!(binding.admit_recorded(cursor).is_err());
    drop(admitted);
    assert_eq!(binding.events(0, 100).unwrap().next_cursor, cursor);
    f.service
        .handle_hook(
            &f.id,
            f.input("UserPromptSubmit").to_string().as_bytes(),
            &Fixed(Decision::Allow),
        )
        .await;
    assert!(matches!(
        binding.admit_recorded_with_wait(cursor, Duration::from_millis(250)),
        Err(IntegrationError::Conflict(_))
    ));
    let projected = binding.events(cursor, 100).unwrap().next_cursor;
    drop(
        binding
            .admit_recorded_with_wait(projected, Duration::from_millis(250))
            .unwrap(),
    );
    binding.detach().unwrap();
    assert!(matches!(
        binding.admit_recorded_with_wait(projected, Duration::from_millis(250)),
        Err(IntegrationError::Detached)
    ));
}

#[tokio::test]
async fn status_waits_for_a_short_writer_but_admission_still_refuses_immediately() {
    use crate::integration::SessionStatus;
    let f = Fixture::new(10);
    assert_eq!(
        f.service
            .handle_hook(
                &f.id,
                f.input("SessionStart").to_string().as_bytes(),
                &Never
            )
            .await
            .exit_code,
        0
    );
    let reference = f.service.observed_session(&f.id, "native-session").unwrap();
    let binding = f.service.bind_existing(&reference).unwrap();
    let cursor = binding.events(0, 100).unwrap().next_cursor;
    let guard = binding.admit_recorded(cursor).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (status_tx, status_rx) = std::sync::mpsc::channel();
    let (admission_tx, admission_rx) = std::sync::mpsc::channel();
    let service = IntegrationService::open_existing(f._dir.path().join("state")).unwrap();
    let id = binding.id().to_owned();
    let worker = std::thread::spawn(move || {
        let binding = service.binding(&id).unwrap();
        admission_tx
            .send(binding.admit_recorded(cursor).is_err())
            .unwrap();
        started_tx.send(()).unwrap();
        status_tx.send(binding.status()).unwrap();
    });
    assert!(admission_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap());
    started_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    let early = status_rx.recv_timeout(std::time::Duration::from_millis(30));
    drop(guard);
    assert!(
        matches!(early, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
        "status must wait for the occupied writer"
    );
    assert_eq!(
        status_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
            .unwrap(),
        SessionStatus::Observed
    );
    worker.join().unwrap();

    let guard = binding.admit_recorded(cursor).unwrap();
    let started = std::time::Instant::now();
    assert!(matches!(
        binding.status(),
        Err(IntegrationError::Conflict(_))
    ));
    assert!(started.elapsed() >= std::time::Duration::from_millis(200));
    drop(guard);
    binding.detach().unwrap();
    assert_eq!(binding.status().unwrap(), SessionStatus::Detached);
}

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
async fn failed_completion_is_observation_only_and_keeps_the_session_binding_open() {
    use crate::integration::SessionStatus;
    let f = Fixture::new(10);
    let settings: Value = serde_json::from_slice(
        &std::fs::read(f.workspace.join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert!(settings["hooks"]["StopFailure"].is_array());
    f.service
        .handle_hook(
            &f.id,
            f.input("SessionStart").to_string().as_bytes(),
            &Never,
        )
        .await;
    let reference = f.service.observed_session(&f.id, "native-session").unwrap();
    let binding = f.service.bind_existing(&reference).unwrap();
    let mut input = f.input("StopFailure");
    input["error"] = json!("rate_limit");
    input["error_details"] = json!("private diagnostic");
    input["last_assistant_message"] = json!("API Error: private diagnostic");
    let response = f
        .service
        .handle_hook(&f.id, input.to_string().as_bytes(), &Never)
        .await;
    assert_eq!(response.exit_code, 0);
    assert_eq!(response.stdout, "{}");
    assert_eq!(binding.status().unwrap(), SessionStatus::Observed);
    let page = binding.events(0, 10).unwrap();
    assert_eq!(page.records.len(), 2);
    let failure = &page.records[1];
    assert_eq!(failure.request.event, HookEvent::CompletionFailed);
    assert!(failure.decision.is_none());
    assert!(failure.request.final_answer.is_none());
    assert!(!matches!(
        failure.request.payload,
        AgentEventPayload::Terminal { .. }
    ));
    assert!(!serde_json::to_string(&page)
        .unwrap()
        .contains("private diagnostic"));
    let reopened = IntegrationService::open(f._dir.path().join("state")).unwrap();
    let replay = reopened
        .binding(binding.id())
        .unwrap()
        .events(page.records[0].cursor, 10)
        .unwrap();
    assert_eq!(replay.records.len(), 1);
    assert_eq!(replay.records[0].cursor, failure.cursor);
    assert_eq!(replay.records[0].request.event, HookEvent::CompletionFailed);
}

#[tokio::test]
async fn required_native_contracts_are_checked_before_creating_a_binding() {
    use crate::integration::{IntegrationCapability, SessionMode};
    let f = Fixture::new(10);
    f.service
        .handle_hook(
            &f.id,
            f.input("SessionStart").to_string().as_bytes(),
            &Never,
        )
        .await;
    let reference = f.service.observed_session(&f.id, "native-session").unwrap();
    let result = f.service.bind_existing_requiring(
        &reference,
        SessionMode::Unknown,
        &[IntegrationCapability::SuccessfulCompletionObservation],
    );
    assert!(matches!(
        result,
        Err(IntegrationError::RequirementsUnmet(_))
    ));
    let count: i64 = f
        .service
        .connection()
        .unwrap()
        .query_row("SELECT count(*) FROM session_bindings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let binding = f
        .service
        .bind_existing_requiring(
            &reference,
            SessionMode::Unknown,
            &[
                IntegrationCapability::ObservationBinding,
                IntegrationCapability::DurableReplay,
            ],
        )
        .unwrap();
    assert_eq!(binding.reference(), &reference);
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
    assert!(matches!(
        f.service.bind_existing(&reference),
        Err(IntegrationError::Detached)
    ));
    f.service
        .handle_hook(
            &f.id,
            f.input("SessionStart").to_string().as_bytes(),
            &Never,
        )
        .await;
    let fresh = f.service.observed_session(&f.id, "native-session").unwrap();
    assert_ne!(fresh, reference);
    assert_ne!(f.service.bind_existing(&fresh).unwrap().id(), binding.id());
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

struct EditConfigDuringDecision {
    path: PathBuf,
    document: Value,
    calls: AtomicUsize,
}
impl HookHandler for EditConfigDuringDecision {
    fn decide<'a>(&'a self, _: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::fs::write(&self.path, self.document.to_string()).unwrap();
            Ok(Decision::Allow)
        })
    }
}

#[tokio::test]
async fn local_hook_drift_blocks_before_policy_without_erasing_observations() {
    let f = Fixture::new(10);
    let path = f.service.installed(&f.id).unwrap().config_path;
    let original: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let mut disabled = original.clone();
    disabled["disableAllHooks"] = json!(true);
    let mut changed = original.clone();
    changed["hooks"]["Stop"][0]["hooks"][0]["timeout"] = json!(99);
    let handler = EditConfigDuringDecision {
        path: path.clone(),
        document: original.clone(),
        calls: AtomicUsize::new(0),
    };
    for document in [disabled, changed, json!({})] {
        std::fs::write(&path, document.to_string()).unwrap();
        let response = f
            .service
            .handle_hook(&f.id, f.input("Stop").to_string().as_bytes(), &handler)
            .await;
        assert_eq!(response.exit_code, 2);
        assert!(response.stdout.is_empty());
        assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap(),
            document
        );
    }
    let page = f.service.events(&f.id, 0, 20).unwrap();
    assert_eq!(page.records.len(), 3);
    assert!(page.records.iter().all(|record| record.decision.is_none()));
    // A delivered failure notification still belongs in recovery history even
    // when the local hooks have drifted. It invokes no application decision.
    assert_eq!(
        f.service
            .handle_hook(&f.id, f.input("StopFailure").to_string().as_bytes(), &Never)
            .await
            .exit_code,
        0
    );
    assert_eq!(
        f.service.events(&f.id, 3, 20).unwrap().records[0]
            .request
            .event,
        HookEvent::CompletionFailed
    );
}

#[tokio::test]
async fn a_callback_cannot_allow_after_disabling_its_owned_hooks() {
    let f = Fixture::new(10);
    let path = f.service.installed(&f.id).unwrap().config_path;
    let mut document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    document["disableAllHooks"] = json!(true);
    let handler = EditConfigDuringDecision {
        path,
        document,
        calls: AtomicUsize::new(0),
    };
    let response = f
        .service
        .handle_hook(&f.id, f.input("Stop").to_string().as_bytes(), &handler)
        .await;
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    assert_eq!(response.exit_code, 2);
    assert!(response.stdout.is_empty());
    let page = f.service.events(&f.id, 0, 10).unwrap();
    assert_eq!(page.records.len(), 1);
    assert!(page.records[0].decision.is_none());
}

#[tokio::test]
async fn unrelated_settings_edits_during_policy_do_not_invalidate_owned_hooks() {
    let f = Fixture::new(10);
    let path = f.service.installed(&f.id).unwrap().config_path;
    let mut document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    document["env"] = json!({"APPLICATION_FIXTURE":"preserved"});
    let handler = EditConfigDuringDecision {
        path,
        document,
        calls: AtomicUsize::new(0),
    };
    let response = f
        .service
        .handle_hook(&f.id, f.input("Stop").to_string().as_bytes(), &handler)
        .await;
    assert_eq!(response.exit_code, 0);
    assert_eq!(response.stdout, "{}");
    assert_eq!(
        f.service.events(&f.id, 0, 10).unwrap().records[1].decision,
        Some(Decision::Allow)
    );
}
