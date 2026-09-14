//! ADR 0022: the in-process backend records the harness that ran it.
//!
//! The snapshot is opt-in behind `RunOptions::capture_harness`; hook frames
//! and tool durations are always on.

use aikit_sdk::llm::mock::{MockGateway, MockResponse};
use aikit_sdk::session_store::SessionStore;
use aikit_sdk::{
    run_aikit_agent_with_gateway, AgentEventPayload, HookAction, HookPhase, RunOptions,
};
use tempfile::TempDir;

fn options(tmp: &TempDir, capture: bool) -> RunOptions {
    RunOptions::new()
        .with_model("test-model")
        .with_current_dir(tmp.path().to_path_buf())
        .with_capture_harness(capture)
}

fn store(tmp: &TempDir) -> SessionStore {
    SessionStore {
        sessions_dir: tmp.path().join("sessions"),
    }
}

fn run(tmp: &TempDir, capture: bool, responses: Vec<MockResponse>) -> Vec<AgentEventPayload> {
    let mut payloads = Vec::new();
    run_aikit_agent_with_gateway(
        "hi",
        &options(tmp, capture),
        Box::new(MockGateway::new(responses)),
        Some(store(tmp)),
        |ev| payloads.push(ev.payload),
    )
    .unwrap();
    payloads
}

fn snapshot_count(payloads: &[AgentEventPayload]) -> usize {
    payloads
        .iter()
        .filter(|p| matches!(p, AgentEventPayload::HarnessSnapshot { .. }))
        .count()
}

#[test]
fn capture_harness_emits_one_snapshot_right_after_session_started() {
    let tmp = TempDir::new().unwrap();
    let payloads = run(&tmp, true, vec![MockResponse::text("done")]);
    assert!(matches!(
        payloads[0],
        AgentEventPayload::SessionStarted { .. }
    ));
    match &payloads[1] {
        AgentEventPayload::HarnessSnapshot {
            backend,
            model,
            system_prompt,
            tools,
            hooks,
            ..
        } => {
            assert_eq!(backend, "aikit");
            assert_eq!(model.as_deref(), Some("test-model"));
            assert!(system_prompt
                .as_deref()
                .unwrap_or_default()
                .contains("helpful AI agent"));
            let read_file = tools
                .iter()
                .find(|t| t.name == "read_file")
                .expect("read_file in the snapshot");
            assert_eq!(
                read_file.description.as_deref(),
                Some("Read the contents of a file")
            );
            assert_eq!(
                read_file.input_schema.as_ref().unwrap()["required"][0],
                "path"
            );
            assert!(hooks.iter().any(|h| h == "context_compression"));
            assert!(hooks.iter().any(|h| h == "tool_dispatch"));
        }
        other => panic!("expected HarnessSnapshot second, got {other:?}"),
    }
    assert_eq!(snapshot_count(&payloads), 1);
}

#[test]
fn snapshot_is_absent_unless_asked_for() {
    let tmp = TempDir::new().unwrap();
    let payloads = run(&tmp, false, vec![MockResponse::text("done")]);
    assert_eq!(snapshot_count(&payloads), 0);
    // The default is off; an eval that never opts in sees no new frame.
    assert!(!RunOptions::default().capture_harness);
}

#[test]
fn tool_results_carry_a_duration_and_an_unknown_tool_is_a_blocked_hook() {
    let tmp = TempDir::new().unwrap();
    let payloads = run(
        &tmp,
        false,
        vec![
            MockResponse::tool_call("c1", "no_such_tool", "{}"),
            MockResponse::text("done"),
        ],
    );
    assert!(payloads.iter().any(|p| matches!(
        p,
        AgentEventPayload::AikitToolResult {
            call_id,
            is_error: true,
            duration_ms: Some(_),
            started_at_ms: Some(ms),
            ..
        } if call_id == "c1" && *ms > 0
    )));
    assert!(payloads.iter().any(|p| matches!(
        p,
        AgentEventPayload::Hook {
            phase: HookPhase::BeforeTool,
            hook_name,
            action: HookAction::Blocked,
            payload: Some(detail),
        } if hook_name == "tool_dispatch" && detail["tool_name"] == "no_such_tool"
    )));
}

#[test]
fn hook_and_snapshot_frames_serialize_under_their_own_tags() {
    let tmp = TempDir::new().unwrap();
    let payloads = run(
        &tmp,
        true,
        vec![
            MockResponse::tool_call("c1", "no_such_tool", "{}"),
            MockResponse::text("done"),
        ],
    );
    let json: Vec<String> = payloads
        .iter()
        .map(|p| serde_json::to_string(p).unwrap())
        .collect();
    assert!(json
        .iter()
        .any(|j| j.starts_with(r#"{"harness_snapshot":{"backend":"aikit""#)));
    assert!(json.iter().any(|j| j.contains(
        r#""hook":{"phase":"before_tool","hook_name":"tool_dispatch","action":"blocked""#
    )));
}
