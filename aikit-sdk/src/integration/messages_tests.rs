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
        if self
            .service
            .agent_tool_environment_key(&self.installation)
            .unwrap()
            .is_some()
        {
            // Owned modules defer classic startup without inherited context.
            // Supply a synthetic journal start at the current revision for this
            // storage/RPC fixture; do not pretend a native module was executed.
            let mut start = self
                .service
                .events(&self.installation, 0, 100)
                .unwrap()
                .records[0]
                .request
                .clone();
            start.id = uuid::Uuid::new_v4().to_string();
            start.event = HookEvent::SessionStarted;
            start.native_turn = None;
            start.invocation_id = generation.then(|| uuid::Uuid::new_v4().to_string());
            self.service.connection().unwrap().execute("INSERT INTO hook_invocations(id,installation_id,session_id,body,installation_revision) VALUES (?1,?2,'session',?3,?4)",
                params![start.id,self.installation,serde_json::to_string(&start).unwrap(),self.service.installation_revision(&self.installation).unwrap()]).unwrap();
            return;
        }
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
fn append_intent(row: &str) -> crate::integration::MessageAppendIntent {
    crate::integration::MessageAppendIntent {
        native_row_id: row.into(),
        native_source: "claude-code/2.1.296/session.append".into(),
        framed_payload_sha256: "a".repeat(64),
    }
}
fn append_stored() -> crate::integration::MessageAppendStored {
    crate::integration::MessageAppendStored {
        stored_payload_sha256: "a".repeat(64),
        evidence_reference: "retained-native-row-result".into(),
    }
}

#[tokio::test]
async fn append_checkpoint_survives_lost_return_and_reopen_without_ack_or_resend() {
    let f = Fixture::new().await;
    let binding = f.binding();
    binding.stage_message(request("append")).unwrap();
    let ticket = binding.reserve_message("append").unwrap();
    let prepared = binding
        .prepare_message_append(&ticket, append_intent("row"))
        .unwrap();
    assert!(prepared.stored.is_none());
    assert_eq!(
        binding
            .prepare_message_append(&ticket, append_intent("row"))
            .unwrap(),
        prepared
    );
    assert_ne!(prepared.intent.framed_payload_sha256, ticket.payload_sha256);
    let recovered = binding.recover_message_record("append").unwrap();
    assert_eq!(recovered.state, MessageState::Unknown);
    assert!(recovered.acknowledgement.is_none() && recovered.consumption.is_none());
    let reopened = IntegrationService::open_existing(f.dir.path().join("state")).unwrap();
    let historical = reopened.binding(binding.id()).unwrap();
    assert_eq!(
        historical.message_append_checkpoint("append").unwrap(),
        Some(prepared)
    );
    let stored = historical
        .record_message_append_stored(&ticket, "row", append_stored())
        .unwrap();
    assert!(stored.stored_at_unix_ms.is_some());
    assert_eq!(
        historical
            .record_message_append_stored(&ticket, "row", append_stored())
            .unwrap(),
        stored
    );
    assert_eq!(historical.message_status("append").unwrap(), recovered);
    assert!(historical
        .message_observations("append", 0, 10)
        .unwrap()
        .records
        .is_empty());
    assert!(historical.reserve_message("append").is_err());
}

#[tokio::test]
async fn append_rewrite_is_retained_without_replacing_intent_or_qualified_proof() {
    let f = Fixture::new().await;
    let binding = f.binding();
    binding.stage_message(request("rewrite")).unwrap();
    let ticket = binding.reserve_message("rewrite").unwrap();
    binding
        .prepare_message_append(&ticket, append_intent("row"))
        .unwrap();
    let mut changed = append_stored();
    changed.stored_payload_sha256 = "b".repeat(64);
    let result = binding
        .record_message_append_stored(&ticket, "row", changed)
        .unwrap();
    assert_ne!(
        result.intent.framed_payload_sha256,
        result.stored.unwrap().stored_payload_sha256
    );
    assert!(binding
        .record_message_append_stored(&ticket, "row", append_stored())
        .is_err());
    assert_eq!(
        binding.message_status("rewrite").unwrap().state,
        MessageState::Dispatching
    );
    let proven = binding
        .record_message_observation(&ticket, ack("independent-proof"))
        .unwrap();
    assert_eq!(proven.state, MessageState::Acknowledged);
    assert_eq!(binding.message_status("rewrite").unwrap(), proven);
    assert!(binding
        .message_append_checkpoint("rewrite")
        .unwrap()
        .unwrap()
        .stored
        .is_some());
}

