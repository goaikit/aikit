# Magic-tool layer is a standalone crate over aikit-sdk, not part of cli-framework

The magic-tool HTTP + registry layer (`aikit-magictool`) is its own workspace crate that depends on
`aikit-sdk` for agent execution, and is **not** folded into cli-framework's `api` host layer. cli-framework
is the generic host (bind/auth/versioning/swagger/health + a mount hook); `aikit-magictool` produces an
`axum::Router` that the host merges into its `v1` `ApiVersion`.

## Why

`aikit` already depends on `cli-framework`, so putting agent-backed tool code (which needs `aikit-sdk`) into
cli-framework would create a cycle: `aikit → cli-framework → aikit-sdk → … → aikit`. cli-framework's `api`
layer is also deliberately decoupled from domain routes — it hosts the app's router, it does not provide one.
A separate crate keeps the core + HTTP layers agent-agnostic (deps: axum, serde, jsonschema) with the
`aikit-sdk` coupling isolated to a feature-gated backend module, which also lets `newton` consume the same
crate (path→git, like `aikit-evals`).

## Consequences

- L1 (core) + L2 (HTTP) carry no `aikit-agent`/`aikit-sdk` dependency, enforced by the `agent` Cargo feature.
- A future non-aikit host can mount the router with its own `ToolExecutor`/`ToolChat` implementations.
- The crate tracks cli-framework's pinned axum version (lockstep) since the mounted router crosses that boundary.
