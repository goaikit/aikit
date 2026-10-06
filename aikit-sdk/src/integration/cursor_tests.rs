use super::*;
use crate::integration::{
    DecisionFuture, HookHandler, InstallSpec, InstallationStatus, IntegrationCapability,
    IntegrationService, SessionMode, Support,
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

#[test]
fn native_command_preserves_argv_utf8_streams_and_exit_status() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    let probe_dir = dir.path().join("spaces ' $ ; unicode é");
    std::fs::create_dir(&probe_dir).unwrap();
    let executable = probe_dir.join(if cfg!(windows) { "probe.exe" } else { "probe" });
    let arguments = vec![
        String::new(),
        "quote\"and\\".into(),
        "'$(not-a-command); & %NAME% !NAME!\nUTF8-é".into(),
    ];
    // A tiny native process tests the real platform parser and byte streams,
    // rather than validating a serializer against another copy of its rules.
    let source = format!(
        r#"
        use std::io::{{Read, Write}};
        fn main() {{
            assert_eq!(std::env::args().skip(1).collect::<Vec<_>>(), {arguments:?});
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).unwrap();
            assert_eq!(input, "stdin-é\\n");
            std::io::stdout().write_all("stdout-é".as_bytes()).unwrap();
            std::io::stderr().write_all("stderr-é".as_bytes()).unwrap();
            std::process::exit(23);
        }}
    "#
    );
    let source_path = dir.path().join("probe.rs");
    std::fs::write(&source_path, source).unwrap();
    let compiler = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg(&source_path)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compiler.status.success(),
        "{}",
        String::from_utf8_lossy(&compiler.stderr)
    );
    let serialized = command(&HookCommand {
        executable,
        arguments,
    })
    .unwrap();
    let mut process = if cfg!(windows) {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(serialized.split(' ').skip(1));
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", &serialized]);
        cmd
    };
    let mut child = process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all("stdin-é\\n".as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(23),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, "stdout-é".as_bytes());
    assert_eq!(output.stderr, "stderr-é".as_bytes());
}

#[test]
fn cursor_command_serialization_keeps_shell_syntax_inside_literal_data() {
    use base64::Engine;
    let handler = HookCommand {
        executable: "C:/handler with spaces.exe".into(),
        arguments: vec![
            "".into(),
            "quote\"and\\".into(),
            "'$(unsafe); & %NAME% !NAME!\nUTF8-é".into(),
        ],
    };
    let unix = command_for(&handler, false).unwrap();
    assert!(unix.contains("'\\''"));
    let windows = command_for(&handler, true).unwrap();
    let encoded = windows
        .strip_prefix("powershell.exe -NoProfile -NonInteractive -EncodedCommand ")
        .unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    let script = String::from_utf16(
        &bytes
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert!(script.contains("$p.UseShellExecute=$false"));
    assert!(script.contains("$p.FileName='C:/handler with spaces.exe'"));
    assert!(script.contains("''$(unsafe)"));
    assert_eq!(windows_argument(""), "\"\"");
    assert_eq!(windows_argument("a\"b\\"), "\"a\\\"b\\\\\"");
}
