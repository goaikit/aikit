//! The agent loop driven by scripted inputs: no network, no tools, no clock.
//!
//! These are the loop tests of `aikit-agent` (`loop_runner`), restated against
//! the step machine.

use aikit_agent_core::{
    hook_names, AgentCore, AgentEvent, ContextToolCall, ContextToolResult, CoreConfig, Effect,
    FunctionDefinition, HookAction, HookPhase, Input, InputError, LlmError, LlmResponse,
    LlmStreamEvent, LlmUsage, Outcome, RunError, TokenBudget, ToolCall, ToolCallFunction,
    ToolDefinition, ToolOutcome, ToolTiming, Turn,
};
use serde_json::json;

// ---- helpers ---------------------------------------------------------------

fn tool(name: &str) -> ToolDefinition {
    ToolDefinition {
        tool_type: "function".to_string(),
        function: FunctionDefinition {
            name: name.to_string(),
            description: Some(format!("the {name} tool")),
            parameters: json!({"type": "object"}),
        },
    }
}

fn config(stream: bool, tools: &[&str]) -> CoreConfig {
    CoreConfig {
        model: "test-model".to_string(),
        stream,
        max_iterations: 10,
        token_budget: TokenBudget::with_total(100_000),
        system_instructions: "You are a test agent.".to_string(),
        tools: tools.iter().map(|t| tool(t)).collect(),
    }
}

fn core(stream: bool, tools: &[&str], prompt: &str) -> AgentCore {
    AgentCore::new(config(stream, tools), vec![Turn::user(prompt)])
}

fn text_response(text: &str) -> LlmResponse {
    LlmResponse {
        content: Some(text.to_string()),
        tool_calls: vec![],
        finish_reason: Some("stop".to_string()),
        usage: None,
        model: None,
    }
}

fn tool_response(calls: &[(&str, &str, &str)]) -> LlmResponse {
    LlmResponse {
        content: None,
        tool_calls: calls
            .iter()
            .map(|(id, name, args)| ToolCall {
                id: id.to_string(),
                call_type: Some("function".to_string()),
                function: ToolCallFunction {
                    name: name.to_string(),
                    arguments: args.to_string(),
                },
            })
            .collect(),
        finish_reason: Some("tool_calls".to_string()),
        usage: None,
        model: None,
    }
}

fn line(data: serde_json::Value) -> Input {
    Input::ModelLine {
        line: format!("data: {data}\n"),
    }
}

fn feed(core: &mut AgentCore, input: Input) -> Vec<Effect> {
    core.on(input).expect("input accepted")
}

fn events(effects: &[Effect]) -> Vec<AgentEvent> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Emit { event } => Some(event.clone()),
            _ => None,
        })
        .collect()
}

fn names(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .map(|e| match e {
            Effect::Emit { event } => {
                let v = serde_json::to_value(event).unwrap();
                format!("emit:{}", v["type"].as_str().unwrap())
            }
            Effect::CallModel { .. } => "call_model".to_string(),
            Effect::CallTool { .. } => "call_tool".to_string(),
            Effect::Done { .. } => "done".to_string(),
        })
        .collect()
}

fn outcome(effects: &[Effect]) -> &Outcome {
    match effects.last() {
        Some(Effect::Done { outcome }) => outcome,
        other => panic!("expected Done last, got {other:?}"),
    }
}

/// Every batch hands at most one request to the host, as its last effect.
fn assert_one_request_last(effects: &[Effect]) {
    let requests = effects
        .iter()
        .filter(|e| matches!(e, Effect::CallModel { .. } | Effect::CallTool { .. }))
        .count();
    assert!(requests <= 1, "more than one request: {effects:?}");
    if requests == 1 {
        assert!(matches!(
            effects.last(),
            Some(Effect::CallModel { .. } | Effect::CallTool { .. })
        ));
    }
}

// ---- runs ------------------------------------------------------------------

