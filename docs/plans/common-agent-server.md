# AIKit common agent server: implementation plan

Status: per-host implementation delivered in the worktree; bounded validation recorded below. Production fleet qualification remains outstanding.

Repository: https://github.com/goaikit/aikit
Baseline: `c5fcf14d8dcf4656b050ebbbe98b197600f86801`
Branch: `feat/common-agent-server`

## Outcome

Client applications use one versioned contract to discover backends, create sessions, send turns, receive canonical agent events, respond to agent requests, cancel work, and reconnect. Each host supervises its own agent processes and workspaces. Fleet routing references stable host and session identities.

The implementation extends the existing canonical AgentEventPayload vocabulary and Backend/Transport/Control model. Existing canonical payloads are preserved; ACP/OpenCode frames additionally retain their native payloads. New session backend identities and capabilities are independent of the existing one-shot runner registry.

## Agreed decisions

- Deliver the per-host server first, with stable host/session identity; defer the central control plane.
- Keep sessions on the workspace host; no cross-host migration.
- One user/team trust boundary per host; scoped client credentials. Separate isolated hosts for untrusted tenants.
- Client disconnection leaves work running. Persist receipts, events and pending requests; restart reports interruption and uncertain outcomes without automatic replay of actions.
- Cover all nine agents through explicit session capabilities; no implied feature parity.
- Explicit permission policy; interactive requests deny at expiry. No approval inferred from an absent client.
- Linux production target and Windows development support. Synthetic load scenario: 100 independent hosts with 10 sessions each.
- Breaking changes permitted; canonical agent payloads remain intact. The new lifecycle endpoints live under `/api/v1/gateway` and existing CLI-run endpoints retain their original run semantics.

## Backend coverage

| Backend | Existing foundation | Planned integration | Limits to verify |
| --- | --- | --- | --- |
| Claude | SDK session bridge and permission callback | Session factory, request broker, correlated controls | Native acknowledgments, cancellation while permission is pending, resume/fork |
| Codex | app-server JSON-RPC bridge | Session factory, approvals/user input, steer, native error handling | Wire model/resume; distinguish enqueue from native acknowledgment |
| Pi | RPC stdio session bridge | Session factory, steer/follow-up, model/context operations | Wire resume; propagate RPC responses and errors |
| Cursor | CLI JSONL runner | ACP transport and session adapter | Verify launch/version/auth, advertised permissions and resume |
| OpenCode | CLI JSONL runner | Native HTTP/SSE session adapter | Event correlation, permission/question lifecycle, external server ownership |
| Gemini | CLI stream runner | ACP session adapter; native `gemini --acp` confirmed | Never claim persistent control or permissions based on CLI output alone |
| Built-in AIKit | In-process runner | In-process session adapter | Verify cancellation, multi-turn state, tools and resume behavior |
| Grok | No AIKit backend identity | Add identity, discovery and ACP adapter | Verify executable/auth and actual native capabilities |
| Antigravity | No AIKit backend identity | Add identity, discovery and ACP adapter | Verify executable/auth and actual native capabilities |

All nine are the union of the seven AIKit backends and the additional T3 providers previously discussed. Native SDK/CLI/API versions must be recorded for integration tests. T3 supplies implementation evidence; use native documentation to validate protocol assumptions.

## Phase 1 — Contract and state machine

1. Add an HTTP-independent contract module to aikit-sdk, compiled without requiring Claude/Codex optional features.
2. Define host/session/turn/command/request identifiers. Keep native session identifiers separate from public identifiers.
3. Define typed session creation and control requests. Cover send turn, interrupt turn, respond to permission/question, close, inspect, and supported settings. Treat steering and queued follow-up as distinct operations.
4. Define per-session adapter capabilities, separate from existing mixed runner/backend flags. Validate requested options before starting a process; reject unsupported settings explicitly.
5. Wrap canonical AgentEventPayload unchanged in a versioned session envelope. Add orchestration events for command outcomes, session lifecycle, and requests without conflating them with agent output.
6. Define queued/accepted/rejected/completed semantics. A local enqueue acknowledgment cannot be labeled native success. Define session and turn terminal states, error codes, and retryability.
7. Define idempotency scope, request body mismatch behavior, concurrent turns, late responses, and duplicate responses. Return the existing receipt for an identical retry; reject reused keys with a different body.
8. Publish wire examples and machine-readable schemas. Specify how clients handle unknown additive events and unsupported versions.

