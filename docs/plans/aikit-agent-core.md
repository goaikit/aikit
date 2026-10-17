# aikit-agent-core: the agent loop as an I/O-free step machine

Status: core and driver implemented; the CI and publish workflow changes are pending (see Delivery). Date: 2026-10-17. First consumer: Copane (the on-device Copilot), whose
ADR 0005 "The Copilot runs on the device, on aikit-agent-core" asks for this crate.

## Problem

`aikit-agent` runs its loop (`loop_runner::run_loop`) as blocking straight-line code. The loop
does its own I/O: it calls the model through `reqwest` inside a private tokio runtime, runs
file and bash tools, measures time with `Instant` and `SystemTime`, and spawns sub-agents as
nested blocking runs. None of that builds for `wasm32-unknown-unknown` or fits a uniffi
callback, so a browser or Android host can't reuse the loop. Writing a second loop per
platform means keeping several loops in step.

The streaming path also buffers: `OpenAiCompatProvider::stream` reads the whole response body
and only then parses it with `parse_sse_body`, so nothing can be shown before the model is done.

## Solution

A new workspace crate, `aikit-agent-core`, holds the pure parts of the loop. `aikit-agent`
becomes a native, blocking driver over it, with an unchanged public API.

### Rules for the core

- No tokio, reqwest, `std::fs`, `std::process`, `std::thread`, `std::net` or `std::time`. The
  only dependencies are `serde`, `serde_json` and `thiserror`.
- It builds for `wasm32-unknown-unknown`. CI checks this on every pull request, and fails if
  the source names one of the forbidden `std` modules.
- The core never reads a clock. The host sends the time as an input.
- Every effect and input is `serde`, so a host can cross an FFI boundary as JSON.

### What moves into the core

