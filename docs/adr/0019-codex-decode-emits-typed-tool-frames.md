# Codex decode emits typed tool frames, and a tool call is what evals count

## Status

accepted

## Context

[ADR 0010](0010-decode-emits-typed-frames.md) introduced `Decoded` and noted that the five non-Claude backends were left wrapping their `StreamMessage`s as `Decoded::Stream`. [ADR 0017](0017-pi-backend-drives-rpc-session-mode.md) then made Pi emit structured tool frames. Codex remained wrapped, and that had a consequence ADR 0010 did not anticipate.

`aikit-evals` counts tool activity from the trace. Before PR #148 the trace had a `RawJson` catch-all and `max_command_count` counted it, so an agent's prose inflated the count — a real Claude run reporting `command_count: 2` for two text messages and zero tools. #148 fixed the payload side: `Message`, `ToolUse`, `ToolResult` and an explicit `Unknown` replaced the catch-all, and counting moved to tool frames.

That left codex counting **zero, structurally and permanently**. Its `decode` returned only `Vec<StreamMessage>`, so `command_execution`, `file_change` and the legacy `action`/`output` lines all arrived as text, landed in `TracePayload::Message`, and were by definition not counted. A `max_command_count` check on a codex suite could not fail no matter what the agent did: it was not a loose limit, it was an inert one. The check's name compounded this — "command count" invited reading it as "shell commands", when the quantity that matters to a limit is tool invocations.

## Decision

**Codex decodes structured tool frames.** `codex::decode` returns `Vec<Decoded>` and the Codex arm of `Backend::decode` no longer wraps. `command_execution` yields a `ToolUse` plus a `ToolResult` for its `aggregated_output`; `file_change` yields a `ToolUse` (applying a patch is a tool call); the legacy `action`/`output` schema yields the same pair. `agent_message`, `reasoning` and unknown item types stay `Decoded::Stream` — agent prose is never promoted to a tool call.

Frames correlate by the item's own `id`, falling back to a value derived from `raw_line_seq`. Fallback ids are **derived, never random**, so a trace is reproducible and re-scoring a recorded run is deterministic.

**Nothing decodable is dropped.** An item carrying output but no `command`, and an `action` whose name we do not recognise, are both recorded rather than discarded — the latter under its own tool name. This extends #148's principle: an unmodelled variant is preserved as itself, because a trace that silently omits activity understates what the agent did, and understating is the failure mode evals exist to prevent.

**The check is `max_tool_calls`.** `max_command_count` remains a serde alias, so existing `checks.toml` files parse unchanged; the canonical name and the emitted `check_name` are the new one.

**A tool call is `tool_use` + `raw_json`.** `raw_json` stays in the count because backends that emit tool calls as raw JSON would otherwise drop to zero — reintroducing, elsewhere, exactly the blindness this fixes. Text, token-usage and `unknown` payloads are not tool calls.

## Consequences

- Codex suites report non-zero tool counts for the first time. Any `max_tool_calls` limit that passed vacuously on codex is now a real constraint and must be re-baselined against a live run — a limit tuned against a count of zero means nothing.
- The `command_count` field in eval artifacts keeps its name. Artifacts are a consumed contract (`fastskill` reads them); renaming a configuration knob does not justify breaking readers. The field name and the check name deliberately diverge, and this ADR is where that divergence is recorded.
- ADR 0010's statement that the non-Claude backends are unchanged now holds only for Gemini, OpenCode, Cursor and the in-process Aikit backend. Codex and Pi emit typed frames; that ADR's decision stands, its inventory has moved on.
- The legacy `action`/`output` schema carries no id and decoding is per-line and stateless, so those frames share a constant fallback `call_id` and pair only approximately within a multi-command session. The current `item.completed` schema is exact — both frames come from one item. Correlating the legacy schema properly would require decode to hold state across lines, which is not worth it for a superseded schema.
- Codex already declared `.with_structured_tools()` in its `BackendCapabilities` while its decoder emitted none. The capability was aspirational; it is now accurate. Capability flags describe the decoder, so a flag that outruns its decoder is a bug, not a roadmap.
- Downstream scoring gets a truthful signal on codex, which matters most to `aikit-textgrad`: the gate reduces checks to a scalar, and a check that could never fail contributed a constant.
