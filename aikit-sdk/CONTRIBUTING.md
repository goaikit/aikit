# Contributing to aikit-sdk

`aikit-sdk` is the stable integration surface for catalog/path/deploy/run behavior.
Changes here impact both the `aikit` CLI and `aikit-py`.

## Source modules

| Module | Purpose |
|--------|---------|
| `agent_runner.rs` | `AgentRunner` builder + `AgentDetector` |
| `pipeline.rs` | Structured template→agent→validate→report pipeline |
| `report.rs` | `ReportRenderer` — Markdown and JSON output from validated data |
| `session_store.rs` | `SessionStore` — session persistence (`~/.aikit/sessions/`) |
| `template.rs` | `TemplateRenderer` — single-pass `{{slot}}` substitution |
| `validation.rs` | `ResponseValidator` — JSON extraction + jsonschema validation |
| `mcp_deploy.rs` | MCP server config merge helpers |
| `runner.rs` / `run_progress.rs` | Low-level agent execution and event streaming |
| `aikit_agent_adapter.rs` | Adapter for the built-in `aikit` agent |

## Test suites

- `tests/builtin_agent_test.rs` — integration tests for the built-in agent
- `tests/host_tool_provider_sdk_test.rs` — host-tool-provider SDK tests
- `tests/streaming_agents.rs` — streaming event shape tests

## Validation

Run from workspace root:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p aikit-sdk
```

If your change touches event streaming, platform-specific runner behavior, or pipeline
retry logic, also run:

```bash
cargo test -p aikit-sdk -- --ignored
```

## Guidelines

- Keep APIs deterministic and explicit.
- Avoid hidden fallback logic that changes output shape or path resolution silently.
- Preserve compatibility of event payload contracts and error semantics.
- `PipelineError` variants are part of the public API — treat additions as semver minor
  and removals as semver major.
- Update `README.md` when public behavior changes.
- Add tests for new path rules, deployment logic, and run/event behavior.

## References

- `README.md`
- `../aikit-py/README.md`
- `../CONTRIBUTING.md`
