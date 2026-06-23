# Claude decode delegates to the claude-agent-sdk parser

## Status

accepted

## Context

aikit's hand-rolled Claude decode handled only `assistant` text and `result`, silently dropping thinking, tool calls, tool results, and server (advisor) tools that the `claude` CLI already emits in `stream-json`. A maintained typed parser exists — `claude-agent-sdk` (`/home/sysuser/ws001/aroff/claude-agent-sdk-rust`), whose `parse_message(&Value) -> Result<Option<Message>>` is **sync and pure** and yields the full typed `Message`/`ContentBlock` taxonomy with 1:1 wire compatibility. This is the Transport-seam payoff anticipated in [ADR 0007](0007-backend-transport-seam.md) (Phase B).

## Decision

`backends/claude.rs::decode` delegates to `claude_agent_sdk::parse_message`, then maps the typed `Message` to canonical [`Decoded`](0010-decode-emits-typed-frames.md) frames: Text → `Stream` (Delta), Thinking → `Stream` (Reasoning), tool_use / server_tool_use → `ToolUse`, tool_result / advisor_tool_result and `user` tool-result blocks → `ToolResult`, `result` → `Stream` (Final, turn_id = session_id). System / stream_event / rate_limit / task / hook lines yield no message frames — rate-limit stays in `extract_quota` and usage in `extract_usage`, both unchanged. Parse errors and unknown types yield no frames (matching the legacy decoder's silent skip).

The dependency is gated behind a default-on `claude-sdk` feature; with it off, a hand-rolled text-only fallback decode is used, so aikit-sdk still builds without the Claude-specific dependency.

## Consequences

- Text output is behaviour-preserving; thinking and structured tool calls/results are now surfaced (additive). Claude's declared `structured_tools`/`reasoning` capabilities are now genuinely backed.
- aikit-sdk gains an optional dependency on `claude-agent-sdk` (which pulls tokio) when the feature is on. Phase B2 will use the same crate's async `ClaudeSDKClient` + control protocol for the bidirectional/interruptible axis; this ADR covers decode only (no async added yet).
- A `result` line without `session_id` now yields no Final frame (the SDK's `ResultMessage.session_id` is required); in practice the CLI always includes it.
