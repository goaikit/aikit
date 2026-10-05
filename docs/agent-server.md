# Common agent server

`aikit serve` exposes a durable session API under `/api/v1/gateway`. The Rust wire types live in `aikit_sdk::runner::session`. One host owns its workspace, processes, credentials, session journal and stable `host_id`. Client applications may disconnect without stopping work.

## Start a host

```sh
export AIKIT_GATEWAY_DATA=/var/lib/aikit/host
export AIKIT_GATEWAY_WORKSPACES='["/srv/workspaces"]'
aikit serve --host 127.0.0.1 --port 8787 --api-key "$AIKIT_HOST_KEY"
```

Use TLS termination for remote clients. The working directory defaults to the allowed workspace root and `.aikit/host` is the default data directory. The server canonicalizes requested working directories and checks allowed roots; this is admission control, not a filesystem sandbox. Run a host in the trust boundary appropriate for its agent tools and credentials. Run a separate container/VM for each untrusted tenant.

The store holds a process lock. Two processes cannot supervise the same store. For Linux production, supervise the host and its descendants as one systemd control group or container so a host crash also stops orphan agent processes. Configure systemd `KillMode=control-group` and a shutdown deadline appropriate for the longest in-process work. Configure distinct storage and ports for independent hosts.

## Backend coverage

| Key | Transport | Notes |
| --- | --- | --- |
| `claude` | Claude SDK session bridge | Turns, model, context, interrupt, permissions and native resume |
| `codex` | Codex app-server | Turns, steering, interrupt, permissions, questions and native resume |
| `pi` | Pi stdio RPC | Turns, steering/follow-up, model/context, interrupt and resume; requires explicit `allow` policy because gateway permission routing is unsupported |
| `cursor` | ACP stdio | Default `cursor-agent acp` |
| `grok` | ACP stdio | Default `grok --permission-mode default agent stdio` |
| `gemini` | ACP stdio | Default `gemini --acp`; arguments can be overridden for older installations |
| `antigravity` | ACP stdio | Configure `AIKIT_ANTIGRAVITY_BIN` to the installed ACP harness; Linux passes `--uid=` by default. Supply the harness environment/authentication appropriate for that installation |
| `opencode` | OpenCode HTTP/SSE | Configure `AIKIT_OPENCODE_URL`; the server is operator-managed. Supports permissions and questions. Model uses `provider/model` |
| `aikit` | Existing in-process runner | Native history/resume and sequential turns. Requires explicit `allow`. Interrupting an active in-process turn is unsupported; close while active fails |

ACP launch overrides are operator environment variables `AIKIT_<BACKEND>_BIN`, `AIKIT_<BACKEND>_ARGS` (a JSON string array) and optional `AIKIT_<BACKEND>_AUTH_METHOD`. Arguments are passed directly, never through a shell. Typical native auth methods include Cursor `cursor_login`, Grok `xai.api_key`/`cached_token` and Antigravity `oauth-personal`; configure credentials on the host. ACP resume support is negotiated during initialization and appears in the session's capabilities. A requested creation model is sent through ACP's model operation and fails explicitly if unsupported.

OpenCode uses the unprefixed SDK-v2-compatible `/session`, `/event`, `/permission` and `/question` HTTP routes. `OPENCODE_SERVER_PASSWORD` and optional `OPENCODE_SERVER_USERNAME` configure Basic authentication to the operator-selected endpoint. Native event stream loss interrupts the gateway session; it never pretends that missing native events were replayed.

Discovery advertises implemented adapter capabilities, not whether binaries, credentials or model access are installed. Creation failures are recorded in the command receipt. Every client must inspect the returned session capabilities.

## Contract

| HTTP operation | Meaning |
| --- | --- |
| `GET /gateway` | Protocol version, host identity, capacity and backend capabilities |
| `GET /gateway/schema` | Generated JSON schemas for version 1 wire types |
| `GET /gateway/metrics` | Host-wide operational gauges and counters (owner or host-wide read grant) |
| `POST /gateway/sessions` | Durably accept creation and the initial prompt |
| `GET /gateway/sessions` | Persisted session metadata |
| `GET /gateway/sessions/{id}` | State, capabilities, native session ID and last event sequence |
| `POST /gateway/sessions/{id}/commands` | Submit an idempotent command |
| `GET /gateway/sessions/{id}/commands/{command_id}` | Inspect a command receipt |
| `GET /gateway/commands/{command_id}` | Inspect a creation receipt |
| `GET /gateway/sessions/{id}/events?after=N` | Replay after sequence N, then stream live events |
| `GET /gateway/sessions/{id}/requests` | Outstanding requests and deadlines |
| `POST /gateway/sessions/{id}/requests/{request_id}/response` | Answer a request with a respond-scoped token |

