use std::sync::Arc;

use crate::compression::maybe_compress;
use crate::config::AgentConfig;
use crate::context::{ContextPacket, ContextToolCall, ContextToolResult, TokenBudget, Turn};
use crate::errors::AgentError;
use crate::llm::gateway::LlmGateway;
use crate::llm::types::{LlmMessage, LlmRequest, LlmStreamEvent, LlmUsage, ToolCall};
use crate::skills::discover_skills;
use crate::subagents::SpawnSubagentTool;
use crate::tools::{
    GitTool, ReadFileTool, ReadSkillTool, RunBashTool, Tool, ToolContext, WriteFileTool,
};
use crate::AgentInternalEvent;

type LlmCallResult = (String, Vec<ToolCall>, Option<String>, Option<LlmUsage>);

pub fn run(
    config: AgentConfig,
    prompt: &str,
    gateway: Box<dyn LlmGateway>,
) -> Result<Vec<AgentInternalEvent>, AgentError> {
    let gateway: Arc<dyn LlmGateway> = Arc::from(gateway);
    run_inner(config, prompt, gateway)
}

pub(crate) fn run_inner(
    config: AgentConfig,
    prompt: &str,
    gateway: Arc<dyn LlmGateway>,
) -> Result<Vec<AgentInternalEvent>, AgentError> {
    let mut events = Vec::new();

    // 1. Load AGENTS.md if present
    let system_instructions = build_system_instructions(&config)?;

    // 2. Discover skills
    let skills = discover_skills(&config.skills_dirs);

    // 3. Build initial context packet
    let budget = TokenBudget {
        total_budget: config.context_budget_tokens,
        reserve_for_tools: 1000,
        reserve_for_output: 2000,
    };
    let mut context = ContextPacket::new(system_instructions, budget);
    context.skills_summary = skills.iter().map(|s| s.metadata.clone()).collect();
    context.add_turn(Turn::user(prompt));

    // 4. Build available tools
    let tools = build_tools(&config, Arc::clone(&gateway), &skills);

    // 5. Main agent loop
    for iteration in 0..config.max_iterations {
        // Check context budget and compress if needed
        if let Some(compression) =
            maybe_compress(&mut context).map_err(|e| AgentError::ContextCompression {
                message: e.to_string(),
            })?
        {
            events.push(AgentInternalEvent::ContextCompressed {
                original_tokens: compression.original_tokens,
                compressed_tokens: compression.compressed_tokens,
                turns_summarized: compression.turns_summarized,
            });
        }

        // Build LLM request
        let tool_schemas: Vec<_> = tools.iter().map(|t| t.schema()).collect();
        let req = build_llm_request(&config, &context, tool_schemas);

        // Call LLM
        let (response_text, tool_calls, finish_reason, usage) = if config.stream {
            call_stream(&*gateway, req, &mut events)?
        } else {
            call_complete(&*gateway, req, &mut events)?
        };

        // Emit usage event if present
        if let Some(u) = usage {
            events.push(AgentInternalEvent::TokenUsage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                total_tokens: u.total_tokens,
            });
        }

        // Add assistant turn to context
        if tool_calls.is_empty() {
            context.add_turn(Turn::assistant(&response_text));
        } else {
            let ctx_tool_calls: Vec<ContextToolCall> = tool_calls
                .iter()
                .map(|tc| ContextToolCall {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                })
                .collect();
            context.add_turn(Turn::assistant_with_tool_calls(
                &response_text,
                ctx_tool_calls,
            ));
        }

        // If finish reason is "stop" with no tool calls, we're done
        let done = finish_reason.as_deref() == Some("stop") && tool_calls.is_empty();

        // Execute tool calls
        if !tool_calls.is_empty() {
            let tool_ctx = ToolContext::new(config.workdir.clone(), config.allowed_roots.clone());

            let mut tool_results = Vec::new();
            for tc in &tool_calls {
                let call_id = tc.id.clone();
                let tool_name = tc.function.name.clone();
                let args: serde_json::Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);

                events.push(AgentInternalEvent::ToolUse {
                    tool_name: tool_name.clone(),
                    tool_input: args.clone(),
                    call_id: call_id.clone(),
                });

                let output = execute_tool(&tools, &tool_name, args, &tool_ctx);

                events.push(AgentInternalEvent::ToolResult {
                    call_id: call_id.clone(),
                    output: output.content.clone(),
                    is_error: output.is_error,
                });

                tool_results.push(ContextToolResult {
                    call_id,
                    output: output.content,
                    is_error: output.is_error,
                });
            }

            context.add_turn(Turn::tool_result(tool_results));
        }

        // Emit step finish
        events.push(AgentInternalEvent::StepFinish {
            iteration,
            finish_reason: finish_reason.unwrap_or_else(|| "unknown".to_string()),
        });

        if done {
            break;
        }

        // When we have exhausted iterations but still have pending tool calls, error
        if iteration + 1 >= config.max_iterations && !tool_calls.is_empty() {
            events.push(AgentInternalEvent::Error {
                code: "E_AIKIT_MAX_ITERATIONS".to_string(),
                message: format!("exceeded max iterations ({})", config.max_iterations),
            });
            return Err(AgentError::MaxIterations {
                max: config.max_iterations,
            });
        }
    }

    Ok(events)
}

