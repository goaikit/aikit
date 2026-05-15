use std::sync::Arc;

use crate::compression::maybe_compress;
use crate::config::AgentConfig;
use crate::context::{ContextPacket, ContextToolCall, ContextToolResult, TokenBudget, Turn};
use crate::errors::AgentError;
use crate::llm::gateway::LlmGateway;
use crate::llm::types::{LlmMessage, LlmRequest, LlmStreamEvent, LlmUsage, ToolCall};
use crate::skills::SkillProvider;

#[cfg(not(feature = "fastskill"))]
use crate::skills::FilesystemSkillProvider;
use crate::subagents::SpawnSubagentTool;
use crate::tools::{
    GitTool, HostToolAdapter, ReadFileTool, ReadSkillTool, RunBashTool, Tool, ToolContext,
    WriteFileTool,
};
use crate::AgentInternalEvent;

#[cfg(feature = "fastskill")]
use crate::skills::FastskillSkillBackend;

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

    // 1. Discover skills using the appropriate backend.
    // AK-08 DEFERRED (issue #29): Remote-skills parity for embedded runs. The embedded path
    // already supports local skills via config.skills_dirs (set from AIKIT_SKILLS_DIR or
    // .aikit/skills/). Remote skill resolution via the fastskill feature flag works identically
    // in both CLI and embedded paths when the feature is enabled. Full remote-skills parity
    // for embedded runs is deferred to a follow-up issue.
    let (skills, provider) = discover_skills_for_run(&config)?;

    // 2. Build system instructions (includes skills catalog when skills are present)
    let skill_metadatas: Vec<crate::skills::SkillMetadata> =
        skills.iter().map(|s| s.metadata.clone()).collect();
    let system_instructions = build_system_instructions(&config, &skill_metadatas)?;

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
    let tools = build_tools(
        &config,
        Arc::clone(&gateway),
        &skills,
        Arc::clone(&provider),
    );

    // 5. Main agent loop
    run_loop(&config, &mut context, &tools, &gateway, &mut events)?;

    Ok(events)
}

/// Run the agent loop starting from an existing context seeded with prior conversation turns.
pub fn run_with_context(
    config: AgentConfig,
    prior_turns: Vec<Turn>,
    new_prompt: &str,
    gateway: Box<dyn LlmGateway>,
) -> Result<Vec<AgentInternalEvent>, AgentError> {
    let gateway: Arc<dyn LlmGateway> = Arc::from(gateway);
    let mut events = Vec::new();

    let (skills, provider) = discover_skills_for_run(&config)?;

    let skill_metadatas: Vec<crate::skills::SkillMetadata> =
        skills.iter().map(|s| s.metadata.clone()).collect();
    let system_instructions = build_system_instructions(&config, &skill_metadatas)?;

    let budget = TokenBudget {
        total_budget: config.context_budget_tokens,
        reserve_for_tools: 1000,
        reserve_for_output: 2000,
    };
    let mut context = ContextPacket::new(system_instructions, budget);
    context.skills_summary = skills.iter().map(|s| s.metadata.clone()).collect();

    for turn in prior_turns {
        context.add_turn(turn);
    }
    context.add_turn(Turn::user(new_prompt));

    let tools = build_tools(
        &config,
        Arc::clone(&gateway),
        &skills,
        Arc::clone(&provider),
    );

    run_loop(&config, &mut context, &tools, &gateway, &mut events)?;

    Ok(events)
}