All paths above are relative to `/api/v1`. Creation example:

```json
{"command_id":"client-1:create-1","backend":"claude","cwd":"/srv/workspaces/project","prompt":"Review the tests","permission_policy":"ask"}
```

Optional creation fields are `model` and `resume` (the native agent session ID). After host restart, use the recorded native ID to explicitly create a resumed session when supported. The new public session has a new ID; the interrupted session and its retained events remain inspectable. Native resume does not guarantee restoration of an interrupted tool call.

Command examples:

```json
{"command_id":"client-1:turn-2","type":"send_turn","text":"Explain the failures"}
{"command_id":"client-1:interrupt-1","type":"interrupt"}
{"command_id":"client-1:answer-1","type":"respond","request_id":"...","response":{"type":"allow"}}
{"command_id":"client-1:close-1","type":"close"}
```

Other capability-gated actions are `steer`, `follow_up`, `set_model`, and `context_usage`. Question responses use `{"type":"answers","answers":...}` with the native answer schema included in the request payload. ACP permission responses select a supplied `allow_once` option ID; unknown option IDs cannot grant permission. A `deny` response or timeout denies the request. Session close and interrupt resolve pending requests without waiting for a connected viewer.

The explicit permission policies are `ask` (default), `deny`, and `allow`. They control requests surfaced by the native agent; they do not override tool actions the agent's own configuration already permits without asking. Pending interactive requests expire after 120 seconds. Approval is never inferred from a missing client.

Receipts distinguish durable `accepted`, locally `dispatched`, `failed`, and `outcome_unknown`. Dispatch is not native success or turn completion. Native errors and terminal results arrive through the event stream. Identical retries return the existing receipt; a reused command ID with a different body fails. IDs are scoped to creation or to a public session. A new command ID is a new operation.

The event envelope contains `version`, `host_id`, `session_id`, `sequence`, `turn_id`, `timestamp_ms`, `type`, and `payload`. An `agent` event contains the original AgentEvent; native extensions use a `native` event tagged with their protocol. Clients use the outer sequence to deduplicate, preserve order and reconnect. Never interpret the inner agent sequence as the replay cursor.

## Reconnect, retention and capacity

SSE event name is `session_event`, and `id` is the outer sequence. Supply `Last-Event-ID` or `after`; explicit `after` takes precedence. Replay is read from the same durable journal used for live delivery. Clients can replay duplicates safely using sequence IDs. A disconnected subscriber does not own the session or cancel its turn.

Retention is bounded to 10,000 events and 16 MiB per session. Oversized events fail closed at 1 MiB. Replay pages contain at most 128 events and 1 MiB; subscriber buffers contain two frames. A subscriber blocked for 15 seconds is disconnected. Subscriber count is capped relative to configured session capacity. A stale cursor returns 410; a stream that falls behind retention receives `replay_error`. Fetch session metadata and explicitly resynchronize from available native history or show a history gap. Session metadata is not a complete transcript snapshot.

There are 32 queued commands and at most 32 pending requests per session. The host rejects new sessions when its configured capacity or 10,000-session history ceiling is reached. SQLite limits the main database to 131,072 pages (normally 512 MiB); persistence failure stops admission. Preserve/export state before rotating the data directory. Receipts are retained to prevent accidental duplicate execution. Event truncation never authorizes replaying a command.

Restart marks formerly active sessions `interrupted`, clears stale pending requests, and marks unresolved accepted commands `outcome_unknown`. It does not automatically repeat tools or prompts. Browser origins are denied unless listed in `AIKIT_GATEWAY_ORIGINS` as a JSON array. Use `fetch` streaming with Authorization; native EventSource cannot set a bearer header.

## Scoped client tokens

The existing `--api-key` is the host owner credential. Optional `AIKIT_GATEWAY_TOKENS_FILE` points to an operator-managed JSON array:

```json
[{"token":"replace-with-at-least-32-random-characters","scopes":["read","respond"],"sessions":["public-session-id"]}]
```