fn build_system_instructions(config: &AgentConfig) -> Result<String, AgentError> {
    let mut parts = Vec::new();
    parts.push(
        "You are a helpful AI agent. Complete the requested task carefully and accurately."
            .to_string(),
    );

    if let Some(agents_md) = &config.agents_md_path {
        let content = std::fs::read_to_string(agents_md).map_err(|e| AgentError::AgentsMdRead {
            reason: e.to_string(),
        })?;
        parts.push(content);
    }

    Ok(parts.join("\n\n"))
}

fn build_tools(
    config: &AgentConfig,
    gateway: Arc<dyn LlmGateway>,
    skills: &[crate::skills::DiscoveredSkill],
) -> Vec<Box<dyn Tool>> {
    let mut tools: Vec<Box<dyn Tool>> = vec![
        Box::new(ReadFileTool),
        Box::new(WriteFileTool),
        Box::new(RunBashTool),
        Box::new(GitTool),
        Box::new(ReadSkillTool {
            skills: skills.to_vec(),
        }),
    ];

    // Only add spawn_subagent if max_subagent_depth > 0
    if config.max_subagent_depth > 0 {
        tools.push(Box::new(SpawnSubagentTool {
            parent_config: config.clone(),
            gateway,
        }));
    }

    tools
}

fn build_llm_request(
    config: &AgentConfig,
    context: &ContextPacket,
    tools: Vec<crate::llm::types::ToolDefinition>,
) -> LlmRequest {
    let mut messages = Vec::new();

    if !context.system_instructions.is_empty() {
        messages.push(LlmMessage {
            role: "system".to_string(),
            content: context.system_instructions.clone(),
        });
    }

    for turn in &context.conversation {
        match turn.role {
            crate::context::TurnRole::User => {
                messages.push(LlmMessage {
                    role: "user".to_string(),
                    content: turn.content.clone(),
                });
            }
            crate::context::TurnRole::Assistant => {
                messages.push(LlmMessage {
                    role: "assistant".to_string(),
                    content: turn.content.clone(),
                });
            }
            crate::context::TurnRole::Tool => {
                if let Some(results) = &turn.tool_results {
                    for result in results {
                        messages.push(LlmMessage {
                            role: "tool".to_string(),
                            content: result.output.clone(),
                        });
                    }
                }
            }
        }
    }

    let has_tools = !tools.is_empty();
    LlmRequest {
        model: config.model.clone(),
        base_url: config.base_url.clone(),
        api_key: config.api_key.clone(),
        messages,
        tools,
        tool_choice: if has_tools {
            Some(crate::llm::types::ToolChoice::auto())
        } else {
            None
        },
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: config.stream,
    }
}

fn call_complete(
    gateway: &dyn LlmGateway,
    req: LlmRequest,
    events: &mut Vec<AgentInternalEvent>,
) -> Result<LlmCallResult, AgentError> {
    let response = gateway.complete(req).map_err(|e| {
        let code = extract_error_code(&e);
        let msg = e.to_string();
        events.push(AgentInternalEvent::Error { code, message: msg });
        map_llm_error(e)
    })?;

    let content = response.content.unwrap_or_default();
    events.push(AgentInternalEvent::TextFinal {
        content: content.clone(),
        turn_id: None,
    });

    Ok((
        content,
        response.tool_calls,
        response.finish_reason,
        response.usage,
    ))
}

