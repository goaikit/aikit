# Decode emits typed canonical frames, not just StreamMessages

## Status

accepted

## Context

Phase A's `Backend::decode` returned `Vec<StreamMessage>` — a text-only lowest common denominator. To carry structured tool calls (and to let richer backends like Claude surface what they already emit), a single decoded line must be able to yield more than text: a tool call has a `call_id`, `tool_name`, and structured `input` that `StreamMessage { text, phase, role, kind }` cannot represent. This is the producer-side gap that [spec 005](../../specs/005-runner-structured-events/spec.md) identified.

## Decision

`Backend::decode` returns `Vec<Decoded>`, where `Decoded` is a small canonical enum: `Stream(StreamMessage)`, `ToolUse { call_id, tool_name, input }`, `ToolResult { call_id, output, is_error }`. The run loop maps each frame to an `AgentEventPayload`; two new **generic** payload variants — `AgentEventPayload::ToolUse` and `ToolResult` — carry tool frames for *external* backends (distinct from the existing `Aikit*` variants used by the in-process agent). The empty-text filter applies only to `Decoded::Stream`; tool frames always pass through.

The five non-Claude backends are unchanged in behaviour: their `decode` still produces only `StreamMessage`s, which `Backend::decode` wraps as `Decoded::Stream`. The legacy `normalize_json_line` helper keeps its `Vec<StreamMessage>` signature by projecting out `Stream` frames; structured frames are observed via `run_agent_events`.

## Consequences

- Additive on the wire (the Event Streaming Protocol is an open tagged-frame format — [ADR 0005](0005-agent-events-are-the-shared-streaming-protocol.md)); existing consumers that match `StreamMessage` are unaffected. `AgentEventPayload` is `#[non_exhaustive]`, so external matches already tolerate the new variants.
- `serve`'s SSE translation does not yet special-case the new frames (they fall through its wildcard); surfacing them as richer SSE is a follow-on (spec 005 E1/E3).
- Unifying token-usage and quota into the same `Decoded` stream was considered and deferred: keeping `extract_usage`/`extract_quota` separate preserves the run loop's event ordering and the `emit_token_usage_events` semantics exactly.