#[test]
fn non_streaming_completes() {
    let mut c = core(false, &[], "Say hello");
    let start = c.start();
    assert_eq!(names(&start), ["call_model"]);
    let Effect::CallModel { request } = &start[0] else {
        unreachable!()
    };
    assert!(!request.stream);
    assert_eq!(request.model, "test-model");
    assert!(request.tools.is_empty());
    assert!(request.tool_choice.is_none());
    let roles: Vec<_> = request.messages.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(roles, ["system", "user"]);

    let mut resp = text_response("Hello!");
    resp.usage = Some(LlmUsage {
        input_tokens: 10,
        output_tokens: 3,
        total_tokens: Some(13),
    });
    let out = feed(&mut c, Input::ModelResponse { response: resp });
    assert_eq!(
        names(&out),
        [
            "emit:text_final",
            "emit:token_usage",
            "emit:step_finish",
            "done"
        ]
    );
    assert!(matches!(outcome(&out), Outcome::Completed));
    assert!(c.is_finished());
    assert!(
        matches!(&events(&out)[0], AgentEvent::TextFinal { content, .. } if content == "Hello!")
    );
    assert!(
        matches!(&events(&out)[2], AgentEvent::StepFinish { iteration: 0, finish_reason } if finish_reason == "stop")
    );
    let last = c.conversation().last().unwrap();
    assert_eq!(last.content, "Hello!");
}

#[test]
fn streaming_emits_deltas_line_by_line() {
    let mut c = core(true, &[], "Say hello");
    let start = c.start();
    let Effect::CallModel { request } = &start[0] else {
        panic!("expected CallModel")
    };
    assert!(request.stream);

    let a = feed(
        &mut c,
        line(json!({"choices":[{"delta":{"content":"Hel"},"finish_reason":null}]})),
    );
    assert!(matches!(&events(&a)[..], [AgentEvent::TextDelta { content, .. }] if content == "Hel"));
    let b = feed(
        &mut c,
        line(json!({"choices":[{"delta":{"content":"lo"},"finish_reason":null}]})),
    );
    assert!(matches!(&events(&b)[..], [AgentEvent::TextDelta { content, .. }] if content == "lo"));
    let c3 = feed(
        &mut c,
        line(
            json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}}),
        ),
    );
    assert!(c3.is_empty());
    // Comments, blank lines and what follows [DONE] carry nothing.
    for l in [
        ": keep-alive",
        "",
        "data: [DONE]",
        "data: not-json-after-done",
    ] {
        assert!(feed(
            &mut c,
            Input::ModelLine {
                line: l.to_string()
            }
        )
        .is_empty());
    }

    let end = feed(&mut c, Input::ModelEnd);
    assert_eq!(
        names(&end),
        [
            "emit:text_final",
            "emit:token_usage",
            "emit:step_finish",
            "done"
        ]
    );
    assert!(
        matches!(&events(&end)[0], AgentEvent::TextFinal { content, .. } if content == "Hello")
    );
    assert!(matches!(
        &events(&end)[1],
        AgentEvent::TokenUsage {
            input_tokens: 5,
            output_tokens: 2,
            total_tokens: Some(7)
        }
    ));
    assert!(matches!(outcome(&end), Outcome::Completed));
}

#[test]
fn parsed_stream_events_work_like_lines() {
    let mut c = core(true, &[], "hi");
    c.start();
    let out = feed(
        &mut c,
        Input::ModelEvent {
            event: LlmStreamEvent::TextDelta {
                content: "hey".to_string(),
            },
        },
    );
    assert_eq!(names(&out), ["emit:text_delta"]);
    feed(
        &mut c,
        Input::ModelEvent {
            event: LlmStreamEvent::Completed {
                finish_reason: "stop".to_string(),
                usage: None,
            },
        },
    );
    let end = feed(&mut c, Input::ModelEnd);
    assert_eq!(names(&end), ["emit:text_final", "emit:step_finish", "done"]);
}

