use super::*;
use crate::integration::{
    DecisionFuture, HookCommand, HookEvent, HookHandler, HookRequest, InstallSpec,
    IntegrationService,
};
use serde_json::json;

struct Never;
impl HookHandler for Never {
    fn decide<'a>(&'a self, _: &'a HookRequest) -> DecisionFuture<'a> {
        Box::pin(async { panic!("notification must not call policy") })
    }
}
struct Fixture {
    dir: tempfile::TempDir,
    service: IntegrationService,
    installation: String,
    workspace: std::path::PathBuf,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let service = IntegrationService::open(dir.path().join("state")).unwrap();
        let plan = service
            .plan_install(InstallSpec {
                application_id: "message-fixture".into(),
                agent_key: "claude".into(),
                workspace: workspace.clone(),
                handler: HookCommand {
                    executable: std::env::current_exe().unwrap(),
                    arguments: vec![],
                },
                events: vec![HookEvent::SessionStarted, HookEvent::SessionEnded],
                timeout_seconds: 10,
            })
            .unwrap();
        service.apply_install(&plan.id).unwrap();
        let fixture = Self {
            dir,
            service,
            installation: plan.installation_id,
            workspace,
        };
        fixture.start(true).await;
        fixture
    }
    async fn start(&self, generation: bool) {
        let input =
            json!({"hook_event_name":"SessionStart","session_id":"session","cwd":self.workspace});
        assert_eq!(
            self.service
                .handle_hook(&self.installation, input.to_string().as_bytes(), &Never)
                .await
                .exit_code,
            0
        );
        if generation {
            // Synthetic store test only: inject an invocation into the fixture
            // journal. This is not a native loader/caller/transport qualification.
            self.service.connection().unwrap().execute("UPDATE hook_invocations SET body=json_set(body,'$.invocation_id',?1) WHERE sequence=(SELECT max(sequence) FROM hook_invocations)",
                [uuid::Uuid::new_v4().to_string()]).unwrap();
        }
    }
    fn binding(&self) -> SessionBinding<'_> {
        self.service
            .bind_existing(
                &self
                    .service
                    .observed_session(&self.installation, "session")
                    .unwrap(),
            )
            .unwrap()
    }
}
fn request(id: &str) -> MessageRequest {
    MessageRequest {
        operation_id: id.into(),
        content: "Exact frozen JSON 雪\n\0".into(),
    }
}
fn observation(id: &str, proof: MessageProof) -> MessageObservation {
    MessageObservation {
        event_id: id.into(),
        source: "synthetic-store-fixture".into(),
        proof,
    }
}
fn ack(id: &str) -> MessageObservation {
    observation(
        id,
        MessageProof::Acknowledgement {
            receipt_reference: "native-receipt-ref".into(),
        },
    )
}

#[tokio::test]
async fn exact_payload_reopens_and_cannot_retarget_a_replacement_binding() {
    let f = Fixture::new().await;
    let binding = f.binding();
    let first = binding.stage_message(request("op")).unwrap();
    assert_eq!(first.state, MessageState::Accepted);
    assert!(first.acknowledgement.is_none());
    assert_eq!(binding.stage_message(request("op")).unwrap(), first);
    let mut changed = request("op");
    changed.content.push('x');
    assert!(matches!(
        binding.stage_message(changed),
        Err(IntegrationError::Conflict(_))
    ));
    let reopened = IntegrationService::open(f.dir.path().join("state")).unwrap();
    assert_eq!(
        reopened
            .binding(binding.id())
            .unwrap()
            .message_status("op")
            .unwrap(),
        first
    );
    f.start(true).await;
    let replacement = f.binding();
    assert!(matches!(
        replacement.stage_message(request("op")),
        Err(IntegrationError::Conflict(_))
    ));
    assert!(matches!(
        replacement.message_status("op"),
        Err(IntegrationError::NotFound)
    ));
    assert!(matches!(
        binding.reserve_message("op"),
        Err(IntegrationError::StaleSession)
    ));
    assert_eq!(binding.message_status("op").unwrap(), first);
}

#[tokio::test]
async fn reserved_crash_becomes_unknown_and_late_evidence_never_creates_a_second_attempt() {
    let f = Fixture::new().await;
    let binding = f.binding();
    binding.stage_message(request("op")).unwrap();
    let reservation = binding.reserve_message("op").unwrap();
    // A second connection can begin a write: no reservation holds SQLite over I/O.
    f.service
        .connection()
        .unwrap()
        .execute_batch("BEGIN IMMEDIATE; ROLLBACK")
        .unwrap();
    let reopened = IntegrationService::open(f.dir.path().join("state")).unwrap();
    let old = reopened.binding(binding.id()).unwrap();
    assert_eq!(
        old.recover_message_record("op").unwrap().state,
        MessageState::Unknown
    );
    assert!(old.reserve_message("op").is_err());
    binding.detach().unwrap();
    f.start(true).await;
    let consumption = observation(
        "input",
        MessageProof::Consumption {
            input_reference: "exact-input-ref".into(),
            event_reference: "native-event-ref".into(),
        },
    );
    let consumed = old
        .record_message_observation(&reservation, consumption.clone())
        .unwrap();
    assert_eq!(consumed.state, MessageState::Acknowledged);
    assert!(consumed.acknowledgement.is_none());
    assert!(consumed.consumption.is_some());
    let complete = old
        .record_message_observation(&reservation, ack("ack"))
        .unwrap();
    assert!(complete.acknowledgement.is_some());
    assert_eq!(
        old.record_message_observation(&reservation, consumption)
            .unwrap(),
        complete
    );
    let first = old.message_observations("op", 0, 1).unwrap();
    let second = old
        .message_observations("op", first.next_cursor, 1)
        .unwrap();
    assert_eq!(first.records.len(), 1);
    assert_eq!(second.records.len(), 1);
    assert!(second.next_cursor > first.next_cursor);
    assert_eq!(
        old.message_observations("op", second.next_cursor, 1)
            .unwrap()
            .records
            .len(),
        0
    );
    assert!(old.reserve_message("op").is_err());
}