#[tokio::test]
async fn append_checkpoint_rejects_row_reuse_changed_scope_and_unprepared_storage() {
    let f = Fixture::new().await;
    let binding = f.binding();
    binding.stage_message(request("one")).unwrap();
    binding.stage_message(request("two")).unwrap();
    let one = binding.reserve_message("one").unwrap();
    let two = binding.reserve_message("two").unwrap();
    assert!(binding
        .record_message_append_stored(&one, "row", append_stored())
        .is_err());
    let prepared = binding
        .prepare_message_append(&one, append_intent("row"))
        .unwrap();
    assert!(binding
        .prepare_message_append(&two, append_intent("row"))
        .is_err());
    assert!(binding
        .prepare_message_append(&one, append_intent("changed-row"))
        .is_err());
    assert!(binding
        .record_message_append_stored(&one, "changed-row", append_stored())
        .is_err());
    for fault in 0..4 {
        let mut foreign = one.clone();
        match fault {
            0 => foreign.reservation_id = "foreign".into(),
            1 => foreign.target.invocation_id = Some("foreign".into()),
            2 => foreign.request.content.push_str("changed"),
            _ => foreign.payload_sha256 = "b".repeat(64),
        }
        assert!(binding
            .prepare_message_append(&foreign, append_intent("row"))
            .is_err());
        assert!(binding
            .record_message_append_stored(&foreign, "row", append_stored())
            .is_err());
    }
    assert_eq!(
        binding.message_append_checkpoint("one").unwrap(),
        Some(prepared)
    );
    assert!(binding.message_append_checkpoint("two").unwrap().is_none());
}

#[tokio::test]
async fn append_late_storage_is_historical_but_new_intent_requires_original_active_attempt() {
    let f = Fixture::new().await;
    let binding = f.binding();
    binding.stage_message(request("one")).unwrap();
    binding.stage_message(request("two")).unwrap();
    let one = binding.reserve_message("one").unwrap();
    let two = binding.reserve_message("two").unwrap();
    binding
        .prepare_message_append(&one, append_intent("row"))
        .unwrap();
    binding.detach().unwrap();
    assert!(binding
        .prepare_message_append(&two, append_intent("new-row"))
        .is_err());
    f.start(true).await;
    let stored = binding
        .record_message_append_stored(&one, "row", append_stored())
        .unwrap();
    assert_eq!(
        binding
            .prepare_message_append(&one, append_intent("row"))
            .unwrap(),
        stored
    );
    assert!(binding.reserve_message("one").is_err());
    assert!(binding
        .message_status("one")
        .unwrap()
        .acknowledgement
        .is_none());
}

#[tokio::test]
async fn append_invalid_bounds_digests_and_corrupt_stored_index_fail_closed() {
    let f = Fixture::new().await;
    let binding = f.binding();
    binding.stage_message(request("one")).unwrap();
    let ticket = binding.reserve_message("one").unwrap();
    for fault in 0..4 {
        let mut intent = append_intent("row");
        match fault {
            0 => intent.native_row_id = "x".repeat(257),
            1 => intent.native_row_id.push('\0'),
            2 => intent.native_source.clear(),
            _ => intent.framed_payload_sha256 = "A".repeat(64),
        }
        assert!(binding.prepare_message_append(&ticket, intent).is_err());
    }
    assert!(binding.message_append_checkpoint("one").unwrap().is_none());
    binding
        .prepare_message_append(&ticket, append_intent("row"))
        .unwrap();
    let mut invalid = append_stored();
    invalid.evidence_reference = "x".repeat(4097);
    assert!(binding
        .record_message_append_stored(&ticket, "row", invalid)
        .is_err());
    f.service
        .connection()
        .unwrap()
        .execute(
            "UPDATE message_append_checkpoints SET native_row_id='corrupt'",
            [],
        )
        .unwrap();
    assert!(binding.message_append_checkpoint("one").is_err());
}

#[tokio::test]
async fn append_schema_eleven_upgrade_retains_unknown_handoff_and_never_reserves_again() {
    use crate::integration::{MessageHandoffOutcome, MessageHandoffRecord};
    let f = Fixture::new().await;
    let binding = f.binding();
    binding.stage_message(request("old")).unwrap();
    let ticket = binding.reserve_message("old").unwrap();
    binding.recover_message_record("old").unwrap();
    let before = binding.message_status("old").unwrap();
    let handoff = MessageHandoffRecord {
        schema_version: 1,
        reservation: ticket.clone(),
        outcome: MessageHandoffOutcome::Returned {
            result: json!({"text":"raw return only"}),
        },
        recorded_at_unix_ms: 1,
    };
    let connection = f.service.connection().unwrap();
    connection
        .execute(
            "INSERT INTO message_handoffs VALUES (?1,'old',?2)",
            params![f.installation, serde_json::to_string(&handoff).unwrap()],
        )
        .unwrap();
    connection
        .execute_batch("DROP TABLE message_append_checkpoints; PRAGMA user_version=11;")
        .unwrap();
    assert!(IntegrationService::open_existing(f.dir.path().join("state")).is_err());
    assert!(IntegrationService::upgrade_existing(f.dir.path().join("state")).is_err());
    binding.detach().unwrap();
    let report = IntegrationService::upgrade_existing(f.dir.path().join("state")).unwrap();
    assert_eq!(
        (
            report.previous_schema,
            report.current_schema,
            report.changed
        ),
        (11, 14, true)
    );
    assert!(
        !IntegrationService::upgrade_existing(f.dir.path().join("state"))
            .unwrap()
            .changed
    );
    let reopened = IntegrationService::open_existing(f.dir.path().join("state")).unwrap();
    let old = reopened.binding(binding.id()).unwrap();
    assert_eq!(old.message_handoff("old").unwrap(), Some(handoff));
    assert_eq!(old.message_status("old").unwrap(), before);
    assert!(old.message_append_checkpoint("old").unwrap().is_none());
    assert!(old
        .prepare_message_append(&ticket, append_intent("row"))
        .is_err());
    assert!(old.reserve_message("old").is_err());
}

