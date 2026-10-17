//! The agent loop as a step machine.
//!
//! ```text
//! let mut core = AgentCore::new(config, turns);
//! let mut effects = core.start();
//! loop {
//!     // carry out `effects` in order; each CallModel / CallTool ends a batch
//!     effects = core.on(input)?;
//! }
//! ```
//!
//! At most one [`Effect::CallModel`] or [`Effect::CallTool`] is outstanding,
//! and it is always the last effect of a batch. [`Effect::Done`] ends the run.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::compression::maybe_compress;
use crate::context::{build_messages, ContextToolCall, ContextToolResult, TokenBudget, Turn};
use crate::llm::stream::{SseLine, SseParser};
use crate::llm::types::{
    LlmError, LlmMessage, LlmRequest, LlmResponse, LlmStreamEvent, LlmUsage, ToolCall,
    ToolCallFunction, ToolChoice, ToolDefinition,
};
use crate::llm::wire::ChatCompletionRequest;
use crate::{hook_names, AgentEvent, HookAction, HookPhase};

/// What the loop needs to run. Everything that comes from files or the
/// environment (the system prompt, the tool set) is already resolved by the
/// host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfig {
    /// The model name sent in each request.
    pub model: String,
    /// Ask for streamed answers. Only sets [`ModelRequest::stream`]: the core
    /// takes either kind of answer.
    pub stream: bool,
    /// Model calls allowed in this run. `0` finishes the run at once.
    pub max_iterations: u32,
    /// When the conversation is over budget, the oldest turns are summarised
    /// before the next model call.
    pub token_budget: TokenBudget,
    /// The system prompt, sent first in every request (unless empty).
    pub system_instructions: String,
    /// The tools offered to the model. A call to any other name is refused by
    /// the core and never reaches the host.
    pub tools: Vec<ToolDefinition>,
}

/// A model call, without transport details: no URL, no key. The host adds
/// those (see [`ModelRequest::into_llm_request`]) or sends the body itself
/// (see [`ModelRequest::wire_body_json`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequest {
    pub model: String,
    pub messages: Vec<LlmMessage>,
    pub tools: Vec<ToolDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Whether to stream the answer (and feed [`Input::ModelLine`]s) or wait
    /// for the whole of it (and feed [`Input::ModelResponse`]).
    pub stream: bool,
}

impl ModelRequest {
    /// The request for an `LlmGateway`, with the endpoint and key added.
    pub fn into_llm_request(self, base_url: String, api_key: String) -> LlmRequest {
        LlmRequest {
            model: self.model,
            base_url,
            api_key,
            messages: self.messages,
            tools: self.tools,
            tool_choice: self.tool_choice,
            temperature: self.temperature,
            top_p: self.top_p,
            max_tokens: self.max_tokens,
            stream: self.stream,
        }
    }

    /// The OpenAI chat-completions body to `POST` to `{base_url}/chat/completions`.
    pub fn wire_body(&self) -> ChatCompletionRequest<'_> {
        ChatCompletionRequest::from_parts(
            &self.model,
            &self.messages,
            &self.tools,
            self.temperature,
            self.top_p,
            self.max_tokens,
            self.stream,
        )
    }

    /// [`ModelRequest::wire_body`] as a JSON string.
    pub fn wire_body_json(&self) -> String {
        serde_json::to_string(&self.wire_body()).expect("a chat-completions body serializes")
    }
}

/// What the core asks the host to do. The `effect` tag names the variant in
/// JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum Effect {
    /// Call the model. Answer with [`Input::ModelLine`]s (or
    /// [`Input::ModelEvent`]s) and then [`Input::ModelEnd`], with one
    /// [`Input::ModelResponse`], or with [`Input::ModelError`].
    CallModel { request: ModelRequest },
    /// Run a tool. Answer with [`Input::ToolResult`] for this `call_id`.
    /// `arguments` is the model's argument string parsed as JSON, or `null`
    /// when it isn't valid JSON.
    CallTool {
        call_id: String,
        name: String,
        arguments: Value,
    },
    /// Hand an event to whoever watches the run.
    Emit { event: AgentEvent },
    /// The run is over. No input but [`Input::Now`] is taken after this.
    Done { outcome: Outcome },
}

