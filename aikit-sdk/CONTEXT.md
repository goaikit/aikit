# AIKIT SDK — Agent Runner

The uniform layer for driving external coding-agent CLIs over a transport, decoding their heterogeneous output into one canonical event vocabulary, and exposing it to callers (serve, agentrt, the optimization loop).

## Language

**Backend**:
A runnable agent that aikit drives over a Transport — Claude, Codex, Gemini, OpenCode, Cursor, and the built-in `aikit`. A Backend is an identity (the closed set, parsed from a key string) that produces a Transport, a Decoder, and a declared set of capabilities. The built-in `aikit` is the in-process Backend: it establishes an in-process Transport and emits canonical events directly (no Dialect to decode), and is the richest Backend (tools, subagents, context compression, step lifecycle).
_Avoid_: Codec, provider (that's the LLM-gateway layer), adapter, engine

**Transport**:
How a Backend's channel is established and how messages move across it — in **both** directions. Two impls exist initially: subprocess-stdout-lines (spawn the CLI, build its argv, read newline-delimited output) for the five external Backends, and in-process (direct canonical emission) for the built-in `aikit`. The seam is designed so JSON-RPC-over-stdio (Codex `app-server`), the Claude SDK, websockets, and unix sockets plug in later as additional Transports without reworking Backends. A Transport splits into a reader half (inbound messages) and a writer half (outbound). Modelled on `claude-agent-sdk-rust`'s `Transport`/`TransportReader`/`TransportWriter`.
_Avoid_: Launch, spawn-spec (spawning is just the subprocess Transport's connect step), channel

**Decode**:
Translating one inbound message from a Backend's Dialect into canonical output: zero or more `StreamMessage`s, an optional `TokenUsage`, and an optional quota signal. Pure and side-effect-free. A Backend's decoder may delegate to a dedicated typed parser (`claude-agent-sdk-rust::parse_message`, `aikit-agent-codex` events) rather than poke at `serde_json::Value`.
_Avoid_: Parse, normalize (normalize is the legacy function name being retired)

**Dialect**:
A Backend's native, per-agent message format — e.g. Claude's `stream-json` frames, Codex `app-server`'s JSON-RPC notifications. Each Backend speaks one Dialect; Decode translates a Dialect into the canonical vocabulary. Some Dialects carry far more structure (tool calls, reasoning, content blocks, approvals) than others.
_Avoid_: Schema, format, protocol (the canonical side is the protocol; the per-agent side is the Dialect)

**Control**:
The outbound, interactive axis of a bidirectional Backend: answering approval/permission requests, sending interrupts, driving turn/session lifecycle. Shaped for by the Transport's writer half but not implemented in the initial refactor — the current Backends are one-shot and read-only.
_Avoid_: Command channel, RPC (RPC is one possible Transport, not the concept)

**Canonical agent-event vocabulary**:
The agent-agnostic frame set every Dialect decodes into (`StreamMessage` plus the `AgentEventPayload` variants). Defined by the Event Streaming Protocol — see [ADR 0005](../docs/adr/0005-agent-events-are-the-shared-streaming-protocol.md). The closed set of Backends is an SDK-internal concern; this vocabulary is deliberately open and shared with other runtimes.
_Avoid_: Normalized output, common format

**Backend capability**:
A declared property of a Backend that callers gate behaviour on — e.g. whether it speaks a bidirectional transport, emits structured tool calls, emits reasoning, or is interruptible. Lets a caller subscribe to (or require) richer behaviour only from Backends that actually provide it, instead of assuming the lowest common denominator.
_Avoid_: Feature flag, trait (it describes a Backend, it is not the Rust trait)