#[tokio::test]
async fn append_schema_upgrade_rolls_back_on_malformed_checkpoint_table() {
    let f = Fixture::new().await;
    let binding = f.binding();
    binding.stage_message(request("retained")).unwrap();
    let before = binding.message_status("retained").unwrap();
    binding.detach().unwrap();
    let connection = f.service.connection().unwrap();
    connection.execute_batch("DROP TABLE message_append_checkpoints; CREATE TABLE message_append_checkpoints(broken TEXT); PRAGMA user_version=11;").unwrap();
    assert!(matches!(
        IntegrationService::upgrade_existing(f.dir.path().join("state")),
        Err(IntegrationError::Sql(_))
    ));
    assert_eq!(
        connection
            .pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        11
    );
    assert_eq!(binding.message_status("retained").unwrap(), before);
}

#[tokio::test]
async fn recorded_append_bridge_retains_busy_candidates_and_never_projects_delivery() {
    use crate::integration::RecordedMessageAction;
    let f = bridge_fixture().await;
    let binding = f.binding();
    binding.stage_message(request("bridge-append")).unwrap();
    let ticket = binding.reserve_message("bridge-append").unwrap();
    synthetic_bridge_event(&f, HookEvent::NativeTurnStarted);
    let (prepare, env) = bridge_frame(
        &f,
        RecordedMessageAction::PrepareAppend {
            operation_id: ticket.request.operation_id.clone(),
            reservation_id: ticket.reservation_id.clone(),
            intent: append_intent("native-row"),
        },
    );
    let reply = f
        .service
        .recorded_message_with_context(&f.installation, prepare.clone(), Some(&env))
        .unwrap();
    assert!(!reply.recorded_idle && !reply.recorded_ready);
    assert_eq!(
        reply.append_checkpoint,
        binding.message_append_checkpoint("bridge-append").unwrap()
    );
    let mut stored = append_stored();
    stored.stored_payload_sha256 = "b".repeat(64);
    let (record, _) = bridge_frame(
        &f,
        RecordedMessageAction::RecordAppendStored {
            operation_id: ticket.request.operation_id.clone(),
            reservation_id: ticket.reservation_id.clone(),
            native_row_id: "native-row".into(),
            stored,
        },
    );
    let reply = f
        .service
        .recorded_message_with_context(&f.installation, record.clone(), Some(&env))
        .unwrap();
    let checkpoint = reply.append_checkpoint.unwrap();
    assert_ne!(
        checkpoint.intent.framed_payload_sha256,
        checkpoint.stored.as_ref().unwrap().stored_payload_sha256
    );
    assert_eq!(
        f.service
            .recorded_message_with_context(&f.installation, record, Some(&env))
            .unwrap()
            .append_checkpoint,
        Some(checkpoint.clone())
    );
    assert_eq!(
        binding.message_status("bridge-append").unwrap().state,
        MessageState::Dispatching
    );
    assert!(binding
        .message_observations("bridge-append", 0, 10)
        .unwrap()
        .records
        .is_empty());
    binding.recover_message_record("bridge-append").unwrap();
    assert_eq!(
        f.service
            .recorded_message_with_context(&f.installation, prepare, Some(&env))
            .unwrap()
            .append_checkpoint,
        Some(checkpoint)
    );
    assert!(binding.reserve_message("bridge-append").is_err());
}

#[tokio::test]
async fn recorded_append_bridge_rejects_foreign_attempt_rows_and_inherited_scope() {
    use crate::integration::RecordedMessageAction;
    let f = bridge_fixture().await;
    let binding = f.binding();
    binding.stage_message(request("bridge-append")).unwrap();
    let ticket = binding.reserve_message("bridge-append").unwrap();
    let (mut prepare, env) = bridge_frame(
        &f,
        RecordedMessageAction::PrepareAppend {
            operation_id: ticket.request.operation_id.clone(),
            reservation_id: "foreign".into(),
            intent: append_intent("row"),
        },
    );
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, prepare.clone(), Some(&env))
        .is_err());
    assert!(binding
        .message_append_checkpoint("bridge-append")
        .unwrap()
        .is_none());
    if let RecordedMessageAction::PrepareAppend { reservation_id, .. } = &mut prepare.action {
        *reservation_id = ticket.reservation_id.clone();
    }
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, prepare.clone(), Some("foreign"))
        .is_err());
    f.service
        .recorded_message_with_context(&f.installation, prepare.clone(), Some(&env))
        .unwrap();
    let (record, _) = bridge_frame(
        &f,
        RecordedMessageAction::RecordAppendStored {
            operation_id: ticket.request.operation_id.clone(),
            reservation_id: ticket.reservation_id.clone(),
            native_row_id: "foreign-row".into(),
            stored: append_stored(),
        },
    );
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, record, Some(&env))
        .is_err());
    assert!(binding
        .message_append_checkpoint("bridge-append")
        .unwrap()
        .unwrap()
        .stored
        .is_none());
    f.start(true).await;
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, prepare, Some(&env))
        .is_err());
    assert_eq!(
        binding.message_status("bridge-append").unwrap().state,
        MessageState::Dispatching
    );
}

