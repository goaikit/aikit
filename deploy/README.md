# Linux gateway service

Install the built `aikit` binary at `/usr/local/bin/aikit`, create the `aikit` service user/group, and give that user access to `/srv/workspaces`. Install native agent binaries and configure credentials for this service account. Use absolute `AIKIT_<BACKEND>_BIN` overrides where appropriate; interactive shell profiles are not loaded by systemd.

Create `/etc/aikit/gateway.env`, readable only by root, containing at least:

```ini
AIKIT_SERVE_API_KEY=replace-with-a-generated-secret
```

Optional settings include `AIKIT_GATEWAY_TOKENS_FILE`, `AIKIT_GATEWAY_ORIGINS`, and native agent launch/authentication configuration. Do not use the example placeholder as a deployed key. The required EnvironmentFile makes a missing file a service-start failure.

Install `aikit-gateway.service` in `/etc/systemd/system/`, review the workspace and executable paths, then run `systemctl daemon-reload` and `systemctl enable --now aikit-gateway`. Put authenticated TLS termination in front of its loopback listener. Each independent host needs its own journal; never share one journal between processes.

`KillMode=control-group` stops descendants on service termination. The host attempts a ten-second drain; systemd enforces a twenty-second stop deadline. Interrupted work is not automatically replayed. The service restarts the host, not an interrupted agent session. This unit is a deployment example, not evidence of a tested production deployment.

Read authenticated `GET /api/v1/gateway/metrics` for session/request/subscriber gauges and queue-rejection, persistence-failure and slow-subscriber counters. Counters reset with the host process. An execute-only or session-scoped token cannot read host-wide metrics; use an owner or host-wide read grant.

## Local conformance

```sh
cargo test -p aikit-cli --lib gateway -- --test-threads=1
cargo build -p aikit-cli --bin aikit
AIKIT_HTTP_TEST_BIN=target/debug/aikit node --test examples/gateway-client.test.mjs examples/gateway-server.test.mjs
```

The HTTP suite compiles a controlled ACP peer with `rustc`, uses temporary storage, launches actual host processes, and kills/restarts them. It makes no model calls. The dedicated gateway workflow runs these checks on Linux for pull requests.
