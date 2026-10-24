//! Synthetic journal/guard tests; real native process qualification is separate.
use super::*;
use crate::integration::{
    Decision, DecisionFuture, HookCommand, HookEvent, HookHandler, HookRequest, InstallSpec,
    IntegrationService, MessageRequest, MessageState,
};
use serde_json::json;
struct Allow;
impl HookHandler for Allow {
    fn decide<'a>(&'a self, _: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(async { Ok(Decision::Allow) })
    }
}
struct Fixture {
    dir: tempfile::TempDir,
    sdk: IntegrationService,
    installation: String,
    workspace: std::path::PathBuf,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let sdk = IntegrationService::open(dir.path().join("state")).unwrap();
        let plan = sdk
            .plan_install(InstallSpec {
                application_id: "caller-test".into(),
                agent_key: "pi".into(),
                workspace: workspace.clone(),
                handler: HookCommand {
                    executable: std::env::current_exe().unwrap(),
                    arguments: vec![],
                },
                events: vec![HookEvent::SessionStarted, HookEvent::SessionEnded],
                timeout_seconds: 10,
            })
            .unwrap();
        sdk.apply_install(&plan.id).unwrap();
        let fixture = Self {
            dir,
            sdk,
            installation: plan.installation_id,
            workspace,
        };
        fixture.start("11111111-1111-4111-8111-111111111111").await;
        fixture
    }
    async fn start(&self, generation: &str) {
        let input = json!({"aikit_hook_version":2,"hook_event_name":"session_start","session_id":"original-native","invocation_id":generation,"cwd":self.workspace});
        assert_eq!(
            self.sdk
                .handle_hook(&self.installation, input.to_string().as_bytes(), &Allow)
                .await
                .exit_code,
            0
        );
    }
    fn binding(&self) -> SessionBinding<'_> {
        self.sdk
            .bind_existing(
                &self
                    .sdk
                    .observed_session(&self.installation, "original-native")
                    .unwrap(),
            )
            .unwrap()
    }
    fn cursor(&self) -> u64 {
        self.sdk
            .events(&self.installation, 0, 100)
            .unwrap()
            .next_cursor
    }
}
fn snapshot(binding: &SessionBinding<'_>) -> RecordedLinuxNativeCaller {
    RecordedLinuxNativeCaller {
        schema_version: 1,
        binding_id: binding.id().into(),
        reference: binding.reference().clone(),
        process: LinuxNativeCaller {
            pid: 100,
            uid: 1000,
            start_ticks: 123,
            executable_sha256: "a".repeat(64),
        },
        boot_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
        pid_namespace: 99,
    }
}
#[tokio::test]
async fn association_is_immutable_and_requires_inspected_admission_across_reopen() {
    let f = Fixture::new().await;
    let binding = f.binding();
    let value = snapshot(&binding);
    assert!(binding.recorded_linux_native_caller().unwrap().is_none());
    assert!(binding.admit_inspected_caller(f.cursor(), &value).is_err());
    binding.bind_inspected_caller(value.clone()).unwrap();
    assert_eq!(binding.bind_inspected_caller(value.clone()).unwrap(), value);
    assert!(binding.admit_recorded(f.cursor()).is_err());
    drop(binding.admit_inspected_caller(f.cursor(), &value).unwrap());
    for field in ["pid", "birth", "boot", "namespace", "hash"] {
        let mut changed = value.clone();
        match field {
            "pid" => changed.process.pid += 1,
            "birth" => changed.process.start_ticks += 1,
            "boot" => changed.boot_id = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(),
            "namespace" => changed.pid_namespace += 1,
            _ => changed.process.executable_sha256 = "b".repeat(64),
        }
        assert!(binding.bind_inspected_caller(changed.clone()).is_err());
        assert!(binding
            .admit_inspected_caller(f.cursor(), &changed)
            .is_err());
    }
    let reopened = IntegrationService::open_existing(f.dir.path().join("state")).unwrap();
    assert_eq!(
        reopened
            .binding(binding.id())
            .unwrap()
            .recorded_linux_native_caller()
            .unwrap(),
        Some(value)
    );
}
#[tokio::test]
async fn replacement_and_detachment_never_move_or_reactivate_caller_association() {
    let f = Fixture::new().await;
    let old = f.binding();
    let value = snapshot(&old);
    old.bind_inspected_caller(value.clone()).unwrap();
    f.start("22222222-2222-4222-8222-222222222222").await;
    assert!(old.bind_inspected_caller(value.clone()).is_err());
    assert!(old.admit_inspected_caller(f.cursor(), &value).is_err());
    let new = f.binding();
    assert_ne!(old.id(), new.id());
    assert!(new.recorded_linux_native_caller().unwrap().is_none());
    let fresh = snapshot(&new);
    new.bind_inspected_caller(fresh.clone()).unwrap();
    new.detach().unwrap();
    assert!(new.admit_inspected_caller(f.cursor(), &fresh).is_err());
    assert_eq!(old.recorded_linux_native_caller().unwrap(), Some(value));
    assert_eq!(new.recorded_linux_native_caller().unwrap(), Some(fresh));
}
#[tokio::test]
async fn corrupt_caller_record_cannot_fall_back_to_ordinary_admission() {
    let f = Fixture::new().await;
    let binding = f.binding();
    let value = snapshot(&binding);
    binding.bind_inspected_caller(value.clone()).unwrap();
    f.sdk.connection().unwrap().execute("UPDATE linux_native_callers SET body=json_set(body,'$.reference.native_session_id','foreign') WHERE binding_id=?1",[binding.id()]).unwrap();
    assert!(binding.recorded_linux_native_caller().is_err());
    assert!(binding.admit_recorded(f.cursor()).is_err());
    assert!(binding.admit_inspected_caller(f.cursor(), &value).is_err());
}
#[tokio::test]
async fn schema_15_upgrade_requires_quiescence_preserves_unknown_and_advances_floor() {
    let f = Fixture::new().await;
    let binding = f.binding();
    binding
        .stage_message(MessageRequest {
            operation_id: "uncertain".into(),
            content: "Frozen feedback".into(),
        })
        .unwrap();
    binding.reserve_message("uncertain").unwrap();
    binding.recover_message_record("uncertain").unwrap();
    let before = binding.message_status("uncertain").unwrap();
    assert_eq!(before.state, MessageState::Unknown);
    let connection = f.sdk.connection().unwrap();
    connection
        .execute(
            "INSERT INTO binding_admission_floors(installation_id,cursor) VALUES (?1,0)",
            [&f.installation],
        )
        .unwrap();
    connection
        .execute_batch("DROP TABLE linux_native_callers; PRAGMA user_version=15")
        .unwrap();
    assert!(IntegrationService::open_existing(f.dir.path().join("state")).is_err());
    assert!(IntegrationService::open(f.dir.path().join("state")).is_err());
    assert!(IntegrationService::upgrade_existing(f.dir.path().join("state")).is_err());
    assert_eq!(
        connection
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        15
    );
    let report = IntegrationService::upgrade_existing_detaching(
        f.dir.path().join("state"),
        "caller-test",
        "pi",
        &f.workspace,
        &[binding.id().into()],
    )
    .unwrap();
    assert_eq!(
        (
            report.previous_schema,
            report.current_schema,
            report.changed
        ),
        (15, 16, true)
    );
    let reopened = IntegrationService::open_existing(f.dir.path().join("state")).unwrap();
    let historical = reopened.binding(binding.id()).unwrap();
    assert_eq!(historical.message_status("uncertain").unwrap(), before);
    assert_eq!(historical.status().unwrap(), SessionStatus::Detached);
    assert!(reopened.bind_existing(binding.reference()).is_err());
    assert!(historical.recorded_linux_native_caller().unwrap().is_none());
}