#[tokio::test]
async fn recorded_append_bridge_reader_bounds_candidate_fields_before_journal_access() {
    use crate::integration::{RecordedMessageAction, RecordedMessageRequest};
    let f = bridge_fixture().await;
    for fault in 0..4 {
        let mut intent = append_intent("row");
        match fault {
            0 => intent.native_row_id = "x".repeat(257),
            1 => intent.native_source.clear(),
            2 => intent.framed_payload_sha256 = "G".repeat(64),
            _ => intent.native_source.push('\0'),
        }
        let (frame, _) = bridge_frame(
            &f,
            RecordedMessageAction::PrepareAppend {
                operation_id: "op".into(),
                reservation_id: "attempt".into(),
                intent,
            },
        );
        assert!(
            RecordedMessageRequest::read(serde_json::to_vec(&frame).unwrap().as_slice()).is_err()
        );
    }
    let (frame, _) = bridge_frame(
        &f,
        RecordedMessageAction::RecordAppendStored {
            operation_id: "op".into(),
            reservation_id: "attempt".into(),
            native_row_id: "row".into(),
            stored: append_stored(),
        },
    );
    assert!(RecordedMessageRequest::read(serde_json::to_vec(&frame).unwrap().as_slice()).is_ok());
    let mut value = serde_json::to_value(frame).unwrap();
    value["action"]["native_row_id"] = json!("row\0");
    assert!(RecordedMessageRequest::read(value.to_string().as_bytes()).is_err());
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

async fn bridge_fixture() -> Fixture {
    let f = Fixture::new().await;
    let mut hook_spec = f.service.installed(&f.installation).unwrap().spec;
    hook_spec.events.push(HookEvent::InputSubmitted);
    let hooks = f.service.plan_install(hook_spec).unwrap();
    f.service.apply_install(&hooks.id).unwrap();
    let plan = f
        .service
        .plan_agent_tool(
            &f.installation,
            crate::integration::AgentToolSpec {
                application_id: "message-fixture".into(),
                installation_id: f.installation.clone(),
                tool_name: "review".into(),
                description: "Fixture only".into(),
                input_schema: json!({"type":"object"}),
                instructions: "".into(),
                handler: HookCommand {
                    executable: std::env::current_exe().unwrap(),
                    arguments: vec![],
                },
                timeout_ms: 1000,
            },
        )
        .unwrap();
    f.service.apply_install(&plan.id).unwrap();
    f.start(true).await;
    f
}
fn bridge_frame(
    f: &Fixture,
    action: crate::integration::RecordedMessageAction,
) -> (crate::integration::RecordedMessageRequest, String) {
    // A native poll supplies claims; it must not attach the application as an
    // incidental effect of preparing its test envelope.
    let reference = f
        .service
        .observed_session(&f.installation, "session")
        .unwrap();
    let frame = crate::integration::RecordedMessageRequest {
        schema_version: 1,
        application_id: "message-fixture".into(),
        installation_id: f.installation.clone(),
        agent_key: "claude".into(),
        native_context: crate::integration::NativeToolContext {
            session_id: reference.native_session_id,
            invocation_id: reference.invocation_id.unwrap(),
            cwd: f.workspace.clone(),
        },
        action,
    };
    let inherited=json!({"schema_version":1,"installation_id":f.installation,"native_context":frame.native_context}).to_string();
    (frame, inherited)
}
fn synthetic_idle(f: &Fixture) {
    synthetic_bridge_event(f, HookEvent::NativeTurnFinished);
}
fn synthetic_bridge_event(f: &Fixture, event: HookEvent) {
    let binding = f.binding();
    let mut request = f
        .service
        .events(&f.installation, 0, 100)
        .unwrap()
        .records
        .last()
        .unwrap()
        .request
        .clone();
    request.event = event;
    request.invocation_id = binding.reference().invocation_id.clone();
    request.native_turn = Some(crate::integration::NativeTurn {
        id: "synthetic-completed-turn".into(),
        outcome: Some(crate::integration::NativeTurnOutcome {
            reason: crate::integration::NativeTurnReason::Answer,
            is_aborted: false,
        }),
    });
    f.service.connection().unwrap().execute("INSERT INTO hook_invocations(id,installation_id,session_id,body,installation_revision) VALUES (?1,?2,'session',?3,?4)",
        params![uuid::Uuid::new_v4().to_string(),f.installation,serde_json::to_string(&request).unwrap(),binding.reference().installation_revision]).unwrap();
}

#[tokio::test]
async fn inspection_never_attaches_or_reserves_and_recheck_retires_only_the_exact_attempt() {
    use crate::integration::RecordedMessageAction::{Inspect, Poll, Recheck};
    let f = bridge_fixture().await;
    let (inspect, env) = bridge_frame(&f, Inspect);
    let before = f
        .service
        .recorded_message_with_context(&f.installation, inspect.clone(), Some(&env))
        .unwrap();
    assert!(!before.recorded_idle && before.reservation.is_none());
    let count: i64 = f
        .service
        .connection()
        .unwrap()
        .query_row("SELECT count(*) FROM session_bindings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let binding = f.binding();
    binding.stage_message(request("old")).unwrap();
    synthetic_idle(&f);
    let idle = f
        .service
        .recorded_message_with_context(&f.installation, inspect.clone(), Some(&env))
        .unwrap();
    assert!(idle.recorded_idle && !idle.recorded_ready && idle.reservation.is_none());
    assert_eq!(
        binding.message_status("old").unwrap().state,
        MessageState::Accepted
    );
    let (poll, _) = bridge_frame(&f, Poll);
    let ticket = f
        .service
        .recorded_message_with_context(&f.installation, poll.clone(), Some(&env))
        .unwrap()
        .reservation
        .unwrap();
    let (wrong, _) = bridge_frame(
        &f,
        Recheck {
            operation_id: "old".into(),
            reservation_id: "wrong".into(),
        },
    );
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, wrong, Some(&env))
        .is_err());
    binding.recover_message_record("old").unwrap();
    let (recheck, _) = bridge_frame(
        &f,
        Recheck {
            operation_id: "old".into(),
            reservation_id: ticket.reservation_id,
        },
    );
    let retired = f
        .service
        .recorded_message_with_context(&f.installation, recheck, Some(&env))
        .unwrap();
    assert!(!retired.recorded_ready);
    assert_eq!(retired.recorded_state, Some(MessageState::Unknown));
    binding.stage_message(request("next")).unwrap();
    let next = f
        .service
        .recorded_message_with_context(&f.installation, poll, Some(&env))
        .unwrap()
        .reservation
        .unwrap();
    assert_eq!(next.request.operation_id, "next");
    binding.detach().unwrap();
    assert!(matches!(
        f.service
            .recorded_message_with_context(&f.installation, inspect, Some(&env)),
        Err(IntegrationError::Detached)
    ));
}