#[test]
fn two_iteration_tool_use_non_streaming() {
    let mut c = core(false, &["read_file"], "Read test.txt");
    c.start();

    let out = feed(
        &mut c,
        Input::ModelResponse {
            response: tool_response(&[("c1", "read_file", r#"{"path": "test.txt"}"#)]),
        },
    );
    assert_eq!(
        names(&out),
        ["emit:text_final", "emit:tool_use", "call_tool"]
    );
    assert!(
        matches!(&events(&out)[1], AgentEvent::ToolUse { tool_name, call_id, tool_input }
        if tool_name == "read_file" && call_id == "c1" && tool_input["path"] == "test.txt")
    );
    let Some(Effect::CallTool {
        call_id,
        name,
        arguments,
    }) = out.last()
    else {
        unreachable!()
    };
    assert_eq!((call_id.as_str(), name.as_str()), ("c1", "read_file"));
    assert_eq!(arguments, &json!({"path": "test.txt"}));

    let out = feed(
        &mut c,
        Input::ToolResult {
            call_id: "c1".to_string(),
            result: ToolOutcome::ok("file contents"),
            timing: None,
        },
    );
    assert_eq!(
        names(&out),
        ["emit:tool_result", "emit:step_finish", "call_model"]
    );
    assert!(
        matches!(&events(&out)[1], AgentEvent::StepFinish { iteration: 0, finish_reason } if finish_reason == "tool_calls")
    );
    let Some(Effect::CallModel { request }) = out.last() else {
        unreachable!()
    };
    let roles: Vec<_> = request.messages.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(roles, ["system", "user", "assistant", "tool"]);
    let assistant = &request.messages[2];
    let calls = assistant.tool_calls.as_ref().expect("assistant tool calls");
    assert_eq!(calls[0].id, "c1");
    assert_eq!(calls[0].function.name, "read_file");
    assert_eq!(request.messages[3].tool_call_id.as_deref(), Some("c1"));
    assert_eq!(
        request.messages[3].content.as_deref(),
        Some("file contents")
    );
    assert_eq!(c.iteration(), 1);

    let out = feed(
        &mut c,
        Input::ModelResponse {
            response: text_response("Done reading file"),
        },
    );
    assert!(
        matches!(&events(&out)[0], AgentEvent::TextFinal { content, .. } if content == "Done reading file")
    );
    assert!(matches!(
        &events(&out)[1],
        AgentEvent::StepFinish { iteration: 1, .. }
    ));
    assert!(matches!(outcome(&out), Outcome::Completed));
}

#[test]
fn two_iteration_tool_use_streaming() {
    let mut c = core(true, &["read_file"], "Read test.txt");
    c.start();

    // The call's arguments arrive in two pieces; the second has no id.
    feed(
        &mut c,
        line(
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c2","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}),
        ),
    );
    feed(
        &mut c,
        line(
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\": \"test.txt\"}"}}]}}]}),
        ),
    );
    feed(
        &mut c,
        line(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]})),
    );
    let out = feed(&mut c, Input::ModelEnd);
    assert_eq!(
        names(&out),
        ["emit:text_final", "emit:tool_use", "call_tool"]
    );
    let Some(Effect::CallTool {
        call_id, arguments, ..
    }) = out.last()
    else {
        unreachable!()
    };
    assert_eq!(call_id, "c2");
    assert_eq!(arguments, &json!({"path": "test.txt"}));

    let out = feed(
        &mut c,
        Input::ToolResult {
            call_id: "c2".to_string(),
            result: ToolOutcome::ok("contents"),
            timing: None,
        },
    );
    assert!(matches!(out.last(), Some(Effect::CallModel { request }) if request.stream));

    feed(
        &mut c,
        line(json!({"choices":[{"delta":{"content":"Done reading file"},"finish_reason":"stop"}]})),
    );
    let out = feed(&mut c, Input::ModelEnd);
    assert!(
        matches!(&events(&out)[0], AgentEvent::TextFinal { content, .. } if content == "Done reading file")
    );
    assert!(matches!(outcome(&out), Outcome::Completed));
}

#[test]
fn several_tool_calls_run_one_after_the_other() {
    let mut c = core(false, &["a", "b"], "go");
    c.start();
    let out = feed(
        &mut c,
        Input::ModelResponse {
            response: tool_response(&[("c1", "a", "{}"), ("c2", "b", "not json")]),
        },
    );
    assert!(matches!(out.last(), Some(Effect::CallTool { call_id, .. }) if call_id == "c1"));
    let out = feed(
        &mut c,
        Input::ToolResult {
            call_id: "c1".to_string(),
            result: ToolOutcome::ok("one"),
            timing: None,
        },
    );
    assert_eq!(
        names(&out),
        ["emit:tool_result", "emit:tool_use", "call_tool"]
    );
    // Arguments that aren't JSON reach the tool as null, as before.
    assert!(
        matches!(out.last(), Some(Effect::CallTool { call_id, arguments, .. })
        if call_id == "c2" && arguments.is_null())
    );
    let out = feed(
        &mut c,
        Input::ToolResult {
            call_id: "c2".to_string(),
            result: ToolOutcome::err("two failed"),
            timing: None,
        },
    );
    assert!(
        matches!(&events(&out)[0], AgentEvent::ToolResult { is_error: true, output, .. } if output == "two failed")
    );
    let Some(Effect::CallModel { request }) = out.last() else {
        panic!("expected the next model call")
    };
    let tool_msgs: Vec<_> = request
        .messages
        .iter()
        .filter(|m| m.role == "tool")
        .map(|m| m.tool_call_id.clone().unwrap())
        .collect();
    assert_eq!(tool_msgs, ["c1", "c2"]);
}

