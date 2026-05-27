#[cfg(feature = "testing")]
#[test]
fn with_mock_returns_queued_response_and_captures_prompt() {
    let (runner, captured) =
        aikit_sdk::AgentRunner::with_mock(vec![Ok("mock response".to_string())]);
    let runner = runner.agent("unused-in-mock");
    let result = runner.run("test prompt");
    assert_eq!(result.unwrap(), "mock response");
    assert_eq!(*captured.lock().unwrap(), vec!["test prompt".to_string()]);
}

#[cfg(feature = "testing")]
#[test]
fn captured_prompts_type_alias_resolves() {
    let (_runner, captured) = aikit_sdk::AgentRunner::with_mock(vec![Ok("ok".to_string())]);
    let _typed: aikit_sdk::CapturedPrompts = captured;
}

#[cfg(feature = "testing")]
#[test]
fn with_mock_error_injection_propagates() {
    use aikit_sdk::PipelineError;
    let (runner, _) =
        aikit_sdk::AgentRunner::with_mock(vec![Err(PipelineError::ValidationFailed {
            raw_output: "bad".to_string(),
            errors: vec!["e".to_string()],
        })]);
    let result = runner.run("any");
    assert!(matches!(
        result,
        Err(PipelineError::ValidationFailed { .. })
    ));
}
