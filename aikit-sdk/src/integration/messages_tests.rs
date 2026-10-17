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