#[tokio::test]
async fn wrong_scope_changed_replay_and_contradictory_failure_are_refused() {
    let f = Fixture::new().await;
    let binding = f.binding();
    binding.stage_message(request("op")).unwrap();
    let mut ticket = binding.reserve_message("op").unwrap();
    let actual = ticket.clone();
    ticket.target.native_session_id = "foreign".into();
    assert!(binding
        .record_message_observation(&ticket, ack("ack"))
        .is_err());
    let done = binding
        .record_message_observation(&actual, ack("ack"))
        .unwrap();
    assert!(done.consumption.is_none());
    assert!(binding
        .record_message_observation(
            &actual,
            observation(
                "ack",
                MessageProof::Failure {
                    failure_reference: "changed".into()
                }
            )
        )
        .is_err());
    assert!(binding
        .record_message_observation(
            &actual,
            observation(
                "fail",
                MessageProof::Failure {
                    failure_reference: "failure".into()
                }
            )
        )
        .is_err());
    binding.stage_message(request("other")).unwrap();
    let other = binding.reserve_message("other").unwrap();
    assert!(binding
        .record_message_observation(&other, ack("ack"))
        .is_err()); // event ID cannot migrate to another operation
    let failed = binding
        .record_message_observation(
            &other,
            observation(
                "failed",
                MessageProof::Failure {
                    failure_reference: "explicit-native-failure".into(),
                },
            ),
        )
        .unwrap();
    assert_eq!(failed.state, MessageState::Failed);
    assert!(binding.reserve_message("other").is_err());
    assert!(binding
        .record_message_observation(&other, ack("late-contradiction"))
        .is_err());
}

#[tokio::test]
async fn journal_and_projection_roll_back_together_and_corrupt_content_is_not_sent() {
    let f = Fixture::new().await;
    let binding = f.binding();
    binding.stage_message(request("op")).unwrap();
    let ticket = binding.reserve_message("op").unwrap();
    let connection = f.service.connection().unwrap();
    connection.execute_batch("CREATE TRIGGER reject_message_projection BEFORE UPDATE ON outgoing_messages BEGIN SELECT RAISE(ABORT,'injected storage failure'); END;").unwrap();
    assert!(binding
        .record_message_observation(&ticket, ack("ack"))
        .is_err());
    assert_eq!(
        binding.message_status("op").unwrap().state,
        MessageState::Dispatching
    );
    assert!(binding
        .message_observations("op", 0, 10)
        .unwrap()
        .records
        .is_empty());
    connection.execute_batch("DROP TRIGGER reject_message_projection; UPDATE outgoing_messages SET body=json_set(body,'$.request.content','corrupted');").unwrap();
    assert!(matches!(
        binding.message_status("op"),
        Err(IntegrationError::Invalid(_))
    ));
    assert!(binding.recover_message_record("op").is_err());
}

#[tokio::test]
async fn migration_retains_history_and_legacy_generation_cannot_reserve() {
    let f = Fixture::new().await;
    let binding = f.binding();
    let connection = f.service.connection().unwrap();
    connection
        .execute_batch(
            "DROP TABLE outgoing_messages; DROP TABLE message_observations; PRAGMA user_version=9",
        )
        .unwrap();
    drop(connection);
    let reopened = IntegrationService::open(f.dir.path().join("state")).unwrap();
    let first = reopened
        .binding(binding.id())
        .unwrap()
        .stage_message(request("op"))
        .unwrap();
    assert_eq!(
        reopened
            .binding(binding.id())
            .unwrap()
            .message_status("op")
            .unwrap(),
        first
    );
    assert_eq!(
        reopened
            .events(&f.installation, 0, 10)
            .unwrap()
            .records
            .len(),
        1
    );
    f.start(false).await;
    let legacy = f.binding();
    legacy.stage_message(request("legacy")).unwrap();
    assert!(matches!(
        legacy.reserve_message("legacy"),
        Err(IntegrationError::Unsupported(_))
    ));
    assert_eq!(
        legacy.recover_message_record("legacy").unwrap().state,
        MessageState::Accepted
    );
    assert!(legacy
        .stage_message(MessageRequest {
            operation_id: "large".into(),
            content: "x".repeat(MESSAGE_CONTENT_LIMIT + 1)
        })
        .is_err());
    assert!(legacy.message_observations("legacy", u64::MAX, 1).is_err());
}