/// What the host tells the core. The `input` tag names the variant in JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "input", rename_all = "snake_case")]
pub enum Input {
    /// One raw line of the model's SSE stream, as it arrives (with or without
    /// its line ending). Lines after `data: [DONE]` are ignored.
    ModelLine { line: String },
    /// One already-parsed stream event, for a host whose transport parses.
    ModelEvent { event: LlmStreamEvent },
    /// The stream is over.
    ModelEnd,
    /// The whole answer of a non-streamed call.
    ModelResponse { response: LlmResponse },
    /// The model call failed.
    ModelError { error: LlmError },
    /// The result of the outstanding [`Effect::CallTool`].
    ToolResult {
        call_id: String,
        result: ToolOutcome,
        /// When the tool ran, as the host measured it. Without it the core
        /// uses the [`Input::Now`] values around the call.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timing: Option<ToolTiming>,
    },
    /// The current time, in Unix epoch milliseconds. Taken in every state.
    Now { unix_ms: i64 },
    /// Stop the run. The core answers [`Effect::Done`] with
    /// [`Outcome::Cancelled`].
    Cancel,
}

/// A tool's output as the model will see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolOutcome {
    pub content: String,
    pub is_error: bool,
}

impl ToolOutcome {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    pub fn err(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

/// When a tool ran (ADR 0022).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolTiming {
    /// Unix epoch milliseconds when the tool started.
    pub started_at_ms: i64,
    pub duration_ms: u64,
}

/// How a run ended. The `status` tag names the variant in JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    /// The model finished, or the run used its last iteration without a
    /// pending tool call.
    Completed,
    /// The run failed. An [`AgentEvent::Error`] was emitted just before.
    Failed { error: RunError },
    /// The host sent [`Input::Cancel`].
    Cancelled,
}

/// Why a run failed. The `kind` tag names the variant in JSON.
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunError {
    /// The model call failed ([`Input::ModelError`]), or a stream line was
    /// not a valid chunk.
    #[error("{error}")]
    Model { error: LlmError },
    /// The stream carried an [`LlmStreamEvent::ProviderError`].
    #[error("{code}: {message}")]
    Provider { code: String, message: String },
    /// The last allowed iteration still asked for tools.
    #[error("E_AIKIT_MAX_ITERATIONS: exceeded max iterations ({max})")]
    MaxIterations { max: u32 },
}

/// An input the core can't take in its current state. The core's state is
/// unchanged.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputError {
    #[error("input `{input}` is not expected while the core is {state}")]
    Unexpected { input: String, state: String },
    #[error("tool result for `{call_id}`, but the core is waiting for `{expected}`")]
    WrongCall { call_id: String, expected: String },
}

/// The agent loop. See the [module docs](self).
///
/// The whole state is `serde`, so a host can save a run between inputs and
/// pick it up again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCore {
    config: CoreConfig,
    conversation: Vec<Turn>,
    iteration: u32,
    now_ms: i64,
    state: State,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum State {
    NotStarted,
    AwaitingModel { turn: ModelTurn },
    AwaitingTool { round: ToolRound },
    Finished,
}

impl State {
    fn name(&self) -> &'static str {
        match self {
            State::NotStarted => "not started",
            State::AwaitingModel { .. } => "waiting for the model",
            State::AwaitingTool { .. } => "waiting for a tool",
            State::Finished => "finished",
        }
    }
}

/// One model answer being assembled.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ModelTurn {
    parser: SseParser,
    text: String,
    tool_calls: Vec<ToolCall>,
    pending: Option<PartialToolCall>,
    finish_reason: Option<String>,
    usage: Option<LlmUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            function: ToolCallFunction {
                name: self.function_name,
                arguments: self.arguments,
            },
        }
    }
}

/// The tool calls of one model answer, run one after the other.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolRound {
    calls: Vec<ToolCall>,
    /// The call being run, or about to be.
    next: usize,
    results: Vec<ContextToolResult>,
    finish_reason: Option<String>,
    /// `now_ms` when the outstanding call was handed to the host.
    started_at_ms: i64,
}