Acceptance: serialization fixtures and state-machine tests establish identical meaning across SDK, HTTP and every backend. Agent-specific information remains available in typed payloads or namespaced extensions.

## Phase 2 — Session ownership, storage and replay

1. Separate session creation from event subscription. A disconnected viewer must not implicitly destroy the agent session.
2. Introduce a session owner with serialized commands and bounded queues. Hold registry locks only for lookup/reservation; never across process I/O, callbacks, network calls, or database waits.
3. Persist session metadata, command receipts, pending requests and event sequences in a dedicated live-session schema. Reuse SQLite infrastructure where appropriate, keeping passive capture cursors distinct from live event cursors.
4. Assign monotonically increasing sequence numbers at the session owner. Commit durable state before acknowledging durable acceptance.
5. Implement replay followed by live subscription without a gap or duplicate ambiguity. Bound replay pages by count and bytes. Expose an explicit expired-cursor response and snapshot recovery path.
6. Define retention and snapshot compaction. Batch or coalesce text deltas while retaining ordering and final content. Bound both event count and bytes.
7. On restart, reconcile recorded sessions with process/native state. Report interrupted or outcome-unknown commands honestly; never blindly resend an action whose side effect might already have occurred.
8. Release capacity after failed opens, process exits and confirmed closure. Drain gracefully during host shutdown.

Acceptance: duplicate requests, concurrent commands, slow subscribers, reconnect boundaries, disk failures and restart recovery have deterministic tests. No unbounded queue or history accumulation.

## Phase 3 — Existing bridge correctness and requests

1. Route Claude, Codex and Pi through a shared session factory and explicit options mapping.
2. Fix dropped Codex model/resume and Pi resume fields.
3. Propagate interrupt, disconnect and send-turn errors instead of discarding them. Track native responses where the underlying bridge supports them.
4. Add a request broker connecting native permission callbacks to remote client responses. Include deadline, cancellation, session ownership, request identity and one-time resolution.
5. Remove implicit approval caused by missing callbacks in remotely controlled sessions. Apply the configured policy explicitly; pending interactive requests must resolve through an authorized response or policy timeout.
6. Support agent questions separately from permissions, retaining the native answer schema.
7. Ensure interrupt/close can complete while the agent is waiting for a permission response; avoid blocking the only event-reading loop.

Acceptance: scripted fake peers demonstrate allow, deny, timeout, disconnect, duplicate/late response, native rejection and process exit during an outstanding request.

## Phase 4 — Remaining backends

1. Implement reusable ACP transport with initialization/version negotiation, request correlation, bounded framing, notifications, permissions, cancellation, EOF cleanup and process supervision.
2. Add provider-specific Cursor, Grok and Antigravity launch/auth configuration; do not infer executable flags from a generic ACP label.
3. Implement OpenCode HTTP/SSE adapter with bounded reconnect and session-scoped event filtering. Distinguish an owned server process from an externally configured endpoint.
4. Integrate Gemini and built-in AIKit using their actual supported run/session semantics. A supervised single-run mode must advertise that limitation; full native sessions depend on the scope answer and verified backend support.
5. Update exhaustive backend matches, key parsing, catalog/discovery, capabilities, errors and documentation for new identities.

Acceptance: the same conformance suite runs against all nine adapters. Each advertised operation has executable behavioral coverage; unsupported operations fail before side effects.

## Phase 5 — HTTP and client wiring

1. Expose discovery, create/list/get/close session, command submission, pending requests and replayable SSE through the existing server composition and authentication middleware.
2. Validate workspace roots and options at session creation. Bind request authorization to the session rather than trusting body identifiers.
3. Ensure SSE has event IDs, heartbeat, ordered replay, bounded buffering and deterministic cleanup. Slow clients must not block unrelated sessions.
4. Apply per-host capacity limits to every adapter, including concurrent session opens; add request/body/frame size limits and overload responses.
5. Wire the selected compatibility policy through existing messages, sessions, live-sessions, CLI REPL and downstream contract consumers found in the repo.
6. Provide a client example demonstrating creation, disconnect/reconnect, approval response and cancellation. Include web/mobile-friendly wire documentation without claiming native apps are implemented.

Acceptance: HTTP tests cover validation/auth, all backend selections, options round-trip, errors, replay, request resolution and resource cleanup using injected transports.

## Phase 6 — Fleet and operational behavior