fn call_stream(
    gateway: &dyn LlmGateway,
    req: LlmRequest,
    events: &mut Vec<AgentInternalEvent>,
) -> Result<LlmCallResult, AgentError> {
    let handle = gateway.stream(req).map_err(|e| {
        let code = extract_error_code(&e);
        let msg = e.to_string();
        events.push(AgentInternalEvent::Error { code, message: msg });
        map_llm_error(e)
    })?;

    let mut full_text = String::new();
    let mut finish_reason: Option<String> = None;
    let mut usage: Option<LlmUsage> = None;
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut pending_tool: Option<PartialToolCall> = None;

    for event_result in handle {
        match event_result {
            Ok(event) => match event {
                LlmStreamEvent::TextDelta { content } => {
                    full_text.push_str(&content);
                    events.push(AgentInternalEvent::TextDelta {
                        content,
                        turn_id: None,
                    });
                }
                LlmStreamEvent::ToolCallDelta {
                    id,
                    function_name,
                    arguments_delta,
                } => {
                    if !id.is_empty() {
                        if let Some(prev) = pending_tool.take() {
                            tool_calls.push(prev.into_tool_call());
                        }
                        pending_tool = Some(PartialToolCall {
                            id,
                            function_name,
                            arguments: arguments_delta,
                        });
                    } else if let Some(ref mut tc) = pending_tool {
                        tc.arguments.push_str(&arguments_delta);
                        if !function_name.is_empty() && tc.function_name.is_empty() {
                            tc.function_name = function_name;
                        }
                    }
                }
                LlmStreamEvent::UsageUpdate { usage: u } => {
                    usage = Some(u);
                }
                LlmStreamEvent::Completed {
                    finish_reason: fr,
                    usage: u,
                } => {
                    finish_reason = Some(fr);
                    if let Some(u) = u {
                        usage = Some(u);
                    }
                }
                LlmStreamEvent::ProviderError { code, message } => {
                    events.push(AgentInternalEvent::Error {
                        code: code.clone(),
                        message: message.clone(),
                    });
                    return Err(AgentError::LlmRequestFailed {
                        message: format!("{}: {}", code, message),
                    });
                }
            },
            Err(e) => {
                let code = extract_error_code(&e);
                let msg = e.to_string();
                events.push(AgentInternalEvent::Error { code, message: msg });
                return Err(map_llm_error(e));
            }
        }
    }

    if let Some(tc) = pending_tool.take() {
        tool_calls.push(tc.into_tool_call());
    }

    events.push(AgentInternalEvent::TextFinal {
        content: full_text.clone(),
        turn_id: None,
    });

    Ok((full_text, tool_calls, finish_reason, usage))
}

struct PartialToolCall {
    id: String,
    function_name: String,
    arguments: String,
}

impl PartialToolCall {
    fn into_tool_call(self) -> ToolCall {
        ToolCall {
            id: self.id,
            call_type: Some("function".to_string()),
            function: crate::llm::types::ToolCallFunction {
                name: self.function_name,
                arguments: self.arguments,
            },
        }
    }
}

fn execute_tool(
    tools: &[Box<dyn Tool>],
    name: &str,
    input: serde_json::Value,
    ctx: &ToolContext,
) -> crate::tools::ToolOutput {
    if let Some(tool) = tools.iter().find(|t| t.name() == name) {
        tool.execute(input, ctx).unwrap_or_else(|e| {
            crate::tools::ToolOutput::err(format!("E_AIKIT_TOOL_EXEC_FAILED: {}", e))
        })
    } else {
        crate::tools::ToolOutput::err(format!("unknown tool: {}", name))
    }
}

fn map_llm_error(e: crate::llm::types::LlmError) -> AgentError {
    match e {
        crate::llm::types::LlmError::NoApiKey { checked } => AgentError::NoApiKey { checked },
        crate::llm::types::LlmError::RequestFailed { message } => {
            AgentError::LlmRequestFailed { message }
        }
        crate::llm::types::LlmError::ErrorResponse { status, url, body } => {
            AgentError::LlmErrorResponse { status, url, body }
        }
        crate::llm::types::LlmError::StreamProtocol { line, detail } => {
            AgentError::StreamProtocol { line, detail }
        }
    }
}