Scopes are `read`, `execute`, and `respond`. Omit `sessions` for host-wide gateway access. Session-scoped tokens cannot list other sessions, create new sessions, or access legacy endpoints. `respond` authorizes the dedicated response endpoint. Tokens are loaded at startup; restart after rotation. Store the file with permissions restricted to the host service account.

A `respond` action sent through the generic commands endpoint requires both `execute` and `respond`; execute-only grants cannot approve requests through that route. The dedicated response endpoint requires only `respond`.

## Errors and retry decisions

Gateway lifecycle HTTP errors expose `error.code`, diagnostic `error.message`, and `error.retry`. Codes are stable labels; diagnostics may contain native details and must not be parsed as codes. Framework extraction/authentication failures may retain their existing HTTP error format. The example client exposes `GatewayError.status`, `.code` and `.retry` and bounds ordinary HTTP requests to 30 seconds; streaming is controlled by the caller's abort signal.

Failed command receipts additionally include `failure: {code, retry}` alongside the existing diagnostic `error`. Older persisted receipts without this optional field remain readable. Retry advice is explicit:

| Advice | Client action |
| --- | --- |
| `same_command_id` | Back off, then retry the identical operation with its original command ID |
| `new_command_after_backoff` | The command was rejected before native dispatch; after checking capacity, a new ID may be submitted |
| `inspect_receipt` | Inspect the existing receipt/session/events; do not automatically repeat possible native side effects |
| `never` | Resolve the conflict, invalid operation, or ended session; do not automatically retry |
| `backoff` | Retry the read/subscription after capacity becomes available |
| `resynchronize` | Explicitly recover history or display a gap before subscribing again |
| `operator_action` | Retention/storage intervention is required |

An HTTP timeout or lost response is not proof that an operation failed. Query its receipt and reuse the same command ID. Commands queued behind failed startup or a closed session receive a terminal `session_closed_before_dispatch` failure instead of remaining accepted forever.

## Native deadlines and monitoring

ACP pipe writes have a 30-second maximum included in the request deadline; request deadlines vary by operation (initialization 30 seconds, authentication/session setup 60 seconds, prompt completion one hour). A timed-out ACP connection is killed and reaped. Codex request deadlines include lock acquisition, pipe writing and reply waiting (60 seconds by default); timeout terminates the connection. Claude connection/control calls are bounded to 30 seconds each, and synchronous context lookup to 60 seconds. Pi pipe writes are bounded to 30 seconds and kill the child on timeout; readiness and stats also retain their existing deadlines. OpenCode HTTP requests retain their 60-second timeout and 15-second stream-start deadline. These bounds apply to individual native operations; they are not a single global model-turn deadline.

Metrics include active/opening sessions, pending requests, active subscribers, rejected command queues, persistence failures and slow-subscriber disconnections. Counters reset at process restart and have no per-session labels. They do not replace latency distributions or resource profiling.

`deploy/aikit-gateway.service` and its README provide a Linux service example with a required credentials file, process-group termination and explicit stop deadline. `.github/workflows/gateway.yml` runs the dedicated Linux fixture, SDK and actual HTTP/crash tests without model credentials. Adding the workflow does not establish that it has passed on a hosted runner.

## Validation

`cargo test -p aikit-cli --lib gateway -- --test-threads=1` runs persistence, HTTP, authorization, capacity and protocol fixture tests. ACP fixtures are standalone Rust subprocesses and require `rustc`; they make no model calls. OpenCode fixtures use a local mock HTTP server. The SDK session bridge regression tests run separately. Fixture coverage and compilation do not establish compatibility with every installed native version or credentials.

The ignored synthetic fleet test exercises 100 independent stores with 10 sessions each. It measures this host implementation on one machine; it is not evidence for an actual 100-server deployment. Use the deployment load script against real host URLs to measure HTTP latency, reconnect behavior and resource use under the intended hardware, agent versions and model workload.

The checked-in `session-contract-v1.json` is generated by `cargo run -p aikit-sdk --example session_schema --no-default-features`. `examples/gateway-client.mjs` demonstrates authenticated fetch streaming, command IDs and reconnect cursors. `scripts/gateway-load.mjs` runs an explicitly configured deployment workload.

Readiness becomes false on drain or journal failure. Discovery reports active session count and configured capacity. Shutdown allows up to ten seconds for gateway workers to close; the process supervisor remains responsible for stopping descendants after that deadline. This version has no central host directory, automatic migration, transcript snapshot compaction, or latency histogram exporter. Use explicit replay-gap handling and external service metrics when operating it.