#[tokio::test]
async fn module_poll_waits_for_first_application_binding_but_never_revives_detach() {
    use crate::integration::RecordedMessageAction::Poll;
    let f = bridge_fixture().await;
    let (frame, env) = bridge_frame(&f, Poll);
    let deferred = f
        .service
        .recorded_message_with_context(&f.installation, frame.clone(), Some(&env))
        .unwrap();
    assert!(!deferred.recorded_idle && !deferred.recorded_ready && deferred.reservation.is_none());
    let count: i64 = f
        .service
        .connection()
        .unwrap()
        .query_row("SELECT count(*) FROM session_bindings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "poll must not attach the application");
    let binding = f.binding();
    binding.stage_message(request("op")).unwrap();
    synthetic_idle(&f);
    let ticket = f
        .service
        .recorded_message_with_context(&f.installation, frame.clone(), Some(&env))
        .unwrap();
    assert!(ticket.reservation.is_some());
    binding.detach().unwrap();
    assert!(matches!(
        f.service
            .recorded_message_with_context(&f.installation, frame, Some(&env)),
        Err(IntegrationError::Detached)
    ));
}

#[tokio::test]
async fn recorded_bridge_defers_busy_reserves_once_and_rechecks_without_sending() {
    use crate::integration::RecordedMessageAction::{Poll, Recheck};
    let f = bridge_fixture().await;
    let binding = f.binding();
    binding.stage_message(request("op")).unwrap();
    let (frame, env) = bridge_frame(&f, Poll);
    let deferred = f
        .service
        .recorded_message_with_context(&f.installation, frame.clone(), Some(&env))
        .unwrap();
    assert!(!deferred.recorded_idle);
    assert!(deferred.reservation.is_none());
    assert_eq!(
        binding.message_status("op").unwrap().state,
        MessageState::Accepted
    );
    synthetic_idle(&f);
    let prepared = f
        .service
        .recorded_message_with_context(&f.installation, frame.clone(), Some(&env))
        .unwrap();
    assert!(prepared.recorded_idle);
    assert!(!prepared.recorded_ready);
    let ticket = prepared.reservation.unwrap();
    assert_eq!(ticket.request, request("op"));
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, frame, Some(&env))
        .unwrap()
        .reservation
        .is_none());
    let (recheck, env) = bridge_frame(
        &f,
        Recheck {
            operation_id: "op".into(),
            reservation_id: ticket.reservation_id.clone(),
        },
    );
    assert!(
        f.service
            .recorded_message_with_context(&f.installation, recheck.clone(), Some(&env))
            .unwrap()
            .recorded_ready
    );
    synthetic_bridge_event(&f, HookEvent::InputSubmitted);
    let busy = f
        .service
        .recorded_message_with_context(&f.installation, recheck.clone(), Some(&env))
        .unwrap();
    assert!(!busy.recorded_idle);
    assert!(!busy.recorded_ready);
    binding.recover_message_record("op").unwrap();
    assert!(
        !f.service
            .recorded_message_with_context(&f.installation, recheck, Some(&env))
            .unwrap()
            .recorded_ready
    );
    assert_eq!(
        binding.message_status("op").unwrap().state,
        MessageState::Unknown
    );
}

