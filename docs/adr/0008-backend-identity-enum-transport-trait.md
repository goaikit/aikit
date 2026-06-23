# Backend identity is a closed enum; runtime behaviour is a trait

## Status

accepted

## Context

[ADR 0007](0007-backend-transport-seam.md) established a Transport seam and deferred how a Backend is expressed in Rust. Two forces pull opposite ways: the *identity* set of Backends is closed and benefits from compiler-enforced exhaustiveness (no stringly-typed key leakage, no "forgot to wire it up" drift), while the *runtime* behaviour is heterogeneous (one-shot stdout vs stateful bidirectional JSON-RPC client) and benefits from trait-object polymorphism — the pattern `claude-agent-sdk-rust` already uses (`Box<dyn Transport>`).

## Decision

A **hybrid**. `enum Backend { Claude, Codex, Gemini, OpenCode, Cursor }` is the closed identity, parsed once from a key string at the runner boundary; exhaustive `match` drives the static, pure, closed facts — key string, binary candidates, capabilities, and decode dispatch (each arm may delegate to a typed parser). A **`Transport` trait** (reader/writer halves) carries the open, heterogeneous runtime channel: subprocess-stdout-lines now, JSON-RPC/websocket/unix later. `Backend` maps to a transport constructor.

## Consequences

- Adding a Backend is a compile error until its key, binary candidates, capabilities, and decode arm are all supplied — the drift-safety we wanted from an enum.
- Adding a Transport does not touch the `Backend` enum — new transports are new trait impls.
- Decode stays per-Backend free functions dispatched by the enum (pure, closed, delegatable), not a method on the Transport trait — keeping I/O (Transport) and parsing (Decode) separable.
