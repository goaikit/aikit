# Contributing to AIKIT

This repository is a Cargo workspace (`members = [".", "aikit-sdk", "aikit-py", "aikit-agent", "aikit-evals", "aikit-magictool"]`):

- `aikit` (root CLI, crate `aikit-cli`): package install/init/build/publish, checks, release, run, serve, spec entry points. Commands are registered with `cli-framework` (`CommandSpec`) in `src/cli/mod.rs`.
- `aikit-sdk`: reusable Rust gateway for catalog, deploy, agent run/event APIs, the structured agent pipeline, and session store
- `aikit-py`: Python bindings and package over the SDK
- `aikit-agent`: in-process agent runtime used by `aikit agent run --agent aikit`
- `aikit-evals`: evaluation harness for agent runs
- `aikit-magictool`: reusable magic-tool HTTP layer (one-shot and multi-turn form-fill); mounted on `aikit serve` when the root crate is built with `--features tools`

## Prerequisites

- Rust toolchain (`rustup`, `cargo`)
- `cargo-nextest` for the standard test flow
- `gh` CLI for release-related workflows

Install nextest:

```bash
cargo install cargo-nextest
```

## Development workflow

```bash
# 1) Format
cargo fmt --all

# 2) Lint
cargo clippy --workspace --all-targets --all-features -- -D warnings

# 3) Test
./scripts/run-tests.sh
```

For crate-specific work, scope commands with `-p`:

```bash
cargo test -p aikit-sdk
cargo test -p aikit-agent
cargo test -p aikit-magictool
```

Build or test the CLI with magic-tool routes enabled:

```bash
cargo build --features tools
cargo test -p aikit --features tools
```

## Project conventions

- Keep modules focused and avoid cross-crate coupling that bypasses public APIs.
- Prefer simple, direct implementations over defensive fallback branches.
- Update docs (`README.md`, `architecture.md`, crate READMEs) when behavior changes.
- Add tests for new command paths and integration behavior.

## Pull requests

- Keep PRs small and reviewable.
- Include a short test plan in the PR description (commands run + results).
- Use Conventional Commit messages.

## Serve subsystem

`src/cli/serve.rs` implements the HTTP server for `aikit serve`. It uses
`cli-framework`'s `ApiServerBuilder` for versioning, health checks, and auth,
with domain routes on `/api/v1`. Agent sessions use `aikit_sdk::session_store`.

With `--features tools`, the server also mounts `aikit-magictool` routes under
`/api/v1/aitools/…` (see `src/tools/` for built-in tools such as
`agents/draft_definition`). Design notes: `docs/adr/0001-magictool-standalone-crate-over-aikit-sdk.md`.

Integration tests live under `tests/serve/` (smoke, auth, timeout, limits,
disconnect). Run them with:

```bash
cargo test -p aikit -- serve
```

## Additional references

- Testing details: `TESTING.md`
- Agent runtime crate notes: `aikit-agent/CONTRIBUTING.md`