#[tokio::test]
async fn recorded_bridge_rejects_claims_replacement_detach_and_missing_owned_module() {
    use crate::integration::RecordedMessageAction::Poll;
    let plain = Fixture::new().await;
    let (frame, env) = bridge_frame(&plain, Poll);
    assert!(plain
        .service
        .recorded_message_with_context(&plain.installation, frame, Some(&env))
        .is_err());
    let f = bridge_fixture().await;
    let binding = f.binding();
    binding.stage_message(request("op")).unwrap();
    synthetic_idle(&f);
    let (frame, env) = bridge_frame(&f, Poll);
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, frame.clone(), None)
        .is_err());
    let mut foreign = frame.clone();
    foreign.application_id = "foreign-app".into();
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, foreign, Some(&env))
        .is_err());
    let mut foreign = frame.clone();
    foreign.native_context.invocation_id = uuid::Uuid::new_v4().to_string();
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, foreign, Some(&env))
        .is_err());
    binding.detach().unwrap();
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, frame.clone(), Some(&env))
        .is_err());
    // Polling cannot undo detach or create another application binding.
    let count: i64 = f
        .service
        .connection()
        .unwrap()
        .query_row("SELECT count(*) FROM session_bindings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    f.start(true).await;
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, frame, Some(&env))
        .is_err());
    assert_eq!(
        binding.message_status("op").unwrap().state,
        MessageState::Accepted
    );
}

#[tokio::test]
async fn recorded_bridge_bounds_fail_before_reservation_and_frames_cannot_choose_state_paths() {
    use crate::integration::{
        RecordedMessageAction::Poll, RecordedMessageRequest, MESSAGE_BRIDGE_PAYLOAD_LIMIT,
    };
    let f = bridge_fixture().await;
    let binding = f.binding();
    binding
        .stage_message(MessageRequest {
            operation_id: "large".into(),
            content: "x".repeat(MESSAGE_BRIDGE_PAYLOAD_LIMIT + 1),
        })
        .unwrap();
    synthetic_idle(&f);
    let (frame, env) = bridge_frame(&f, Poll);
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, frame.clone(), Some(&env))
        .is_err());
    assert_eq!(
        binding.message_status("large").unwrap().state,
        MessageState::Accepted
    );
    let mut json = serde_json::to_value(&frame).unwrap();
    json["database"] = json!("arbitrary.db");
    assert!(RecordedMessageRequest::read(json.to_string().as_bytes()).is_err());
    assert!(RecordedMessageRequest::read(
        vec![b'x'; super::super::AGENT_TOOL_CALL_LIMIT + 1].as_slice()
    )
    .is_err());
    json.as_object_mut().unwrap().remove("database");
    json["schema_version"] = json!(2);
    assert!(RecordedMessageRequest::read(json.to_string().as_bytes()).is_err());
    assert!(RecordedMessageRequest::read(serde_json::to_vec(&frame).unwrap().as_slice()).is_ok());
}

#[tokio::test]
async fn inspection_reads_while_a_writer_is_held_but_poll_still_refuses_to_reserve() {
    use crate::integration::RecordedMessageAction::{Inspect, Poll};
    let f = bridge_fixture().await;
    let binding = f.binding();
    binding.stage_message(request("op")).unwrap();
    synthetic_idle(&f);
    let held = f.service.state_lock().unwrap();
    let (inspect, env) = bridge_frame(&f, Inspect);
    let reply = f
        .service
        .recorded_message_with_context(&f.installation, inspect, Some(&env))
        .unwrap();
    assert!(reply.recorded_idle && reply.reservation.is_none());
    let (poll, _) = bridge_frame(&f, Poll);
    assert!(matches!(
        f.service
            .recorded_message_with_context(&f.installation, poll, Some(&env)),
        Err(IntegrationError::Conflict(_))
    ));
    assert_eq!(
        binding.message_status("op").unwrap().state,
        MessageState::Accepted
    );
    drop(held);
}