impl ModelTurn {
    /// Fold one stream event into the answer.
    fn apply(&mut self, event: LlmStreamEvent, out: &mut Vec<Effect>) -> Result<(), RunError> {
        match event {
            LlmStreamEvent::TextDelta { content } => {
                self.text.push_str(&content);
                emit(
                    out,
                    AgentEvent::TextDelta {
                        content,
                        turn_id: None,
                    },
                );
            }
            LlmStreamEvent::ToolCallDelta {
                id,
                function_name,
                arguments_delta,
            } => {
                if !id.is_empty() {
                    if let Some(prev) = self.pending.take() {
                        self.tool_calls.push(prev.into_tool_call());
                    }
                    self.pending = Some(PartialToolCall {
                        id,
                        function_name,
                        arguments: arguments_delta,
                    });
                } else if let Some(ref mut tc) = self.pending {
                    tc.arguments.push_str(&arguments_delta);
                    if !function_name.is_empty() && tc.function_name.is_empty() {
                        tc.function_name = function_name;
                    }
                }
            }
            LlmStreamEvent::UsageUpdate { usage } => self.usage = Some(usage),
            LlmStreamEvent::Completed {
                finish_reason,
                usage,
            } => {
                self.finish_reason = Some(finish_reason);
                if usage.is_some() {
                    self.usage = usage;
                }
            }
            LlmStreamEvent::ProviderError { code, message } => {
                emit(
                    out,
                    AgentEvent::Error {
                        code: code.clone(),
                        message: message.clone(),
                    },
                );
                return Err(RunError::Provider { code, message });
            }
        }
        Ok(())
    }
}

fn emit(out: &mut Vec<Effect>, event: AgentEvent) {
    out.push(Effect::Emit { event });
}

fn input_name(input: &Input) -> &'static str {
    match input {
        Input::ModelLine { .. } => "model_line",
        Input::ModelEvent { .. } => "model_event",
        Input::ModelEnd => "model_end",
        Input::ModelResponse { .. } => "model_response",
        Input::ModelError { .. } => "model_error",
        Input::ToolResult { .. } => "tool_result",
        Input::Now { .. } => "now",
        Input::Cancel => "cancel",
    }
}

impl AgentCore {
    /// A run over `conversation`: the prior turns, ending with the new user
    /// prompt. Nothing happens until [`AgentCore::start`].
    pub fn new(config: CoreConfig, conversation: Vec<Turn>) -> Self {
        Self {
            config,
            conversation,
            iteration: 0,
            now_ms: 0,
            state: State::NotStarted,
        }
    }

    /// Start the run: the first [`Effect::CallModel`], or [`Effect::Done`]
    /// when `max_iterations` is 0. A second call returns no effects.
    pub fn start(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        if !matches!(self.state, State::NotStarted) {
            return out;
        }
        if self.config.max_iterations == 0 {
            self.finish(Outcome::Completed, &mut out);
        } else {
            self.begin_iteration(&mut out);
        }
        out
    }