#[test]
fn max_iterations_exceeded() {
    let mut cfg = config(false, &["read_file"]);
    cfg.max_iterations = 1;
    let mut c = AgentCore::new(cfg, vec![Turn::user("test")]);
    c.start();
    feed(
        &mut c,
        Input::ModelResponse {
            response: tool_response(&[("call-1", "read_file", r#"{"path": "nonexistent"}"#)]),
        },
    );
    let out = feed(
        &mut c,
        Input::ToolResult {
            call_id: "call-1".to_string(),
            result: ToolOutcome::err("not found"),
            timing: None,
        },
    );
    assert_eq!(
        names(&out),
        ["emit:tool_result", "emit:step_finish", "emit:error", "done"]
    );
    assert!(
        matches!(&events(&out)[2], AgentEvent::Error { code, message }
        if code == "E_AIKIT_MAX_ITERATIONS" && message == "exceeded max iterations (1)")
    );
    assert!(matches!(
        outcome(&out),
        Outcome::Failed {
            error: RunError::MaxIterations { max: 1 }
        }
    ));
}

#[test]
fn last_iteration_without_tool_calls_completes() {
    let mut cfg = config(false, &[]);
    cfg.max_iterations = 1;
    let mut c = AgentCore::new(cfg, vec![Turn::user("test")]);
    c.start();
    let mut resp = text_response("cut off");
    resp.finish_reason = Some("length".to_string());
    let out = feed(&mut c, Input::ModelResponse { response: resp });
    assert!(matches!(outcome(&out), Outcome::Completed));
}

#[test]
fn a_step_that_does_not_stop_asks_again() {
    let mut c = core(true, &[], "hi");
    c.start();
    // A stream that ends without a finish reason is not done.
    feed(
        &mut c,
        line(json!({"choices":[{"delta":{"content":"part"}}]})),
    );
    let out = feed(&mut c, Input::ModelEnd);
    assert!(
        matches!(&events(&out)[1], AgentEvent::StepFinish { finish_reason, .. } if finish_reason == "unknown")
    );
    assert!(matches!(out.last(), Some(Effect::CallModel { .. })));
}

#[test]
fn max_iterations_zero_finishes_at_once() {
    let mut cfg = config(false, &[]);
    cfg.max_iterations = 0;
    let mut c = AgentCore::new(cfg, vec![Turn::user("test")]);
    let out = c.start();
    assert_eq!(names(&out), ["done"]);
    assert!(matches!(outcome(&out), Outcome::Completed));
}

#[test]
fn start_twice_does_nothing_the_second_time() {
    let mut c = core(false, &[], "hi");
    assert_eq!(c.start().len(), 1);
    assert!(c.start().is_empty());
}

// ---- tools the model was not offered ------------------------------------------

#[test]
fn unknown_tool_is_a_blocked_dispatch_hook_before_its_error_result() {
    let mut c = core(false, &["read_file"], "try");
    c.start();
    let out = feed(&mut c, Input::Now { unix_ms: 5_000 });
    assert!(out.is_empty());
    let out = feed(
        &mut c,
        Input::ModelResponse {
            response: tool_response(&[("c1", "no_such_tool", "{}")]),
        },
    );
    // The host is never asked to run it.
    assert_eq!(
        names(&out),
        [
            "emit:text_final",
            "emit:tool_use",
            "emit:hook",
            "emit:tool_result",
            "emit:step_finish",
            "call_model"
        ]
    );
    let ev = events(&out);
    assert!(matches!(&ev[2], AgentEvent::Hook {
        phase: HookPhase::BeforeTool,
        hook_name,
        action: HookAction::Blocked,
        payload: Some(p),
    } if hook_name == hook_names::TOOL_DISPATCH
        && p["tool_name"] == "no_such_tool"
        && p["call_id"] == "c1"
        && p["reason"] == "unknown_tool"));
    assert!(matches!(&ev[3], AgentEvent::ToolResult {
        call_id, output, is_error: true, duration_ms: 0, started_at_ms: 5_000
    } if call_id == "c1" && output == "unknown tool: no_such_tool"));
    let Some(Effect::CallModel { request }) = out.last() else {
        unreachable!()
    };
    let tool_msg = request.messages.last().unwrap();
    assert_eq!(tool_msg.role, "tool");
    assert_eq!(
        tool_msg.content.as_deref(),
        Some("unknown tool: no_such_tool")
    );
}

#[test]
fn a_known_tool_fires_no_dispatch_hook() {
    let mut c = core(false, &["read_file"], "read it");
    c.start();
    let out = feed(
        &mut c,
        Input::ModelResponse {
            response: tool_response(&[("c1", "read_file", "{}")]),
        },
    );
    assert!(!events(&out)
        .iter()
        .any(|e| matches!(e, AgentEvent::Hook { .. })));
}

#[test]
fn an_unknown_call_before_a_known_one_is_answered_in_order() {
    let mut c = core(false, &["read_file"], "go");
    c.start();
    let out = feed(
        &mut c,
        Input::ModelResponse {
            response: tool_response(&[("c1", "nope", "{}"), ("c2", "read_file", "{}")]),
        },
    );
    assert_eq!(
        names(&out),
        [
            "emit:text_final",
            "emit:tool_use",
            "emit:hook",
            "emit:tool_result",
            "emit:tool_use",
            "call_tool"
        ]
    );
    assert!(matches!(out.last(), Some(Effect::CallTool { call_id, .. }) if call_id == "c2"));
}

// ---- failures ----------------------------------------------------------------

#[test]
fn stream_protocol_error() {
    let mut c = core(true, &[], "test");
    c.start();
    feed(
        &mut c,
        line(json!({"choices":[{"delta":{"content":"ok"}}]})),
    );
    let out = feed(
        &mut c,
        Input::ModelLine {
            line: "data: {malformed".to_string(),
        },
    );
    assert_eq!(names(&out), ["emit:error", "done"]);
    assert!(
        matches!(&events(&out)[0], AgentEvent::Error { code, .. } if code == "E_AIKIT_STREAM_PROTOCOL")
    );
    match outcome(&out) {
        Outcome::Failed {
            error:
                RunError::Model {
                    error: LlmError::StreamProtocol { line, detail },
                },
        } => {
            assert_eq!(*line, 2);
            assert!(detail.contains("JSON parse error"));
        }
        other => panic!("expected a stream protocol failure, got {other:?}"),
    }
}

#[test]
fn model_error_fails_the_run() {
    let mut c = core(false, &[], "test");
    c.start();
    let error = LlmError::ErrorResponse {
        status: 500,
        url: "https://example.test/chat/completions".to_string(),
        body: "boom".to_string(),
    };
    let out = feed(&mut c, Input::ModelError { error });
    assert!(
        matches!(&events(&out)[0], AgentEvent::Error { code, message }
        if code == "E_AIKIT_LLM_ERROR_RESPONSE" && message.contains("HTTP 500"))
    );
    assert!(matches!(
        outcome(&out),
        Outcome::Failed {
            error: RunError::Model {
                error: LlmError::ErrorResponse { status: 500, .. }
            }
        }
    ));
}

#[test]
fn provider_error_in_the_stream_fails_the_run() {
    let mut c = core(true, &[], "test");
    c.start();
    let out = feed(
        &mut c,
        Input::ModelEvent {
            event: LlmStreamEvent::ProviderError {
                code: "rate_limited".to_string(),
                message: "slow down".to_string(),
            },
        },
    );
    assert_eq!(names(&out), ["emit:error", "done"]);
    match outcome(&out) {
        Outcome::Failed { error } => {
            assert!(
                matches!(error, RunError::Provider { code, message } if code == "rate_limited" && message == "slow down")
            );
            assert_eq!(error.to_string(), "rate_limited: slow down");
        }
        other => panic!("expected a provider failure, got {other:?}"),
    }
}

// ---- context -----------------------------------------------------------------

#[test]
fn prior_turns_are_sent_in_order() {
    let turns = vec![
        Turn::user("first question"),
        Turn::assistant_with_tool_calls(
            "looking",
            vec![ContextToolCall {
                id: "old-1".to_string(),
                name: "read_file".to_string(),
                arguments: "{}".to_string(),
            }],
        ),
        Turn::tool_result(vec![ContextToolResult {
            call_id: "old-1".to_string(),
            output: "old output".to_string(),
            is_error: false,
        }]),
        Turn::assistant("first answer"),
        Turn::user("second question"),
    ];
    let mut c = AgentCore::new(config(false, &[]), turns);
    let start = c.start();
    let Effect::CallModel { request } = &start[0] else {
        unreachable!()
    };
    let shape: Vec<_> = request
        .messages
        .iter()
        .map(|m| (m.role.as_str(), m.content.clone().unwrap_or_default()))
        .collect();
    assert_eq!(
        shape,
        [
            ("system", "You are a test agent.".to_string()),
            ("user", "first question".to_string()),
            ("assistant", "looking".to_string()),
            ("tool", "old output".to_string()),
            ("assistant", "first answer".to_string()),
            ("user", "second question".to_string()),
        ]
    );
    assert_eq!(
        request.messages[2].tool_calls.as_ref().unwrap()[0].id,
        "old-1"
    );
}

#[test]
fn an_over_budget_conversation_is_compressed_before_the_call() {
    let mut cfg = config(false, &[]);
    cfg.token_budget = TokenBudget::with_total(3_100); // 100 tokens available
    let long = "word ".repeat(400);
    let turns = vec![
        Turn::user(long.clone()),
        Turn::assistant(long.clone()),
        Turn::user("now"),
    ];
    let mut c = AgentCore::new(cfg, turns);
    let out = c.start();
    assert_eq!(names(&out), ["emit:context_compressed", "call_model"]);
    assert!(
        matches!(&events(&out)[0], AgentEvent::ContextCompressed { turns_summarized: 2, original_tokens, compressed_tokens }
        if compressed_tokens < original_tokens)
    );
    assert_eq!(c.conversation().len(), 2);
    assert_eq!(c.conversation()[1].content, "now");
}

#[test]
fn tool_timing_comes_from_now_inputs() {
    let mut c = core(false, &["read_file"], "read it");
    c.start();
    feed(&mut c, Input::Now { unix_ms: 1_000 });
    feed(
        &mut c,
        Input::ModelResponse {
            response: tool_response(&[("c1", "read_file", "{}")]),
        },
    );
    feed(&mut c, Input::Now { unix_ms: 1_250 });
    let out = feed(
        &mut c,
        Input::ToolResult {
            call_id: "c1".to_string(),
            result: ToolOutcome::ok("x"),
            timing: None,
        },
    );
    assert!(matches!(
        &events(&out)[0],
        AgentEvent::ToolResult {
            started_at_ms: 1_000,
            duration_ms: 250,
            ..
        }
    ));
}

#[test]
fn tool_timing_from_the_host_wins() {
    let mut c = core(false, &["read_file"], "read it");
    c.start();
    feed(
        &mut c,
        Input::ModelResponse {
            response: tool_response(&[("c1", "read_file", "{}")]),
        },
    );
    let out = feed(
        &mut c,
        Input::ToolResult {
            call_id: "c1".to_string(),
            result: ToolOutcome::ok("x"),
            timing: Some(ToolTiming {
                started_at_ms: 1_700_000_000_000,
                duration_ms: 42,
            }),
        },
    );
    assert!(matches!(
        &events(&out)[0],
        AgentEvent::ToolResult {
            started_at_ms: 1_700_000_000_000,
            duration_ms: 42,
            ..
        }
    ));
}

// ---- protocol ----------------------------------------------------------------

#[test]
fn inputs_the_state_cannot_take_are_refused_without_changing_it() {
    let mut c = core(false, &["read_file"], "go");
    // Before start.
    assert!(matches!(
        c.on(Input::ModelEnd),
        Err(InputError::Unexpected { .. })
    ));
    c.start();
    // A tool result while the model is answering.
    let err = c
        .on(Input::ToolResult {
            call_id: "c1".to_string(),
            result: ToolOutcome::ok("x"),
            timing: None,
        })
        .unwrap_err();
    assert_eq!(
        err,
        InputError::Unexpected {
            input: "tool_result".to_string(),
            state: "waiting for the model".to_string()
        }
    );
    feed(
        &mut c,
        Input::ModelResponse {
            response: tool_response(&[("c1", "read_file", "{}")]),
        },
    );
    // A result for a call the core didn't ask for.
    let err = c
        .on(Input::ToolResult {
            call_id: "c9".to_string(),
            result: ToolOutcome::ok("x"),
            timing: None,
        })
        .unwrap_err();
    assert_eq!(
        err,
        InputError::WrongCall {
            call_id: "c9".to_string(),
            expected: "c1".to_string()
        }
    );
    // A model answer while a tool runs.
    assert!(c
        .on(Input::ModelResponse {
            response: text_response("x"),
        })
        .is_err());
    // The right result is still taken.
    let out = feed(
        &mut c,
        Input::ToolResult {
            call_id: "c1".to_string(),
            result: ToolOutcome::ok("x"),
            timing: None,
        },
    );
    assert!(matches!(out.last(), Some(Effect::CallModel { .. })));
}

#[test]
fn nothing_but_now_is_taken_after_done() {
    let mut c = core(false, &[], "hi");
    c.start();
    feed(
        &mut c,
        Input::ModelResponse {
            response: text_response("bye"),
        },
    );
    assert!(c.is_finished());
    assert!(c.on(Input::Now { unix_ms: 1 }).unwrap().is_empty());
    assert!(matches!(
        c.on(Input::Cancel),
        Err(InputError::Unexpected { state, .. }) if state == "finished"
    ));
    assert!(c.on(Input::ModelEnd).is_err());
}

#[test]
fn cancel_ends_the_run() {
    let mut c = core(false, &["read_file"], "go");
    c.start();
    feed(
        &mut c,
        Input::ModelResponse {
            response: tool_response(&[("c1", "read_file", "{}")]),
        },
    );
    let out = feed(&mut c, Input::Cancel);
    assert_eq!(names(&out), ["done"]);
    assert!(matches!(outcome(&out), Outcome::Cancelled));
    assert!(c.is_finished());
}

#[test]
fn every_batch_ends_with_its_only_request() {
    let mut c = core(false, &["a"], "go");
    let mut batches = vec![c.start()];
    batches.push(feed(
        &mut c,
        Input::ModelResponse {
            response: tool_response(&[("c1", "nope", "{}"), ("c2", "a", "{}"), ("c3", "a", "{}")]),
        },
    ));
    for id in ["c2", "c3"] {
        batches.push(feed(
            &mut c,
            Input::ToolResult {
                call_id: id.to_string(),
                result: ToolOutcome::ok("ok"),
                timing: None,
            },
        ));
    }
    batches.push(feed(
        &mut c,
        Input::ModelResponse {
            response: text_response("done"),
        },
    ));
    for b in &batches {
        assert_one_request_last(b);
    }
}

// ---- serde -------------------------------------------------------------------

#[test]
fn inputs_read_from_json() {
    let inputs: Vec<Input> = serde_json::from_value(json!([
        {"input": "model_line", "line": "data: [DONE]"},
        {"input": "model_event", "event": {"type": "text_delta", "content": "hi"}},
        {"input": "model_end"},
        {"input": "model_response", "response": {"content": "hi", "tool_calls": [], "finish_reason": "stop", "usage": null, "model": null}},
        {"input": "model_error", "error": {"kind": "request_failed", "message": "offline"}},
        {"input": "tool_result", "call_id": "c1", "result": {"content": "ok", "is_error": false}},
        {"input": "tool_result", "call_id": "c1", "result": {"content": "ok", "is_error": false},
         "timing": {"started_at_ms": 1, "duration_ms": 2}},
        {"input": "now", "unix_ms": 1760000000000_i64},
        {"input": "cancel"}
    ]))
    .unwrap();
    assert_eq!(inputs.len(), 9);
    assert!(
        matches!(&inputs[4], Input::ModelError { error: LlmError::RequestFailed { message } } if message == "offline")
    );
    assert!(matches!(&inputs[5], Input::ToolResult { timing: None, .. }));
    assert!(matches!(
        &inputs[6],
        Input::ToolResult {
            timing: Some(ToolTiming {
                started_at_ms: 1,
                duration_ms: 2
            }),
            ..
        }
    ));
}

#[test]
fn effects_and_inputs_round_trip_through_json() {
    let mut c = core(false, &["read_file"], "go");
    let mut all = c.start();
    let inputs = vec![
        Input::Now { unix_ms: 10 },
        Input::ModelResponse {
            response: tool_response(&[("c0", "nope", "{}"), ("c1", "read_file", r#"{"p":1}"#)]),
        },
        Input::ToolResult {
            call_id: "c1".to_string(),
            result: ToolOutcome::ok("x"),
            timing: None,
        },
        Input::ModelError {
            error: LlmError::RequestFailed {
                message: "offline".to_string(),
            },
        },
    ];
    for input in inputs {
        let json = serde_json::to_string(&input).unwrap();
        let back: Input = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
        all.extend(feed(&mut c, back));
    }
    let tags: Vec<String> = all
        .iter()
        .map(|e| {
            let v = serde_json::to_value(e).unwrap();
            let back: Effect = serde_json::from_value(v.clone()).unwrap();
            assert_eq!(serde_json::to_value(&back).unwrap(), v);
            v["effect"].as_str().unwrap().to_string()
        })
        .collect();
    for tag in ["call_model", "call_tool", "emit", "done"] {
        assert!(tags.iter().any(|t| t == tag), "no {tag} effect in {tags:?}");
    }
    let done = serde_json::to_value(all.last().unwrap()).unwrap();
    assert_eq!(done["outcome"]["status"], "failed");
    assert_eq!(done["outcome"]["error"]["kind"], "model");
    assert_eq!(done["outcome"]["error"]["error"]["kind"], "request_failed");
}

#[test]
fn call_tool_json_shape() {
    let e = Effect::CallTool {
        call_id: "c1".to_string(),
        name: "read_file".to_string(),
        arguments: json!({"path": "a"}),
    };
    assert_eq!(
        serde_json::to_value(&e).unwrap(),
        json!({"effect": "call_tool", "call_id": "c1", "name": "read_file", "arguments": {"path": "a"}})
    );
}

#[test]
fn a_saved_core_picks_up_where_it_left_off() {
    let mut c = core(true, &["read_file"], "go");
    c.start();
    feed(
        &mut c,
        line(json!({"choices":[{"delta":{"content":"half "}}]})),
    );
    let saved = serde_json::to_string(&c).unwrap();
    let mut c: AgentCore = serde_json::from_str(&saved).unwrap();
    feed(
        &mut c,
        line(json!({"choices":[{"delta":{"content":"whole"},"finish_reason":"stop"}]})),
    );
    let out = feed(&mut c, Input::ModelEnd);
    assert!(
        matches!(&events(&out)[0], AgentEvent::TextFinal { content, .. } if content == "half whole")
    );
    // The parser's line count survived too.
    assert!(matches!(outcome(&out), Outcome::Completed));
}

// ---- the request -------------------------------------------------------------

#[test]
fn the_request_carries_no_endpoint_or_key_until_the_host_adds_them() {
    let mut c = core(true, &["read_file"], "go");
    let start = c.start();
    let Effect::CallModel { request } = &start[0] else {
        unreachable!()
    };
    let v = serde_json::to_value(request).unwrap();
    assert!(v.get("base_url").is_none() && v.get("api_key").is_none());
    assert_eq!(v["tool_choice"], "auto");

    let body: serde_json::Value = serde_json::from_str(&request.wire_body_json()).unwrap();
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    assert_eq!(body["tools"][0]["function"]["name"], "read_file");
    assert_eq!(body["messages"][0]["role"], "system");

    let req = request
        .clone()
        .into_llm_request("https://llm.example".to_string(), "k".to_string());
    assert_eq!(req.base_url, "https://llm.example");
    assert_eq!(req.api_key, "k");
    assert!(req.stream);
}