Per-host scope: stable host identity, session ownership, health/readiness, capacity and structured metrics; deployment documentation for TLS termination, service supervision, workspace/credential boundaries and rolling drain. CPU/memory and subprocess limits must reflect the operating system capabilities.

If the fleet control plane is selected: add host enrollment, short-lived scoped credentials, host liveness, routing, session directory and partition/lease behavior. Keep agent execution bound to its workspace host. Define unavailable-host behavior explicitly; moving a running process is not automatic failover.

Measure server-added command latency separately from model latency, event lag, queue bytes, active processes, reconnects, rejected opens and persistence latency. Define a repeatable fleet/load harness. A passing local unit test does not establish hundreds-of-host capacity.

## Verification and delivery

1. Run targeted SDK/contract/session tests after each phase.
2. Exercise actual serialized protocols against controlled fake subprocesses/HTTP peers, including partial frames, malformed data, unexpected IDs and process death.
3. Test authorization boundaries, approval races, replay handoff and idempotency under concurrency.
4. Run `cargo fmt --all -- --check` and the repository CI gates: workspace all-features clippy, build, and tests. Check reduced feature configurations for the provider-independent contract.
5. Run opt-in live tests only with available binaries and configured credentials. Report each backend as fixture-validated, live-validated, or unavailable; never infer operational support from compilation.
6. Record reproducible load settings and measured results; list any scale target not validated.
7. Deliver implementation notes, backend coverage matrix, migration guidance, worktree/branch and test evidence. Keep incomplete requirements visible.

## Current preparation evidence

- Real upstream Git clone obtained; isolated worktree created on the baseline above.
- No AIKit AGENTS.md found in the inspected repository or checked parent paths.
- Existing ADR 0005/0016 establishes the canonical agent event vocabulary and forbids lossy remapping.
- Existing live server supports only Claude, Codex and Pi; passive capture persistence does not provide durable live-session replay.
- Scope confirmed: per-host server, host-bound sessions, one trust boundary per host, durable reconnect/restart records, all nine with explicit capabilities, explicit permission policy with timeout denial, Linux production and Windows development, proposed synthetic load target 100 hosts x 10 sessions. Breaking changes permitted.


## Delivery status (2026-09-30)

- Phases 1-3: common contract/schema, durable host ownership/replay, native SDK option/control/request wiring implemented.
- Phase 4: all nine session adapters wired; Claude/Codex passed a live no-tools smoke. ACP/OpenCode protocol fixtures passed. Remaining native installations require live qualification.
- Phase 5: authenticated HTTP/SSE composition, scoped grants, explicit CORS/origins, capability checks and client example implemented. Existing legacy run endpoints retain their separate semantics; no native mobile app was added.
- Phase 6: host identity, admission limits, process ownership, readiness/drain and synthetic harness delivered. Central fleet control remains deferred. Detailed metrics export, complete transcript snapshot compaction and actual Linux/fleet qualification remain outstanding.
- Checks: all-features build and Clippy passed; 242 SDK runner tests, 19 gateway tests and 3 client tests passed. Synthetic 100-store/1000-worker test passed (creation p50 8.656 ms, p95 10.771 ms). This is a local debug-build fixture measurement, not fleet throughput.
- Full workspace tests are not green on this Windows host: Bash/WSL prerequisites, a WSL-home assertion and two stalled archive-install unit targets prevented a clean result. Native-newline formatting passes; the default newline-style gate flags baseline CRLF checkout files.
- Replay retention is bounded with explicit expired-cursor/gap handling. Metadata is not a full transcript snapshot. Commands expose durable acceptance/local dispatch; native completion is reported by events.
- Complete changed-file coverage, commands, evidence boundaries and deployment limitations are in `docs/agent-server.md`.

## Additional pre-PR scope (2026-10-05)

Implemented actual server/client tests, host process crash/restart tests, permission races, overload/slow-reader tests, native write/control deadlines, stable error/retry metadata, host metrics, a dedicated Linux conformance workflow and a systemd deployment example.

The state review also fixed approval scope bypass through generic commands, non-atomic creation records, stranded queued receipts after startup failure, terminal-state regression, turn admission races and explicit close during drain. Final local results: 29 gateway tests, 243 SDK runner tests and four Node tests passed; CLI build, workspace all-features/all-targets Clippy, native-newline format and diff checks passed. Remote Linux CI and actual deployment/native fleet qualification remain outstanding. See `docs/plans/agent-server-hardening-review.md` for the full review and evidence boundaries.