    /// Take one input and return what to do next.
    pub fn on(&mut self, input: Input) -> Result<Vec<Effect>, InputError> {
        let mut out = Vec::new();
        if let Input::Now { unix_ms } = input {
            self.now_ms = unix_ms;
            return Ok(out);
        }

        let state = std::mem::replace(&mut self.state, State::Finished);
        match (state, input) {
            (
                State::NotStarted | State::AwaitingModel { .. } | State::AwaitingTool { .. },
                Input::Cancel,
            ) => {
                self.finish(Outcome::Cancelled, &mut out);
            }
            (State::AwaitingModel { mut turn }, Input::ModelLine { line }) => {
                match turn.parser.feed_line(&line) {
                    Ok(SseLine::Done) => self.state = State::AwaitingModel { turn },
                    Ok(SseLine::Events(events)) => self.apply_events(turn, events, &mut out),
                    Err(error) => self.fail_model(error, &mut out),
                }
            }
            (State::AwaitingModel { turn }, Input::ModelEvent { event }) => {
                self.apply_events(turn, vec![event], &mut out);
            }
            (State::AwaitingModel { mut turn }, Input::ModelEnd) => {
                if let Some(tc) = turn.pending.take() {
                    turn.tool_calls.push(tc.into_tool_call());
                }
                emit(
                    &mut out,
                    AgentEvent::TextFinal {
                        content: turn.text.clone(),
                        turn_id: None,
                    },
                );
                self.after_model(
                    turn.text,
                    turn.tool_calls,
                    turn.finish_reason,
                    turn.usage,
                    &mut out,
                );
            }
            (State::AwaitingModel { .. }, Input::ModelResponse { response }) => {
                let content = response.content.unwrap_or_default();
                emit(
                    &mut out,
                    AgentEvent::TextFinal {
                        content: content.clone(),
                        turn_id: None,
                    },
                );
                self.after_model(
                    content,
                    response.tool_calls,
                    response.finish_reason,
                    response.usage,
                    &mut out,
                );
            }
            (State::AwaitingModel { .. }, Input::ModelError { error }) => {
                self.fail_model(error, &mut out);
            }
            (
                State::AwaitingTool { mut round },
                Input::ToolResult {
                    call_id,
                    result,
                    timing,
                },
            ) => {
                let expected = &round.calls[round.next].id;
                if &call_id != expected {
                    let err = InputError::WrongCall {
                        call_id,
                        expected: expected.clone(),
                    };
                    self.state = State::AwaitingTool { round };
                    return Err(err);
                }
                let timing = timing.unwrap_or(ToolTiming {
                    started_at_ms: round.started_at_ms,
                    duration_ms: self.now_ms.saturating_sub(round.started_at_ms).max(0) as u64,
                });
                emit(
                    &mut out,
                    AgentEvent::ToolResult {
                        call_id: call_id.clone(),
                        output: result.content.clone(),
                        is_error: result.is_error,
                        duration_ms: timing.duration_ms,
                        started_at_ms: timing.started_at_ms,
                    },
                );
                round.results.push(ContextToolResult {
                    call_id,
                    output: result.content,
                    is_error: result.is_error,
                });
                round.next += 1;
                self.state = State::AwaitingTool { round };
                self.advance_tools(&mut out);
            }
            (state, input) => {
                let err = InputError::Unexpected {
                    input: input_name(&input).to_string(),
                    state: state.name().to_string(),
                };
                self.state = state;
                return Err(err);
            }
        }
        Ok(out)
    }

    /// The conversation so far, including the turns this run added.
    pub fn conversation(&self) -> &[Turn] {
        &self.conversation
    }

    /// The configuration the run was built with.
    pub fn config(&self) -> &CoreConfig {
        &self.config
    }

    /// Whether [`Effect::Done`] has been returned.
    pub fn is_finished(&self) -> bool {
        matches!(self.state, State::Finished)
    }

    /// The iteration in progress, from 0.
    pub fn iteration(&self) -> u32 {
        self.iteration
    }

    fn finish(&mut self, outcome: Outcome, out: &mut Vec<Effect>) {
        self.state = State::Finished;
        out.push(Effect::Done { outcome });
    }

    fn fail_model(&mut self, error: LlmError, out: &mut Vec<Effect>) {
        emit(
            out,
            AgentEvent::Error {
                code: error.code().to_string(),
                message: error.to_string(),
            },
        );
        self.finish(
            Outcome::Failed {
                error: RunError::Model { error },
            },
            out,
        );
    }

    fn apply_events(
        &mut self,
        mut turn: ModelTurn,
        events: Vec<LlmStreamEvent>,
        out: &mut Vec<Effect>,
    ) {
        for event in events {
            if let Err(error) = turn.apply(event, out) {
                self.finish(Outcome::Failed { error }, out);
                return;
            }
        }
        self.state = State::AwaitingModel { turn };
    }

    /// Compress if needed, then ask for the next model call.
    fn begin_iteration(&mut self, out: &mut Vec<Effect>) {
        if let Some(c) = maybe_compress(
            &self.config.system_instructions,
            &mut self.conversation,
            &self.config.token_budget,
        ) {
            emit(
                out,
                AgentEvent::ContextCompressed {
                    original_tokens: c.original_tokens,
                    compressed_tokens: c.compressed_tokens,
                    turns_summarized: c.turns_summarized,
                },
            );
        }
        let tools = self.config.tools.clone();
        let tool_choice = (!tools.is_empty()).then(ToolChoice::auto);
        let request = ModelRequest {
            model: self.config.model.clone(),
            messages: build_messages(&self.config.system_instructions, &self.conversation),
            tools,
            tool_choice,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: self.config.stream,
        };
        self.state = State::AwaitingModel {
            turn: ModelTurn::default(),
        };
        out.push(Effect::CallModel { request });
    }