fn extract_error_code(e: &crate::llm::types::LlmError) -> String {
    match e {
        crate::llm::types::LlmError::NoApiKey { .. } => "E_AIKIT_NO_API_KEY".to_string(),
        crate::llm::types::LlmError::RequestFailed { .. } => {
            "E_AIKIT_LLM_REQUEST_FAILED".to_string()
        }
        crate::llm::types::LlmError::ErrorResponse { .. } => {
            "E_AIKIT_LLM_ERROR_RESPONSE".to_string()
        }
        crate::llm::types::LlmError::StreamProtocol { .. } => "E_AIKIT_STREAM_PROTOCOL".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::mock::{MockGateway, MockResponse};
    use crate::llm::types::{
        LlmError, LlmResponse, LlmStreamEvent, LlmStreamHandle, ToolCall, ToolCallFunction,
    };
    use tempfile::TempDir;

    fn make_config(tmp: &TempDir, stream: bool) -> AgentConfig {
        AgentConfig {
            model: "test-model".to_string(),
            base_url: "http://localhost".to_string(),
            api_key: "test-key".to_string(),
            stream,
            max_iterations: 3,
            max_subagent_depth: 2,
            context_budget_tokens: 12000,
            workdir: tmp.path().to_path_buf(),
            allowed_roots: vec![tmp.path().to_path_buf()],
            skills_dirs: vec![],
            agents_md_path: None,
            timeout_secs: 30,
            connect_timeout_secs: 5,
        }
    }

    #[test]
    fn test_aikit_agent_non_streaming_completes() {
        let tmp = TempDir::new().unwrap();
        let config = make_config(&tmp, false);
        let gw = MockGateway::new(vec![MockResponse::text("Hello, I am the agent!")]);
        let events = run(config, "Say hello", Box::new(gw)).unwrap();
        assert!(!events.is_empty());
        let has_text_final = events.iter().any(|e| {
            matches!(
                e,
                AgentInternalEvent::TextFinal { content, .. }
                if content == "Hello, I am the agent!"
            )
        });
        assert!(
            has_text_final,
            "should have TextFinal event with response text"
        );
    }

    #[test]
    fn test_aikit_agent_no_api_key_error() {
        let tmp = TempDir::new().unwrap();
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("AIKIT_API_KEY");
        let result = AgentConfig::from_env(tmp.path().to_path_buf(), false, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            AgentError::NoApiKey { .. } => {}
            e => panic!("expected NoApiKey, got {:?}", e),
        }
    }

    #[test]
    fn test_aikit_agent_streaming_emits_deltas() {
        let tmp = TempDir::new().unwrap();
        let config = make_config(&tmp, true);
        let gw = MockGateway::new(vec![MockResponse::text("Streaming response")]);
        let events = run(config, "test streaming", Box::new(gw)).unwrap();
        let has_delta = events
            .iter()
            .any(|e| matches!(e, AgentInternalEvent::TextDelta { .. }));
        assert!(has_delta, "streaming should emit TextDelta events");
    }

    #[test]
    fn test_aikit_agent_streaming_seq_monotonically_increasing() {
        let tmp = TempDir::new().unwrap();
        let config = make_config(&tmp, true);
        let gw = MockGateway::new(vec![MockResponse::text("hello world response text")]);
        let events = run(config, "test", Box::new(gw)).unwrap();
        assert!(!events.is_empty());
    }

    #[test]
    fn test_aikit_agent_stream_protocol_error() {
        let tmp = TempDir::new().unwrap();
        let config = make_config(&tmp, true);

        struct ErrorGateway;
        impl LlmGateway for ErrorGateway {
            fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
                Err(LlmError::StreamProtocol {
                    line: 3,
                    detail: "malformed JSON".to_string(),
                })
            }
            fn stream(&self, _req: LlmRequest) -> Result<LlmStreamHandle, LlmError> {
                Err(LlmError::StreamProtocol {
                    line: 3,
                    detail: "malformed JSON".to_string(),
                })
            }
        }

        let result = run(config, "test", Box::new(ErrorGateway));
        assert!(result.is_err());
        match result.unwrap_err() {
            AgentError::StreamProtocol { line, detail } => {
                assert_eq!(line, 3);
                assert!(detail.contains("malformed JSON"));
            }
            e => panic!("expected StreamProtocol, got {:?}", e),
        }
    }

    #[test]
    fn test_max_iterations_exceeded() {
        let tmp = TempDir::new().unwrap();
        let mut config = make_config(&tmp, false);
        config.max_iterations = 1;

        struct ToolCallGateway;
        impl LlmGateway for ToolCallGateway {
            fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
                Ok(LlmResponse {
                    content: Some("calling tool".to_string()),
                    tool_calls: vec![ToolCall {
                        id: "call-1".to_string(),
                        call_type: Some("function".to_string()),
                        function: ToolCallFunction {
                            name: "read_file".to_string(),
                            arguments: r#"{"path": "nonexistent"}"#.to_string(),
                        },
                    }],
                    finish_reason: Some("tool_calls".to_string()),
                    usage: None,
                })
            }
            fn stream(&self, req: LlmRequest) -> Result<LlmStreamHandle, LlmError> {
                self.complete(req).map(|_| {
                    LlmStreamHandle::new(vec![
                        Ok(LlmStreamEvent::TextDelta {
                            content: "calling".to_string(),
                        }),
                        Ok(LlmStreamEvent::Completed {
                            finish_reason: "tool_calls".to_string(),
                            usage: None,
                        }),
                    ])
                })
            }
        }

        let result = run(config, "test", Box::new(ToolCallGateway));
        assert!(result.is_err());
        match result.unwrap_err() {
            AgentError::MaxIterations { max } => assert_eq!(max, 1),
            e => panic!("expected MaxIterations, got {:?}", e),
        }
    }
}
