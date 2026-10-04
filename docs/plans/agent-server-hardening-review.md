# AIKit gateway: additional scope and state review

Date: 2026-10-05


Branch: `feat/common-agent-server`. Prepared for draft PR review. This review supplements the September 30 implementation report; it does not claim a deployed service or a passing remote CI run.

## Additional scope delivered

| Feature | Implementation | Evidence / boundary |
| --- | --- | --- |
| Actual HTTP/client integration | Starts the real binary and a native ACP fixture subprocess; uses the supplied Node client | Tests auth, CORS, commands, approvals, disconnect/replay, metrics and structured errors without model calls |
| Crash/restart qualification | Kills and restarts actual host processes sharing a temporary journal | Tests crashes after dispatch and during startup; native audit log proves restart/idempotent retries do not dispatch again |
| Permission races | Deadline checks and request registration/cancellation serialized under the pending-request lock | Tests timeout, expired/duplicate response, close without a viewer and competing cancel/response |
| Overload/slow subscribers | Existing bounds now have targeted saturation tests; stalled replay-error delivery also has a deadline | Tests full command queue, session admission, subscriber limit, unaffected second-session creation and permit recovery |
| Native-operation deadlines | ACP writes/replies, Codex writes/replies, Claude setup/controls/context and Pi writes now have explicit bounds | Real fixture tests prove ACP/Codex stalled writes/replies terminate within the deadline; production cleanup for every native agent is not claimed |
| Structured failures | Stable lifecycle HTTP codes/retry advice and optional command-receipt failure metadata; client exposes `GatewayError` | Compatibility with receipts lacking the optional field is retained; generic framework auth/extraction errors retain their existing format |
| Minimal metrics | Authenticated host-wide gauges and process-lifetime counters | Session/request/subscriber counts; command rejections, persistence failures, slow-client disconnects |
| Linux CI | Dedicated workflow builds and runs gateway, SDK, actual HTTP/crash and generated-schema checks | Workflow added; remote execution still pending |
| Deployment example | systemd unit and operator README | Required credentials file, loopback bind, process-group termination, explicit stop deadline; not installed or deployed |

## Review findings and corrections

1. **Approval scope bypass:** execute-only grants could approve via the generic command route. Generic response commands now require response authority too. The actual HTTP test verifies rejection of an execute-only approval.
2. **Timeout excluded blocked writes:** ACP/Codex could block while writing before reply timing began. Deadlines now cover writes and replies; timeout terminates the connection. Tests exercise both a non-reading peer and a peer that reads but never replies.
3. **Late permission responses:** a response could be accepted after its advertised deadline but before waiter cleanup. Deadline checking and removal are now serialized; expired responses cannot grant permission.
4. **Cancellation during registration:** closing/draining could race insertion of a pending request. Registration rechecks both states under the request lock and persists registration before exposing it.
5. **Unbounded error delivery:** the SSE error path could wait indefinitely on a stalled subscriber. It now has the same delivery deadline as normal frames.
6. **Non-atomic creation records:** session metadata and creation acceptance were separate database writes. They now commit in one transaction.
7. **Stranded accepted commands:** startup failure or worker termination could leave queued receipts accepted forever. Cleanup now settles queued work as `session_closed_before_dispatch` and releases admission capacity.
8. **Terminal-state regression:** late cleanup could erase failure/interruption, or a failed send could make a closed session appear idle. Terminal states are preserved in both metadata and emitted state events.
9. **Turn admission race:** the idle check and transition to running were separate. The store now checks and reserves the turn under one lock.
10. **Drain/close interaction:** admission protection could reject an explicit close during drain. Close remains allowed, and its receipt is tested through settlement.
11. **Persistence-failure dispatch:** the worker now rechecks readiness before native dispatch after journaling input. Admission/receipt write failures also mark the host unready and increment the failure counter.

Review covered authorization paths, pending-request ownership, queued-command settlement, durable creation, turn admission, late native events, pipe deadlines, subscriber cleanup and shutdown interaction. It was performed against the modified source and the focused tests; it is not a claim of exhaustive production failure testing.

## Validation record

| Final check | Result |
| --- | --- |
| Gateway suite | **29 passed**, 2 opt-in tests ignored; 4.21 seconds |
| SDK runner suite with Claude/Codex features | **243 passed**, 5 opt-in tests ignored; 12.13 seconds |
| Node client and actual server/crash suite | **4 passed**, none skipped; 3.44 seconds |
| CLI binary build | Passed |
| Workspace all-targets/all-features Clippy with warnings denied | Passed |
| Generated schemas with SDK default features disabled | Passed; includes optional `CommandFailure` metadata |
| Formatting with native checkout newline preservation | Passed (`cargo fmt --all -- --check --config newline_style=Auto`) |
| `git diff --check` | Passed |

Logs are retained in the task's `work` directory as `enhance-tests.log`, `enhance-sdk.log`, `enhance-http.log`, `enhance-build.log`, `enhance-clippy.log`, `enhance-schema.log` and `enhance-fmt-check.log`. Final tests ran against the reviewed source. The two ignored gateway tests are the prior live-native smoke and synthetic fleet harness; neither was rerun in this pass.

**Assessment:** the requested additional implementation scope is complete and the focused local checks pass. The branch is ready for PR preparation and Linux CI review, with the remaining operational qualifications below explicitly outstanding. Deployment and production qualification remain outstanding.

## Remaining gaps

- The new Linux workflow has not executed remotely. The systemd example has not been installed or exercised on Linux.
- This pass uses controlled native fixtures. It does not promote the seven previously unqualified backends to live-validated status or replace production approval/resume/cancellation tests.
- Previous full-workspace Windows/WSL failures and stalled archive-install tests were not resolved by this focused pass. A completely green workspace gate remains outstanding.
- No actual hundreds-of-host benchmark, long-duration soak, OS-level disk-full/power-loss test or resource-sizing study was performed.
- Metrics are basic counters/gauges; full latency distributions and a fleet monitoring integration remain future work.
- Central orchestration, cross-host migration, transcript snapshot compaction and native mobile apps remain outside this scope.

## Files added or extended in this pass

- Gateway: `auth.rs`, `errors.rs`, `mod.rs`, `store.rs`, `rpc.rs`, `hardening_tests.rs`.
- Native bridges: `aikit-agent-codex/src/client.rs`; SDK Claude, Codex and Pi session modules.
- Contract: SDK session receipt failure metadata and regenerated `docs/session-contract-v1.json`.
- Client/fixtures: `examples/gateway-client.mjs`, `examples/gateway-server.test.mjs`, `tests/fixtures/acp_peer.rs`.
- Operations: `.github/workflows/gateway.yml`, `deploy/aikit-gateway.service`, `deploy/README.md`, operator guide and plan updates.
