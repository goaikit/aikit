use super::*;
fn spec() -> AgentToolSpec {
    AgentToolSpec {
        application_id: "review-app".into(),
        installation_id: "fixture-installation".into(),
        tool_name: "review".into(),
        description: "Selected review operations".into(),
        input_schema: json!({"type":"object"}),
        instructions: "Current notes 雪; quotes \" and ${literal} are data.".into(),
        handler: HookCommand {
            executable: std::env::current_exe().unwrap(),
            arguments: vec!["inspect".into()],
        },
        timeout_ms: 1000,
        native_completion_bridge: false,
    }
}

#[test]
fn export_is_explicit_new_directory_only_and_does_not_install_settings() {
    let temp = tempfile::tempdir().unwrap();
    let plugin = render_agent_tool_plugin("claude", &spec()).unwrap();
    let destination = temp.path().join("plugin with spaces 雪");
    plugin.export_to_new_directory(&destination).unwrap();
    assert_eq!(plugin.files().len(), 3);
    let manifest: Value =
        serde_json::from_slice(&fs::read(destination.join(".claude-plugin/plugin.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["name"], plugin.name());
    let mut updated = spec();
    updated.instructions.push_str(" New instructions.");
    let replacement = render_agent_tool_plugin("claude", &updated).unwrap();
    let replacement_manifest: Value =
        serde_json::from_str(&replacement.files()[".claude-plugin/plugin.json"]).unwrap();
    assert_eq!(replacement.name(), plugin.name());
    assert_ne!(replacement_manifest["version"], manifest["version"]);
    updated.installation_id = "another-checkout".into();
    assert_ne!(
        render_agent_tool_plugin("claude", &updated).unwrap().name(),
        plugin.name()
    );
    let before = fs::read(destination.join("hooks/register.js")).unwrap();
    assert!(plugin.export_to_new_directory(&destination).is_err());
    assert_eq!(
        fs::read(destination.join("hooks/register.js")).unwrap(),
        before
    );
    assert!(!temp.path().join(".claude/settings.local.json").exists());
}

#[test]
fn unsupported_providers_invalid_schema_names_bounds_and_handlers_are_refused() {
    for provider in ["cursor", "codex", "pi"] {
        assert!(matches!(
            render_agent_tool_plugin(provider, &spec()),
            Err(IntegrationError::Unsupported(_))
        ));
    }
    assert!(matches!(
        render_agent_tool_plugin("made-up", &spec()),
        Err(IntegrationError::UnknownAgent(_))
    ));
    let mut invalid = spec();
    invalid.tool_name = "bad';()".into();
    assert!(render_agent_tool_plugin("claude", &invalid).is_err());
    let mut invalid = spec();
    invalid.input_schema = json!({"type":"object","required":false});
    assert!(render_agent_tool_plugin("claude", &invalid).is_err());
    let mut invalid = spec();
    invalid.input_schema = json!({"type":"object","$ref":"https://example.invalid/never-fetch"});
    assert!(render_agent_tool_plugin("claude", &invalid).is_err());
    let mut invalid = spec();
    invalid.instructions = "x".repeat(32769);
    assert!(render_agent_tool_plugin("claude", &invalid).is_err());
    let mut invalid = spec();
    invalid.handler.arguments.push("bad\0argument".into());
    assert!(render_agent_tool_plugin("claude", &invalid).is_err());
    let mut invalid = spec();
    invalid.handler.executable = PathBuf::from("relative.exe");
    assert!(render_agent_tool_plugin("claude", &invalid).is_err());
}

#[test]
fn invocation_reader_bounds_versions_and_keeps_model_data_out_of_native_metadata() {
    let call = json!({"schema_version":1,"application_id":"review-app","installation_id":"fixture-installation","agent_key":"claude","tool_name":"review",
        "native_context":{"session_id":"native","invocation_id":uuid::Uuid::new_v4().to_string(),"cwd":std::env::current_dir().unwrap()},
        "request":{"native_context":{"session_id":"model-data"},"operation":{"kind":"get_context"}}});
    let read = |value: &Value| AgentToolCall::read(value.to_string().as_bytes());
    let parsed = read(&call).unwrap();
    assert_eq!(parsed.native_context.session_id, "native");
    assert_eq!(parsed.request["native_context"]["session_id"], "model-data");
    let mut bad = call.clone();
    bad["schema_version"] = json!(2);
    assert!(read(&bad).is_err());
    let mut bad = call.clone();
    bad["database"] = json!("foreign.db");
    assert!(read(&bad).is_err());
    let mut bad = call.clone();
    bad["request"] = json!({"text":"x".repeat(AGENT_TOOL_REQUEST_LIMIT)});
    assert!(read(&bad).is_err());
    assert!(AgentToolCall::read(vec![b' '; AGENT_TOOL_CALL_LIMIT + 1].as_slice()).is_err());
    let failure = AgentToolCall::read(br#"{"password":"private-fixture"}"#.as_slice()).unwrap_err();
    assert!(!failure.to_string().contains("private-fixture"));
}

#[test]
fn native_completion_requires_owned_handler_and_is_absent_from_legacy_specs() {
    let original = spec();
    let encoded = serde_json::to_value(&original).unwrap();
    assert!(encoded.get("native_completion_bridge").is_none());
    let decoded: AgentToolSpec = serde_json::from_value(encoded).unwrap();
    assert!(!decoded.native_completion_bridge);
    let default = render_agent_tool_plugin("claude", &original).unwrap();
    assert!(!default.files()["hooks/register.js"].contains("on('classic.Stop'"));
    let mut opted_in = original;
    opted_in.native_completion_bridge = true;
    assert!(render_agent_tool_plugin("claude", &opted_in).is_err());
    let plugin = render_with_session_handler("claude", &opted_in, Some(&opted_in.handler)).unwrap();
    assert!(plugin.files()["hooks/register.js"].contains("on('classic.Stop'"));
    assert!(plugin.files()["hooks/register.js"].contains(".catch(() => ({ block:"));
}