#[tokio::test]
async fn raw_return_waits_for_only_the_local_writer_and_budget_exhaustion_never_reserves_again() {
    use crate::integration::{MessageHandoffOutcome, RecordedMessageAction};
    let f = bridge_fixture().await;
    let binding = f.binding();
    binding.stage_message(request("op")).unwrap();
    let ticket = binding.reserve_message("op").unwrap();
    let (frame, env) = bridge_frame(
        &f,
        RecordedMessageAction::RecordHandoff {
            operation_id: "op".into(),
            reservation_id: ticket.reservation_id.clone(),
            outcome: MessageHandoffOutcome::Returned {
                result: json!({"text":ticket.request.content}),
            },
        },
    );
    let held = f.service.state_lock().unwrap();
    assert!(matches!(
        f.service
            .recorded_message_with_context(&f.installation, frame.clone(), Some(&env)),
        Err(IntegrationError::Conflict(_))
    ));
    assert!(binding.message_handoff("op").unwrap().is_none());
    assert_eq!(
        binding.message_status("op").unwrap().state,
        MessageState::Dispatching
    );
    drop(held);
    let held = f.service.state_lock().unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(40));
        drop(held);
    });
    f.service
        .recorded_message_with_context(&f.installation, frame.clone(), Some(&env))
        .unwrap();
    release.join().unwrap();
    let stored = binding.message_handoff("op").unwrap().unwrap();
    f.service
        .recorded_message_with_context(&f.installation, frame, Some(&env))
        .unwrap();
    assert_eq!(binding.message_handoff("op").unwrap().unwrap(), stored);
    let receipt = binding.message_status("op").unwrap();
    assert_eq!(receipt.state, MessageState::Unknown);
    assert_eq!(
        receipt.reservation_id.as_deref(),
        Some(ticket.reservation_id.as_str())
    );
    assert!(receipt.acknowledgement.is_none() && receipt.consumption.is_none());
    assert!(binding.reserve_message("op").is_err());
}

#[tokio::test]
async fn raw_handoff_is_durable_and_replayed_exactly_without_qualifying_delivery() {
    use crate::integration::{MessageHandoffOutcome, RecordedMessageAction};
    let f = bridge_fixture().await;
    let binding = f.binding();
    binding.stage_message(request("op")).unwrap();
    let ticket = binding.reserve_message("op").unwrap();
    // Return arrives after native admission started a new busy Turn.
    synthetic_bridge_event(&f, HookEvent::NativeTurnStarted);
    let outcome = MessageHandoffOutcome::Returned {
        result: json!({"text":ticket.request.content,"origin":{"type":"plugin","plugin":"fixture"}}),
    };
    let (frame, env) = bridge_frame(
        &f,
        RecordedMessageAction::RecordHandoff {
            operation_id: "op".into(),
            reservation_id: ticket.reservation_id.clone(),
            outcome: outcome.clone(),
        },
    );
    let reply = f
        .service
        .recorded_message_with_context(&f.installation, frame.clone(), Some(&env))
        .unwrap();
    assert!(!reply.recorded_idle);
    assert!(!reply.recorded_ready);
    let stored = binding.message_handoff("op").unwrap().unwrap();
    assert_eq!(stored.reservation, ticket);
    assert_eq!(stored.outcome, outcome);
    let receipt = binding.message_status("op").unwrap();
    assert_eq!(receipt.state, MessageState::Unknown);
    assert!(receipt.acknowledgement.is_none() && receipt.consumption.is_none());
    assert!(binding
        .message_observations("op", 0, 10)
        .unwrap()
        .records
        .is_empty());
    f.service
        .recorded_message_with_context(&f.installation, frame.clone(), Some(&env))
        .unwrap();
    let reopened = IntegrationService::open(f.dir.path().join("state")).unwrap();
    assert_eq!(
        reopened
            .binding(binding.id())
            .unwrap()
            .message_handoff("op")
            .unwrap()
            .unwrap(),
        stored
    );
    let mut changed = frame.clone();
    if let RecordedMessageAction::RecordHandoff { outcome, .. } = &mut changed.action {
        *outcome = MessageHandoffOutcome::Uncertain {
            reason: "native_submit_threw".into(),
        };
    }
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, changed, Some(&env))
        .is_err());
    let mut wrong = frame;
    if let RecordedMessageAction::RecordHandoff { reservation_id, .. } = &mut wrong.action {
        *reservation_id = "foreign".into();
    }
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, wrong, Some(&env))
        .is_err());
    assert!(binding.reserve_message("op").is_err());
    // Independent trusted proof can resolve the operation later; candidate raw
    // data neither creates nor overwrites it, and remains available after detach.
    let proven = binding
        .record_message_observation(&ticket, ack("native-ack"))
        .unwrap();
    assert!(proven.acknowledgement.is_some());
    binding.detach().unwrap();
    assert_eq!(binding.message_handoff("op").unwrap().unwrap(), stored);
}