| Module | Contents |
|---|---|
| `llm::types` | `LlmMessage`, `LlmRequest`, `LlmResponse`, `LlmStreamEvent`, `LlmUsage`, `LlmError`, `ToolCall`, `ToolDefinition`, `LlmStreamHandle`, ... (now also `serde` where they were not) |
| `llm::stream` | `parse_sse_line`, `parse_sse_chunk`, `parse_sse_body`, and the new incremental `SseParser` |
| `llm::wire` | The OpenAI chat-completions wire shapes: the request body both hosts send, and the response shapes |
| `context` | `Turn`, `TurnRole`, `ContextToolCall`, `ContextToolResult`, `TokenBudget`, token estimates |
| `compression` | Summarising the oldest turns when the conversation is over budget |
| `machine` | `AgentCore`: max iterations, tool-call handling, finish conditions, events |
| crate root | `AgentEvent` (today's `AgentInternalEvent`), `HookPhase`, `HookAction`, `hook_names` |

`aikit-agent` re-exports each of these at its old path (`aikit_agent::llm::types::LlmRequest`,
`aikit_agent::context::Turn`, `aikit_agent::AgentInternalEvent`, ...), so consumers compile
unchanged.

### The step machine

```rust
let mut core = AgentCore::new(config, turns);   // turns: prior conversation + the new prompt
let mut effects = core.start();
// then, for every input the host has:
effects = core.on(input)?;
```

`start` and `on` return effects in order. The host carries them out in that order:

- `Emit { event }`: hand the event to the UI or the caller.
- `CallModel { request }`: send the request to the model. `request.stream` says whether to
  stream. `request.wire_body_json()` is the exact OpenAI chat-completions body to POST.
- `CallTool { call_id, name, arguments }`: run the tool, then send `ToolResult`.
- `Done { outcome }`: the run is over: `Completed`, `Failed { error }` or `Cancelled`.

Inputs:

- `ModelLine { line }`: one raw SSE line, as it arrives. Lines after `data: [DONE]` are ignored.
- `ModelEvent { event }`: an already-parsed `LlmStreamEvent`, for a host whose transport parses.
- `ModelResponse { response }`: the whole answer of a non-streaming call.
- `ModelEnd`: the stream is over.
- `ModelError { error }`: the call failed (`LlmError`).
- `ToolResult { call_id, result, timing }`: the tool's output. `timing` is optional; without
  it the core times the tool with the `Now` inputs it has seen.
- `Now { unix_ms }`: the current time. Allowed in every state.
- `Cancel`: stop now; the core answers `Done { outcome: Cancelled }`.

`on` returns `Err(InputError)` for an input the current state can't take, such as a
`ToolResult` for a call the core didn't ask for, or any input after `Done`.

The core asks for one thing at a time: at most one `CallModel` or `CallTool` is outstanding,
and it is always the last effect of a batch. Tool calls of one model answer run one after the
other, as today, so the event order is the same as today's loop:
`ToolUse`, (a `tool_dispatch` hook for an unknown tool), `ToolResult`, then the next call.
A call to a tool the model was not offered never reaches the host: the core answers it with
an `unknown tool: <name>` error result itself.

### For an on-device host (Copane)

- The JSON tags are `effect` (`call_model`, `call_tool`, `emit`, `done`), `input`
  (`model_line`, `model_event`, `model_end`, `model_response`, `model_error`, `tool_result`,
  `now`, `cancel`), `type` on `AgentEvent` and `LlmStreamEvent`, `status` on `Outcome` and
  `kind` on `RunError`, `LlmError` and `InputError`.
- `CallModel.request` has no URL or key. A host POSTs `wire_body_json()` to
  `{base_url}/chat/completions` with its own credentials, then feeds each response line as
  `ModelLine` as soon as it arrives (or the whole JSON answer as `ModelResponse` when
  `request.stream` is false), and `ModelEnd` when the body ends. A transport failure or a
  non-2xx answer is a `ModelError`.
- Send `Now` before `ModelEnd`, `ModelResponse` and `ToolResult` if the host doesn't time
  tools itself; the core uses it for `ToolResult.started_at_ms` and `duration_ms`.
- `AgentCore` is `serde` as a whole, so a host can save a run between inputs (a page reload,
  an Android process restart) and continue it.
- After `Done`, read the conversation with `AgentCore::conversation()` to keep it for the
  next prompt.

### The driver

`aikit-agent` keeps everything that does I/O or is native-only:

- the HTTP client (`OpenAiCompatProvider`) and the `LlmGateway` trait;
- the tools (`read_file`, `write_file`, `run_bash`, `git`, `read_skill`, host tools) and
  their sandbox checks;
- skill discovery, AGENTS.md reading and the system prompt;
- the run-start events: `HarnessSnapshot` and the `persona_tool_policy` hook;
- sub-agents (see Gaps).

`run_loop` builds an `AgentCore`, runs its effects with the gateway and the tools, and feeds the
results back. It sends the tool's measured `timing` (an `Instant` around `execute`, as today),
and a `Now` before each completion and tool result. `run`, `run_streaming`,
`run_with_context` and `run_with_context_streaming` keep their signatures, events and errors.

The driver still gets the stream through `LlmGateway::stream`, which returns parsed
`LlmStreamEvent`s, and feeds them as `ModelEvent`. `OpenAiCompatProvider` still reads the whole
body before parsing; making the native provider incremental is a follow-up and doesn't change
the core.

## Gaps

- **Sub-agents stay in the driver.** `spawn_subagent` is an ordinary tool of the driver: it runs
  a nested blocking `run`. To the core it is just a `CallTool`. An on-device host that wants
  sub-agents has to run a second `AgentCore` behind a tool of its own. A core-level design
  (a `SpawnAgent` effect with its own budget) is left for when a host needs it.
- **No parallel tool calls.** Calls run in order, as today.
- **Non-OpenAI streams** are out of scope: `SseParser` reads OpenAI chat-completions chunks.
- **System prompt assembly** (persona prompt, AGENTS.md, skills catalog) stays in the driver,
  because it reads files. A host passes its finished system prompt in `CoreConfig`.

## Delivery

1. Pull request 1: the `aikit-agent-core` crate, this spec and the core tests (scripted inputs, no
   network).
2. Pull request 2: `aikit-agent` runs on the core. Its own tests stay as they are and pass.
3. A workflow change, pushed by someone allowed to change `.github/workflows/`:
   - `ci.yml`: a job that builds the core for `wasm32-unknown-unknown` and fails if its source
     names `std::fs`, `std::process`, `std::thread`, `std::time` or `std::net`;
   - `publish-crates.yml`: publish `aikit-agent-core` before `aikit-agent`, which now depends
     on it.

## Verification

- `cargo test -p aikit-agent-core` and `cargo test -p aikit-agent`.
- `cargo build -p aikit-agent-core --target wasm32-unknown-unknown`.
- `cargo test --workspace --all-features` for the consumers (`aikit-sdk`,
  `aikit-session-summarize`).