    /// Record the model's answer and either run its tool calls or end the step.
    fn after_model(
        &mut self,
        text: String,
        tool_calls: Vec<ToolCall>,
        finish_reason: Option<String>,
        usage: Option<LlmUsage>,
        out: &mut Vec<Effect>,
    ) {
        if let Some(u) = usage {
            emit(
                out,
                AgentEvent::TokenUsage {
                    input_tokens: u.input_tokens,
                    output_tokens: u.output_tokens,
                    total_tokens: u.total_tokens,
                },
            );
        }

        if tool_calls.is_empty() {
            self.conversation.push(Turn::assistant(text));
            self.finish_step(finish_reason, false, out);
            return;
        }

        let ctx_calls = tool_calls
            .iter()
            .map(|tc| ContextToolCall {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                arguments: tc.function.arguments.clone(),
            })
            .collect();
        self.conversation
            .push(Turn::assistant_with_tool_calls(text, ctx_calls));
        self.state = State::AwaitingTool {
            round: ToolRound {
                calls: tool_calls,
                next: 0,
                results: Vec::new(),
                finish_reason,
                started_at_ms: self.now_ms,
            },
        };
        self.advance_tools(out);
    }

    /// Hand the next offered tool call to the host, answering calls to tools
    /// the model was not offered on the spot. When no call is left, record the
    /// results and end the step.
    fn advance_tools(&mut self, out: &mut Vec<Effect>) {
        let State::AwaitingTool { round } = &mut self.state else {
            return;
        };
        while let Some(tc) = round.calls.get(round.next) {
            let call_id = tc.id.clone();
            let tool_name = tc.function.name.clone();
            let args: Value = serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);

            emit(
                out,
                AgentEvent::ToolUse {
                    tool_name: tool_name.clone(),
                    tool_input: args.clone(),
                    call_id: call_id.clone(),
                },
            );

            let offered = self
                .config
                .tools
                .iter()
                .any(|t| t.function.name == tool_name);
            if offered {
                round.started_at_ms = self.now_ms;
                out.push(Effect::CallTool {
                    call_id,
                    name: tool_name,
                    arguments: args,
                });
                return;
            }

            // ADR 0022: a call to a tool the model was never offered is the
            // loop refusing, not the tool failing. Record the refusal as a
            // hook before the error result it produces.
            emit(
                out,
                AgentEvent::Hook {
                    phase: HookPhase::BeforeTool,
                    hook_name: hook_names::TOOL_DISPATCH.to_string(),
                    action: HookAction::Blocked,
                    payload: Some(serde_json::json!({
                        "call_id": call_id,
                        "tool_name": tool_name,
                        "reason": "unknown_tool",
                    })),
                },
            );
            let output = format!("unknown tool: {tool_name}");
            emit(
                out,
                AgentEvent::ToolResult {
                    call_id: call_id.clone(),
                    output: output.clone(),
                    is_error: true,
                    duration_ms: 0,
                    started_at_ms: self.now_ms,
                },
            );
            round.results.push(ContextToolResult {
                call_id,
                output,
                is_error: true,
            });
            round.next += 1;
        }

        let State::AwaitingTool { round } = std::mem::replace(&mut self.state, State::Finished)
        else {
            unreachable!("checked above");
        };
        self.conversation.push(Turn::tool_result(round.results));
        self.finish_step(round.finish_reason, true, out);
    }

    /// Emit `StepFinish`, then finish the run or begin the next iteration.
    fn finish_step(
        &mut self,
        finish_reason: Option<String>,
        had_tool_calls: bool,
        out: &mut Vec<Effect>,
    ) {
        let done = finish_reason.as_deref() == Some("stop") && !had_tool_calls;
        emit(
            out,
            AgentEvent::StepFinish {
                iteration: self.iteration,
                finish_reason: finish_reason.unwrap_or_else(|| "unknown".to_string()),
            },
        );

        if done {
            self.finish(Outcome::Completed, out);
            return;
        }

        let max = self.config.max_iterations;
        if self.iteration + 1 >= max {
            if had_tool_calls {
                emit(
                    out,
                    AgentEvent::Error {
                        code: "E_AIKIT_MAX_ITERATIONS".to_string(),
                        message: format!("exceeded max iterations ({max})"),
                    },
                );
                self.finish(
                    Outcome::Failed {
                        error: RunError::MaxIterations { max },
                    },
                    out,
                );
            } else {
                self.finish(Outcome::Completed, out);
            }
            return;
        }

        self.iteration += 1;
        self.begin_iteration(out);
    }
}