#[tokio::test]
async fn uncertain_handoff_and_schema_ten_upgrade_preserve_original_operation() {
    use crate::integration::{
        MessageHandoffOutcome, RecordedMessageAction, RecordedMessageRequest,
    };
    let f = bridge_fixture().await;
    let binding = f.binding();
    let original = binding.stage_message(request("op")).unwrap();
    // Simulate the exact previous schema: no handoff table, all journal rows kept.
    f.service
        .connection()
        .unwrap()
        .execute_batch("DROP TABLE message_handoffs; PRAGMA user_version=10;")
        .unwrap();
    let reopened = IntegrationService::open(f.dir.path().join("state")).unwrap();
    assert_eq!(
        reopened
            .binding(binding.id())
            .unwrap()
            .message_status("op")
            .unwrap(),
        original
    );
    let ticket = binding.reserve_message("op").unwrap();
    let (mut frame, env) = bridge_frame(
        &f,
        RecordedMessageAction::RecordHandoff {
            operation_id: "op".into(),
            reservation_id: ticket.reservation_id,
            outcome: MessageHandoffOutcome::Uncertain {
                reason: "native_submit_threw".into(),
            },
        },
    );
    assert!(RecordedMessageRequest::read(serde_json::to_vec(&frame).unwrap().as_slice()).is_ok());
    f.service
        .recorded_message_with_context(&f.installation, frame.clone(), Some(&env))
        .unwrap();
    assert_eq!(
        binding.message_status("op").unwrap().state,
        MessageState::Unknown
    );
    if let RecordedMessageAction::RecordHandoff { outcome, .. } = &mut frame.action {
        *outcome = MessageHandoffOutcome::Returned {
            result: json!("x".repeat(98305)),
        };
    }
    assert!(RecordedMessageRequest::read(serde_json::to_vec(&frame).unwrap().as_slice()).is_err());
    assert!(f
        .service
        .recorded_message_with_context(&f.installation, frame, Some(&env))
        .is_err());
    assert!(binding
        .message_status("op")
        .unwrap()
        .acknowledgement
        .is_none());
}

#[tokio::test]
async fn opt_in_message_module_uses_owned_file_plans_and_can_be_removed_without_a_pump() {
    let f = bridge_fixture().await;
    let spec = crate::integration::AgentToolSpec {
        application_id: "message-fixture".into(),
        installation_id: f.installation.clone(),
        tool_name: "review".into(),
        description: "Fixture only".into(),
        input_schema: json!({"type":"object"}),
        instructions: "".into(),
        handler: HookCommand {
            executable: std::env::current_exe().unwrap(),
            arguments: vec![],
        },
        timeout_ms: 1000,
    };
    let module_path = || {
        let skills = f.workspace.join(".claude/skills");
        std::fs::read_dir(skills)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
            .join("hooks/register.js")
    };
    let plain = std::fs::read_to_string(module_path()).unwrap();
    assert!(!plain.contains("$.clock.every") && !plain.contains("$.prompt.submit"));
    let plan = f
        .service
        .plan_agent_tool_with_message_bridge(
            &f.installation,
            spec.clone(),
            HookCommand {
                executable: std::env::current_exe().unwrap(),
                arguments: vec!["fixed-message-handler".into()],
            },
        )
        .unwrap();
    f.service.apply_install(&plan.id).unwrap();
    let source = std::fs::read_to_string(module_path()).unwrap();
    assert!(source.contains("$.clock.every") && source.contains("$.prompt.submit"));
    assert!(source.contains("fixed-message-handler"));
    assert!(!source.contains("/* AIKIT_MESSAGE_"));
    let plan = f.service.plan_agent_tool(&f.installation, spec).unwrap();
    f.service.apply_install(&plan.id).unwrap();
    assert_eq!(std::fs::read_to_string(module_path()).unwrap(), plain);
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
async fn explicit_upgrade_retains_unknown_receipt_and_detached_binding_without_resend() {
    let f = Fixture::new().await;
    let binding = f.binding();
    binding
        .stage_message(request("unknown-before-upgrade"))
        .unwrap();
    binding.reserve_message("unknown-before-upgrade").unwrap();
    binding
        .recover_message_record("unknown-before-upgrade")
        .unwrap();
    let before = binding.message_status("unknown-before-upgrade").unwrap();
    assert_eq!(before.state, MessageState::Unknown);
    binding.detach().unwrap();
    let events = f.service.events(&f.installation, 0, 100).unwrap();
    let connection = f.service.connection().unwrap();
    connection
        .execute_batch("DROP TABLE message_handoffs; PRAGMA user_version=10")
        .unwrap();
    let report = IntegrationService::upgrade_existing(f.dir.path().join("state")).unwrap();
    assert_eq!(report.previous_schema, 10);
    assert_eq!(report.current_schema, 14);
    assert!(report.changed);
    let upgraded = IntegrationService::open_existing(f.dir.path().join("state")).unwrap();
    let historical = upgraded.binding(binding.id()).unwrap();
    assert_eq!(historical.status().unwrap(), SessionStatus::Detached);
    assert_eq!(
        historical.message_status("unknown-before-upgrade").unwrap(),
        before
    );
    assert_eq!(
        historical
            .stage_message(request("unknown-before-upgrade"))
            .unwrap(),
        before
    );
    assert!(matches!(
        historical.reserve_message("unknown-before-upgrade"),
        Err(IntegrationError::Detached)
    ));
    assert_eq!(
        upgraded
            .events(&f.installation, 0, 100)
            .unwrap()
            .records
            .len(),
        events.records.len()
    );
    assert_eq!(
        connection
            .query_row::<u32, _, _>("SELECT count(*) FROM message_handoffs", [], |row| row
                .get(0))
            .unwrap(),
        0
    );
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
