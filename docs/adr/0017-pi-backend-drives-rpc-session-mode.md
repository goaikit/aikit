# The Pi backend drives RPC mode as a bidirectional session, not json event-stream

## Status

accepted

## Context

Pi exposes two headless surfaces: `pi --mode json` (a one-shot stdout event stream, positional prompt) and `pi --mode rpc` (JSON-RPC over stdio: commands on stdin, events on stdout). They are not equivalent. The json stream is bounded by the kernel `ARG_MAX` (the prompt is a positional argument) and omits token usage from its events — usage there is plugin-only. The RPC protocol instead frames the prompt as a `{"type":"prompt","message":…}` command over stdin (no `ARG_MAX` ceiling), carries `usage` + `cost` on every `AssistantMessage`/`ToolResultMessage` and via `get_session_stats`, and exposes a Control axis: `abort`, `steer`, `follow_up`, and `extension_ui_request` `select`/`confirm` (a permission/approval channel).

The SDK already has two run shapes: the subprocess-stdout-lines path (`run_agent_events`, used by opencode/gemini/cursor) and the bidirectional session path (`open_codex_session`/`open_claude_session`, each a bespoke client implementing the `LiveSession` trait). There is no shared JSON-RPC Transport — every session backend hand-rolls its framing.

## Decision

Pi is added as the seventh Backend (`Backend::Pi`, key `"pi"`) driven exclusively through RPC mode as a bidirectional session — a new `pi_session.rs` implementing `LiveSession`, mirroring `codex_session.rs`, hand-rolling the JSONL framing (no `aikit-agent-pi` crate exists). The one-shot `--mode json` subprocess-lines path is **not** shipped for Pi: it is strictly inferior (no usage, `ARG_MAX`-bounded). Capabilities `bidirectional`, `interruptible`, `structured_tools`, `reasoning`, `resumable_sessions`, and `context_compression` are declared `true` because the RPC events back each one, and `extract_usage` reads `AssistantMessage.usage` so Pi reports per-message and aggregate token usage and cost.

Session/app-server style is the preferred target shape for every backend that supports it; Pi is the vanguard. Migration of the remaining subprocess-lines backends (codex already has both paths; opencode, claude, gemini, cursor) to session style is a follow-on program, tracked separately — not in scope for adding Pi.

## Consequences

- Pi is architecturally codex/claude-tier, not opencode/gemini-tier: the cost is a bespoke session client (~`codex_session.rs` scale) rather than a small `backends/pi.rs` decode-and-argv file.
- Because Pi is session-only, the one-shot `run_agent_events` surface (what `aikit agent run -a pi` calls) must dispatch to `open_pi_session`; the subprocess-lines path does not serve Pi.
- The strategic direction (all capable backends → session style) will, when acted on, make the subprocess-lines Transport and the per-backend `decode()`/`build_argv` arms legacy. That retirement is deferred until each backend migrates.
- Pi is usage-reporting — the earlier assumption that Pi is "usage-blind" applied only to json-mode and is rejected here.

## Implementation (spec 014)

This ADR records the core transport decision (RPC session, not json event-stream). The shipping implementation refines two details, captured in spec 014:

- **Run path.** Rather than a bespoke `pi_session.rs` client for the one-shot `run_agent_events` surface, the subprocess transport is extended with two per-Backend hooks — `stdin_prompt_bytes` (frames the prompt as a JSON-RPC command) and `is_terminal_event` (drains until `agent_settled`, then tears the long-lived RPC server down). This reuses the proven drain/watchdog/cancel machinery instead of duplicating it. A bespoke `LiveSession` client for the interactive `serve`/`session` surface remains future work, mirroring how `codex`/`claude` grew theirs incrementally.
- **`context_compression` is `false` for v1** (consistent with the other external Backends): compaction surfaces as a status message, not a structured compression frame. `false -> true` is a non-breaking change later. The other capabilities (`bidirectional`, `interruptible`, `structured_tools`, `reasoning`, `resumable_sessions`) are `true` as decided here.