fn run_loop(
    config: &AgentConfig,
    context: &mut ContextPacket,
    tools: &[Box<dyn Tool>],
    gateway: &Arc<dyn LlmGateway>,
    events: &mut Vec<AgentInternalEvent>,
) -> Result<(), AgentError> {
    for iteration in 0..config.max_iterations {
        // Check context budget and compress if needed
        if let Some(compression) =
            maybe_compress(context).map_err(|e| AgentError::ContextCompression {
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
        let req = build_llm_request(config, context, tool_schemas);

        // Call LLM
        let (response_text, tool_calls, finish_reason, usage) = if config.stream {
            call_stream(gateway.as_ref(), req, events)?
        } else {
            call_complete(gateway.as_ref(), req, events)?
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

                let output = execute_tool(tools, &tool_name, args, &tool_ctx);

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

    Ok(())
}

pub(crate) fn discover_skills_for_run(
    config: &AgentConfig,
) -> Result<(Vec<crate::skills::DiscoveredSkill>, Arc<dyn SkillProvider>), AgentError> {
    #[cfg(feature = "fastskill")]
    {
        let backend = Arc::new(FastskillSkillBackend::new(config)?);
        let discovered = backend.discover(&config.skills_dirs);
        Ok((discovered, backend as Arc<dyn SkillProvider>))
    }

    #[cfg(not(feature = "fastskill"))]
    {
        let p = Arc::new(FilesystemSkillProvider);
        let discovered = p.discover(&config.skills_dirs);
        Ok((discovered, p as Arc<dyn SkillProvider>))
    }
}

pub(crate) fn build_skills_catalog_block(
    skills: &[crate::skills::SkillMetadata],
) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let mut lines = Vec::with_capacity(skills.len() + 4);
    lines.push("## Available Skills".to_string());
    lines.push(String::new());
    lines.push(
        "Use the `read_skill` tool with one of the following names to load a skill's instructions:"
            .to_string(),
    );
    lines.push(String::new());
    for skill in skills {
        lines.push(format!("- `{}`: {}", skill.name, skill.description));
    }
    Some(lines.join("\n"))
}

fn build_system_instructions(
    config: &AgentConfig,
    skills: &[crate::skills::SkillMetadata],
) -> Result<String, AgentError> {
    let mut parts = Vec::new();

    // Persona prompt is prepended before everything else.
    if let Some(ref persona) = config.session_persona {
        if !persona.prompt.is_empty() {
            parts.push(persona.prompt.clone());
        }
    }

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

    if let Some(catalog) = build_skills_catalog_block(skills) {
        parts.push(catalog);
    }

    Ok(parts.join("\n\n"))
}

fn build_tools(
    config: &AgentConfig,
    gateway: Arc<dyn LlmGateway>,
    skills: &[crate::skills::DiscoveredSkill],
    provider: Arc<dyn SkillProvider>,
) -> Vec<Box<dyn Tool>> {
    let mut tools: Vec<Box<dyn Tool>> = vec![
        Box::new(ReadFileTool),
        Box::new(WriteFileTool),
        Box::new(RunBashTool),
        Box::new(GitTool),
        Box::new(ReadSkillTool {
            skills: skills.to_vec(),
            provider,
        }),
    ];

    // Only add spawn_subagent if max_subagent_depth > 0
    if config.max_subagent_depth > 0 {
        tools.push(Box::new(SpawnSubagentTool {
            parent_config: config.clone(),
            gateway,
        }));
    }

    // Merge host tools, skipping any whose name collides with a built-in.
    if let Some(ref host_provider) = config.host_tool_provider {
        let builtin_names: std::collections::HashSet<String> =
            tools.iter().map(|t| t.name().to_string()).collect();
        for def in host_provider.list_tools() {
            if builtin_names.contains(def.name.as_str()) {
                tracing::warn!(
                    tool = %def.name,
                    "host tool name collides with built-in; host tool skipped"
                );
                continue;
            }
            tools.push(Box::new(HostToolAdapter {
                definition: def,
                provider: Arc::clone(host_provider),
            }));
        }
    }

    // Apply persona tool policy (hard filter at construction time).
    if let Some(ref persona) = config.session_persona {
        if let Some(ref allowlist) = persona.tools {
            tools.retain(|t| allowlist.iter().any(|a| a == t.name()));
        }
        if let Some(ref denylist) = persona.disallowed_tools {
            tools.retain(|t| {
                let keep = !denylist.iter().any(|d| d == t.name());
                if !keep {
                    tracing::debug!(tool = %t.name(), "persona denylist removed tool");
                }
                keep
            });
        }
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
            content: Some(context.system_instructions.clone()),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for turn in &context.conversation {
        match turn.role {
            crate::context::TurnRole::User => {
                messages.push(LlmMessage {
                    role: "user".to_string(),
                    content: Some(turn.content.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            crate::context::TurnRole::Assistant => {
                let tool_calls = turn.tool_calls.as_ref().map(|calls| {
                    calls
                        .iter()
                        .map(|tc| crate::llm::types::MessageToolCall {
                            id: tc.id.clone(),
                            call_type: "function".to_string(),
                            function: crate::llm::types::MessageToolCallFunction {
                                name: tc.name.clone(),
                                arguments: tc.arguments.clone(),
                            },
                        })
                        .collect::<Vec<_>>()
                });
                let content = if turn.content.is_empty() && tool_calls.is_some() {
                    None
                } else {
                    Some(turn.content.clone())
                };
                messages.push(LlmMessage {
                    role: "assistant".to_string(),
                    content,
                    tool_calls,
                    tool_call_id: None,
                });
            }
            crate::context::TurnRole::Tool => {
                if let Some(results) = &turn.tool_results {
                    for result in results {
                        messages.push(LlmMessage {
                            role: "tool".to_string(),
                            content: Some(result.output.clone()),
                            tool_calls: None,
                            tool_call_id: Some(result.call_id.clone()),
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
        LlmError, LlmRequest, LlmResponse, LlmStreamEvent, LlmStreamHandle, ToolCall,
        ToolCallFunction,
    };
    use tempfile::TempDir;

    /// A gateway that records every outbound LlmRequest for inspection.
    struct CapturingGateway {
        captured: std::sync::Arc<std::sync::Mutex<Vec<LlmRequest>>>,
        inner: MockGateway,
    }

    impl CapturingGateway {
        fn new(responses: Vec<MockResponse>) -> Self {
            Self {
                captured: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                inner: MockGateway::new(responses),
            }
        }
    }

    impl LlmGateway for CapturingGateway {
        fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
            self.captured.lock().unwrap().push(req.clone());
            self.inner.complete(req)
        }
        fn stream(&self, req: LlmRequest) -> Result<LlmStreamHandle, LlmError> {
            self.captured.lock().unwrap().push(req.clone());
            self.inner.stream(req)
        }
    }

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
            session_persona: None,
            session_agents: std::collections::HashMap::new(),
            host_tool_provider: None,
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

    /// run_with_context() seeds the LLM request with prior turns before the new user message.
    #[test]
    fn test_run_with_context_seeds_prior_turns_in_order() {
        let tmp = TempDir::new().unwrap();
        let config = make_config(&tmp, false);

        let prior_turns = vec![Turn::user("What is 2+2?"), Turn::assistant("4")];

        let gw = CapturingGateway::new(vec![MockResponse::text("Based on our prior chat...")]);
        let captured = std::sync::Arc::clone(&gw.captured);

        let _events = run_with_context(config, prior_turns, "What is 3+3?", Box::new(gw)).unwrap();

        let requests = captured.lock().unwrap();
        assert!(
            !requests.is_empty(),
            "at least one LLM request must be made"
        );
        let first_req = &requests[0];

        let non_system: Vec<_> = first_req
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .collect();

        assert!(
            non_system.len() >= 3,
            "expected at least 3 non-system messages (2 prior + 1 new), got {}",
            non_system.len()
        );
        assert_eq!(
            non_system[0].content.as_deref(),
            Some("What is 2+2?"),
            "first prior turn should be user message"
        );
        assert_eq!(non_system[0].role, "user");
        assert_eq!(
            non_system[1].content.as_deref(),
            Some("4"),
            "second prior turn should be assistant message"
        );
        assert_eq!(non_system[1].role, "assistant");
        assert_eq!(
            non_system[2].content.as_deref(),
            Some("What is 3+3?"),
            "new user message should be last"
        );
        assert_eq!(non_system[2].role, "user");
    }

    /// run_with_context() includes tool calls from seeded turns in the LLM message list.
    #[test]
    fn test_run_with_context_includes_tool_calls_from_prior_turns() {
        let tmp = TempDir::new().unwrap();
        let config = make_config(&tmp, false);

        let prior_turns = vec![
            Turn::user("Read the file"),
            Turn::assistant_with_tool_calls(
                "",
                vec![ContextToolCall {
                    id: "call-prior-1".to_string(),
                    name: "read_file".to_string(),
                    arguments: r#"{"path":"README.md"}"#.to_string(),
                }],
            ),
            Turn::tool_result(vec![ContextToolResult {
                call_id: "call-prior-1".to_string(),
                output: "# README content".to_string(),
                is_error: false,
            }]),
        ];

        let gw = CapturingGateway::new(vec![MockResponse::text("Done reading.")]);
        let captured = std::sync::Arc::clone(&gw.captured);

        let _events = run_with_context(config, prior_turns, "Summarise it", Box::new(gw)).unwrap();

        let requests = captured.lock().unwrap();
        assert!(!requests.is_empty());
        let first_req = &requests[0];

        let has_tool_call_msg = first_req.messages.iter().any(|m| {
            m.tool_calls
                .as_ref()
                .map(|tc| !tc.is_empty())
                .unwrap_or(false)
        });
        assert!(
            has_tool_call_msg,
            "prior tool-call turn must appear in the outbound message list"
        );

        let has_tool_result_msg = first_req.messages.iter().any(|m| m.tool_call_id.is_some());
        assert!(
            has_tool_result_msg,
            "prior tool-result turn must appear in the outbound message list"
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

    #[test]
    fn test_two_iteration_tool_use_non_streaming() {
        let tmp = TempDir::new().unwrap();
        let config = make_config(&tmp, false);

        let gw = MockGateway::new(vec![
            MockResponse::tool_call("c1", "read_file", r#"{"path": "test.txt"}"#),
            MockResponse::text("Done reading file"),
        ]);

        let events = run(config, "Read test.txt", Box::new(gw)).unwrap();

        let has_tool_use = events.iter().any(|e| {
            matches!(
                e,
                AgentInternalEvent::ToolUse { tool_name, .. }
                if tool_name == "read_file"
            )
        });
        assert!(has_tool_use, "should have ToolUse event");

        let has_final = events.iter().any(|e| {
            matches!(
                e,
                AgentInternalEvent::TextFinal { content, .. }
                if content == "Done reading file"
            )
        });
        assert!(has_final, "should have final text response");
    }

    #[test]
    fn test_two_iteration_tool_use_streaming() {
        let tmp = TempDir::new().unwrap();
        let config = make_config(&tmp, true);

        let gw = MockGateway::new(vec![
            MockResponse::tool_call("c2", "read_file", r#"{"path": "test.txt"}"#),
            MockResponse::text("Done reading file"),
        ]);

        let events = run(config, "Read test.txt", Box::new(gw)).unwrap();

        let has_tool_use = events.iter().any(|e| {
            matches!(
                e,
                AgentInternalEvent::ToolUse { tool_name, .. }
                if tool_name == "read_file"
            )
        });
        assert!(has_tool_use, "should have ToolUse event");

        let has_final = events.iter().any(|e| {
            matches!(
                e,
                AgentInternalEvent::TextFinal { content, .. }
                if content == "Done reading file"
            )
        });
        assert!(has_final, "should have final text response");
    }

    #[test]
    fn test_build_llm_request_includes_tool_calls() {
        let tmp = TempDir::new().unwrap();
        let config = make_config(&tmp, false);

        let budget = crate::context::TokenBudget {
            total_budget: 12000,
            reserve_for_tools: 1000,
            reserve_for_output: 2000,
        };
        let mut context = crate::context::ContextPacket::new("System prompt".to_string(), budget);
        context.add_turn(crate::context::Turn::user("Read the file"));
        context.add_turn(crate::context::Turn::assistant_with_tool_calls(
            "",
            vec![crate::context::ContextToolCall {
                id: "call_abc".to_string(),
                name: "read_file".to_string(),
                arguments: r#"{"path": "AGENTS.md"}"#.to_string(),
            }],
        ));
        context.add_turn(crate::context::Turn::tool_result(vec![
            crate::context::ContextToolResult {
                call_id: "call_abc".to_string(),
                output: "file contents here".to_string(),
                is_error: false,
            },
        ]));

        let req = build_llm_request(&config, &context, vec![]);

        let assistant_msg = req
            .messages
            .iter()
            .find(|m| m.role == "assistant" && m.tool_calls.is_some())
            .expect("should have assistant message with tool_calls");

        assert!(assistant_msg.tool_calls.is_some());
        let calls = assistant_msg.tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_abc");
        assert_eq!(calls[0].function.name, "read_file");
        assert!(assistant_msg.tool_call_id.is_none());
    }

    #[test]
    fn test_build_llm_request_includes_tool_call_id() {
        let tmp = TempDir::new().unwrap();
        let config = make_config(&tmp, false);

        let budget = crate::context::TokenBudget {
            total_budget: 12000,
            reserve_for_tools: 1000,
            reserve_for_output: 2000,
        };
        let mut context = crate::context::ContextPacket::new("System prompt".to_string(), budget);
        context.add_turn(crate::context::Turn::user("Read the file"));
        context.add_turn(crate::context::Turn::assistant_with_tool_calls(
            "",
            vec![crate::context::ContextToolCall {
                id: "call_abc".to_string(),
                name: "read_file".to_string(),
                arguments: r#"{"path": "AGENTS.md"}"#.to_string(),
            }],
        ));
        context.add_turn(crate::context::Turn::tool_result(vec![
            crate::context::ContextToolResult {
                call_id: "call_abc".to_string(),
                output: "file contents here".to_string(),
                is_error: false,
            },
        ]));

        let req = build_llm_request(&config, &context, vec![]);

        let tool_msg = req
            .messages
            .iter()
            .find(|m| m.role == "tool" && m.tool_call_id.is_some())
            .expect("should have tool message with tool_call_id");

        assert_eq!(tool_msg.tool_call_id.as_deref(), Some("call_abc"));
        assert_eq!(tool_msg.content.as_deref(), Some("file contents here"));
        assert!(tool_msg.tool_calls.is_none());
    }

    #[test]
    fn test_build_system_instructions_includes_md_content() {
        let tmp = TempDir::new().unwrap();
        let md_path = tmp.path().join("AGENTS.md");
        std::fs::write(&md_path, "Custom system instructions for testing").unwrap();
        let mut config = make_config(&tmp, false);
        config.agents_md_path = Some(md_path);
        let instructions = build_system_instructions(&config, &[]).unwrap();
        assert!(
            instructions.contains("Custom system instructions for testing"),
            "build_system_instructions should include md file content"
        );
    }

    fn make_skill(name: &str, description: &str) -> crate::skills::SkillMetadata {
        crate::skills::SkillMetadata {
            name: name.to_string(),
            description: description.to_string(),
            path: std::path::PathBuf::new(),
        }
    }

    #[test]
    fn test_catalog_block_empty_returns_none() {
        assert!(build_skills_catalog_block(&[]).is_none());
    }

    #[test]
    fn test_catalog_block_single_skill() {
        let skills = vec![make_skill("git-summarize", "Summarize git activity")];
        let block = build_skills_catalog_block(&skills).unwrap();
        assert!(block.contains("## Available Skills"));
        assert!(block.contains("- `git-summarize`: Summarize git activity"));
    }

    #[test]
    fn test_catalog_block_multiple_skills() {
        let skills = vec![
            make_skill("skill-a", "First skill"),
            make_skill("skill-b", "Second skill"),
        ];
        let block = build_skills_catalog_block(&skills).unwrap();
        let pos_a = block.find("- `skill-a`: First skill").unwrap();
        let pos_b = block.find("- `skill-b`: Second skill").unwrap();
        assert!(pos_a < pos_b, "skill-a must appear before skill-b");
    }

    #[test]
    fn test_catalog_block_empty_description() {
        let skills = vec![make_skill("no-desc", "")];
        let block = build_skills_catalog_block(&skills).unwrap();
        assert!(
            block.contains("- `no-desc`: "),
            "empty description renders as colon-space"
        );
    }

    #[test]
    fn test_catalog_block_header_literal() {
        let skills = vec![make_skill("x", "y")];
        let block = build_skills_catalog_block(&skills).unwrap();
        assert!(block.contains("## Available Skills"));
    }

    #[test]
    fn test_build_system_instructions_catalog_after_agents_md() {
        let tmp = TempDir::new().unwrap();
        let md_path = tmp.path().join("AGENTS.md");
        std::fs::write(&md_path, "AGENTS MD CONTENT").unwrap();
        let mut config = make_config(&tmp, false);
        config.agents_md_path = Some(md_path);
        let skills = vec![
            make_skill("skill-one", "Does thing one"),
            make_skill("skill-two", "Does thing two"),
        ];
        let instructions = build_system_instructions(&config, &skills).unwrap();
        let pos_agents = instructions.find("AGENTS MD CONTENT").unwrap();
        let pos_catalog = instructions.find("## Available Skills").unwrap();
        assert!(
            pos_agents < pos_catalog,
            "AGENTS.md content must appear before the skills catalog"
        );
        assert!(instructions.contains("- `skill-one`: Does thing one"));
        assert!(instructions.contains("- `skill-two`: Does thing two"));
    }

    #[cfg(feature = "fastskill")]
    fn create_skill_in_dir(root: &std::path::Path, dir_name: &str, name: &str, description: &str) {
        let skill_dir = root.join(dir_name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        let content = format!(
            "---\nname: {}\ndescription: {}\n---\n\n# Skill Content\n\nFull skill body here for {}.",
            name, description, name
        );
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    #[cfg(feature = "fastskill")]
    #[test]
    fn test_resolver_backed_discovery_populates_skills_summary() {
        let tmp = TempDir::new().unwrap();
        let skills_root = tmp.path().join("skills");
        std::fs::create_dir_all(&skills_root).unwrap();
        create_skill_in_dir(&skills_root, "skill-a", "skill-a", "First skill");
        create_skill_in_dir(&skills_root, "skill-b", "skill-b", "Second skill");

        let mut config = make_config(&tmp, false);
        config.skills_dirs = vec![skills_root.clone()];

        // Direct assertion against the discovery output that run_inner writes
        // into ContextPacket.skills_summary
        let (skills, _provider) = discover_skills_for_run(&config).unwrap();
        let names: Vec<&str> = skills.iter().map(|s| s.metadata.name.as_str()).collect();
        assert!(
            names.contains(&"skill-a"),
            "skills_summary must include 'skill-a', got: {:?}",
            names
        );
        assert!(
            names.contains(&"skill-b"),
            "skills_summary must include 'skill-b', got: {:?}",
            names
        );

        // End-to-end: read_skill tool call resolves through the fastskill backend
        let gw = MockGateway::new(vec![
            MockResponse::tool_call("read_skill_1", "read_skill", r#"{"skill_name":"skill-a"}"#),
            MockResponse::text("done"),
        ]);
        let events = run_inner(config, "read skill-a", Arc::new(gw)).unwrap();
        let skill_result = events.iter().find_map(|event| {
            if let AgentInternalEvent::ToolResult {
                call_id,
                output,
                is_error,
            } = event
            {
                (call_id == "read_skill_1").then_some((output, *is_error))
            } else {
                None
            }
        });
        let (output, is_error) = skill_result.expect("read_skill tool result should be emitted");
        assert!(!is_error, "read_skill should load fastskill-backed content");
        assert!(
            output.contains("Full skill body here for skill-a."),
            "read_skill should return full resolved content, got: {}",
            output
        );
    }

    #[cfg(feature = "fastskill")]
    #[test]
    fn test_resolver_no_match_yields_empty_summary() {
        let tmp = TempDir::new().unwrap();
        let config = make_config(&tmp, false);

        let (skills, _provider) = discover_skills_for_run(&config).unwrap();
        assert!(
            skills.is_empty(),
            "empty skills_dirs should produce an empty discovery"
        );

        // Run still succeeds end-to-end with no skills
        let gw = MockGateway::new(vec![MockResponse::text("done")]);
        let events = run_inner(config, "test", Arc::new(gw)).unwrap();
        let has_error = events
            .iter()
            .any(|e| matches!(e, AgentInternalEvent::Error { .. }));
        assert!(
            !has_error,
            "run_inner should complete without errors when no skills are found"
        );
    }

    // ── Host tool tests ──────────────────────────────────────────────────────

    use crate::host_tools::{HostToolDefinition, HostToolProvider};

    struct SimpleHostProvider {
        tools: Vec<HostToolDefinition>,
    }

    impl HostToolProvider for SimpleHostProvider {
        fn list_tools(&self) -> Vec<HostToolDefinition> {
            self.tools.clone()
        }
        fn call_tool(&self, _name: &str, _args: serde_json::Value) -> Result<String, String> {
            Ok("host result".to_string())
        }
    }

    fn make_host_def(name: &str) -> HostToolDefinition {
        HostToolDefinition {
            name: name.to_string(),
            description: None,
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }
    }

    #[test]
    fn test_host_tools_merged() {
        let tmp = TempDir::new().unwrap();
        let mut config = make_config(&tmp, false);
        config.host_tool_provider = Some(Arc::new(SimpleHostProvider {
            tools: vec![make_host_def("my_host_tool")],
        }));
        let gateway = Arc::new(MockGateway::new(vec![]));
        let skills = vec![];
        let provider: Arc<dyn crate::skills::SkillProvider> =
            Arc::new(crate::skills::FilesystemSkillProvider);
        let tools = build_tools(&config, gateway, &skills, provider);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(
            names.contains(&"my_host_tool"),
            "host tool should be in the tool list: {:?}",
            names
        );
    }

    #[test]
    fn test_host_tool_name_collision_skips_host() {
        let tmp = TempDir::new().unwrap();
        let mut config = make_config(&tmp, false);
        config.host_tool_provider = Some(Arc::new(SimpleHostProvider {
            tools: vec![make_host_def("run_bash")],
        }));
        let gateway = Arc::new(MockGateway::new(vec![]));
        let skills = vec![];
        let provider: Arc<dyn crate::skills::SkillProvider> =
            Arc::new(crate::skills::FilesystemSkillProvider);
        let tools = build_tools(&config, gateway, &skills, provider);
        let run_bash_count = tools.iter().filter(|t| t.name() == "run_bash").count();
        assert_eq!(
            run_bash_count, 1,
            "should have exactly one run_bash (built-in)"
        );
    }

    #[test]
    fn test_persona_allowlist_applies_to_host_tool() {
        let tmp = TempDir::new().unwrap();
        let mut config = make_config(&tmp, false);
        config.host_tool_provider = Some(Arc::new(SimpleHostProvider {
            tools: vec![make_host_def("my_host_tool")],
        }));
        config.session_persona = Some(crate::agent_definition::AgentPersona {
            name: "test".to_string(),
            description: String::new(),
            prompt: String::new(),
            model: None,
            tools: Some(vec!["read_file".to_string()]),
            disallowed_tools: None,
        });
        let gateway = Arc::new(MockGateway::new(vec![]));
        let skills = vec![];
        let provider: Arc<dyn crate::skills::SkillProvider> =
            Arc::new(crate::skills::FilesystemSkillProvider);
        let tools = build_tools(&config, gateway, &skills, provider);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(
            !names.contains(&"my_host_tool"),
            "allowlist should exclude host tool: {:?}",
            names
        );
        assert!(
            names.contains(&"read_file"),
            "allowlist should include read_file: {:?}",
            names
        );
    }

    #[test]
    fn test_persona_denylist_applies_to_host_tool() {
        let tmp = TempDir::new().unwrap();
        let mut config = make_config(&tmp, false);
        config.host_tool_provider = Some(Arc::new(SimpleHostProvider {
            tools: vec![make_host_def("my_host_tool")],
        }));
        config.session_persona = Some(crate::agent_definition::AgentPersona {
            name: "test".to_string(),
            description: String::new(),
            prompt: String::new(),
            model: None,
            tools: None,
            disallowed_tools: Some(vec!["my_host_tool".to_string()]),
        });
        let gateway = Arc::new(MockGateway::new(vec![]));
        let skills = vec![];
        let provider: Arc<dyn crate::skills::SkillProvider> =
            Arc::new(crate::skills::FilesystemSkillProvider);
        let tools = build_tools(&config, gateway, &skills, provider);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(
            !names.contains(&"my_host_tool"),
            "denylist should exclude host tool: {:?}",
            names
        );
    }
}
