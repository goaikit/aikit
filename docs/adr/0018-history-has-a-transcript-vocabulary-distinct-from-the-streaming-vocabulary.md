# History has a transcript vocabulary distinct from the streaming vocabulary

## Status

proposed

## Context

aikit already has a canonical **streaming** vocabulary for a *live* run:
`Backend::decode` emits `Decoded` frames ([ADR 0010](0010-decode-emits-typed-frames.md))
which the run loop maps to `AgentEventPayload` — a flat, ordered event stream
optimized for incremental delivery over the Event Streaming Protocol
([ADR 0005](0005-agent-events-are-the-shared-streaming-protocol.md)).

[Spec 008](../../specs/008-history-backend/spec.md) adds a *history* reader that
returns a **past** transcript read from disk. The obvious move is to reuse the
streaming vocabulary — return `Vec<AgentEventPayload>` from `messages()` — so
there is "one event type." That is a trap. The two describe different things:

- The streaming vocabulary is **flattened for incremental emission**: one
  assistant turn fans out into several separate frames (a text frame, a tool-use
  frame, later a tool-result frame), deliberately losing the turn's block
  grouping because a live consumer renders frames as they arrive.
- A transcript is **already complete and grouped**: one assistant message owns
  an *ordered list of blocks* (text, thinking, tool_use, tool_result, server
  tools). A renderer of history wants the message with its blocks intact, not a
  re-flattened frame soup it must regroup by `call_id`.

Forcing history through `AgentEventPayload` would (a) discard the block grouping
the on-disk transcript already has, (b) couple the persisted-history API to the
live-streaming frame shape, so any streaming-protocol change would be a
breaking change to history, and (c) misrepresent a stored artifact as a live
stream.

## Decision

History defines its own **transcript** vocabulary — `HistoryMessage` carrying an
ordered `HistoryContent` (`Text` | `Blocks(Vec<HistoryBlock>)` | `Raw`) — and
does **not** reuse `AgentEventPayload`. The two vocabularies coexist by design:
*streaming = flat ordered frames; transcript = grouped ordered blocks.*

They share exactly one type and no more: **`MessageRole`** (`Assistant` | `Tool`
| `System` | `User`) is reused verbatim, so a role means the same thing in both
worlds. `HistoryBlock` is a 1:1 image of the SDK's `ContentBlock` taxonomy under
aikit names, produced by running the stored `message: Value` through
`claude_agent_sdk::parse_message` — the *same typed parser* the live decode path
uses ([ADR 0011](0011-claude-decode-delegates-to-sdk.md)) — but mapped directly
to blocks instead of being flattened into `Decoded` frames. So the two paths
agree at the parser level while diverging in shape above it.

## Consequences

- Two vocabularies to learn, justified: a client rendering *live* runs and a
  client rendering *history* legitimately want different shapes, and each gets
  the one that fits. The shared `MessageRole` keeps role semantics from drifting
  apart.
- History is decoupled from the streaming protocol: evolving `AgentEventPayload`
  / `Decoded` never breaks the persisted-history API, and vice versa. Both
  `HistoryContent` and `HistoryBlock` are `#[non_exhaustive]`, so the taxonomy
  can grow additively.
- Block-level fidelity matches the on-disk transcript, and `HistoryBlock::Raw`
  (plus `HistoryContent::Raw`) guarantees no turn is ever dropped when a future
  block type appears before the taxonomy learns it.
- Cost: the block-mapping logic is a second consumer of `parse_message`
  alongside the frame-flattening decode path. This is accepted precisely so the
  two stay parser-aligned; it is not free duplication.
- Reconsider if a concrete client ever needs to render live and historical
  content through one code path — at which point a shared lower-level block type
  (below both frames and messages) would be the merge point, not collapsing one
  vocabulary into the other.
