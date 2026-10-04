# AIKIT SDK — Agent Runner

The uniform layer for driving external coding-agent CLIs over a transport, decoding their heterogeneous output into one canonical event vocabulary, and exposing it to callers (serve, agentrt, the optimization loop).

## Language

**Backend**:
A runnable agent that aikit drives over a Transport — Claude, Codex, Gemini, OpenCode, Cursor, Pi, and the built-in `aikit`. A Backend is an identity (the closed set, parsed from a key string) that produces a Transport, a Decoder, and a declared set of capabilities. The built-in `aikit` is the in-process Backend: it establishes an in-process Transport and emits canonical events directly (no Dialect to decode), and is the richest Backend (tools, subagents, context compression, step lifecycle).
_Avoid_: Codec, provider (that's the LLM-gateway layer), adapter, engine

**Transport**:
How a Backend's channel is established and how messages move across it — in **both** directions. Two impls exist initially: subprocess-stdout-lines (spawn the CLI, build its argv, read newline-delimited output) for the six external Backends, and in-process (direct canonical emission) for the built-in `aikit`. The seam is designed so JSON-RPC-over-stdio (Codex `app-server`), the Claude SDK, websockets, and unix sockets plug in later as additional Transports without reworking Backends. A Transport splits into a reader half (inbound messages) and a writer half (outbound). Modelled on `claude-agent-sdk-rust`'s `Transport`/`TransportReader`/`TransportWriter`.
_Avoid_: Launch, spawn-spec (spawning is just the subprocess Transport's connect step), channel

**Decode**:
Translating one inbound message from a Backend's Dialect into canonical output: zero or more `Decoded` frames (`Stream`, `ToolUse`, `ToolResult` — see [ADR 0010](../docs/adr/0010-decode-emits-typed-frames.md)), an optional `TokenUsage`, and an optional quota signal. Pure and side-effect-free. A Backend's decoder may delegate to a dedicated typed parser (`claude-agent-sdk-rust::parse_message`, `aikit-agent-codex` events) rather than poke at `serde_json::Value`. Claude, Pi and Codex emit typed tool frames; the remaining backends still produce only `Stream`. Text is never promoted to a tool frame, and a decodable line is never dropped.
_Avoid_: Parse, normalize (normalize is the legacy function name being retired)

**Dialect**:
A Backend's native, per-agent message format — e.g. Claude's `stream-json` frames, Codex `app-server`'s JSON-RPC notifications. Each Backend speaks one Dialect; Decode translates a Dialect into the canonical vocabulary. Some Dialects carry far more structure (tool calls, reasoning, content blocks, approvals) than others.
_Avoid_: Schema, format, protocol (the canonical side is the protocol; the per-agent side is the Dialect)

**Control**:
The outbound, interactive axis of a bidirectional Backend: answering approval/permission requests, sending interrupts, and driving turn/session lifecycle. A control operation may be accepted for delivery before the agent acknowledges it; acceptance and completion are distinct outcomes.
_Avoid_: Command channel, RPC (RPC is one possible Transport, not the concept)

**Canonical agent-event vocabulary**:
The agent-agnostic frame set every Dialect decodes into (`StreamMessage` plus the `AgentEventPayload` variants). Defined by the Event Streaming Protocol — see [ADR 0005](../docs/adr/0005-agent-events-are-the-shared-streaming-protocol.md). The closed set of Backends is an SDK-internal concern; this vocabulary is deliberately open and shared with other runtimes.
_Avoid_: Normalized output, common format

**Backend capability**:
A declared property of a Backend that callers gate behaviour on — e.g. whether it speaks a bidirectional transport, emits structured tool calls, emits reasoning, or is interruptible. Lets a caller subscribe to (or require) richer behaviour only from Backends that actually provide it, instead of assuming the lowest common denominator.
_Avoid_: Feature flag, trait (it describes a Backend, it is not the Rust trait)

**Host session**:
A conversation owned by one workspace host and driven by one session backend. A client views or controls the session; the client's connection does not own its lifetime. Native agent session identity and public host session identity are separate.
_Avoid_: Connection, process, chat window

**Session backend**:
A coding agent reachable through a host session. Its capabilities describe operations available in that session, which can differ from those available through a standalone run.
_Avoid_: Model, LLM provider

**Command receipt**:
The host's recorded disposition of a client action. Durable acceptance, dispatch to the backend, failure, and an uncertain outcome are different dispositions. Repeating the same command identity does not request another execution.
_Avoid_: Turn result, completion

**Pending request**:
An agent's permission request or question awaiting an authorized response before its deadline. A request belongs to one host session and is resolved once; an expired or cancelled request cannot authorize later work.
_Avoid_: Command, notification
