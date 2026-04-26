# Contributing to AIKIT

This repository is a Rust workspace with multiple crates:

- `aikit` (root CLI): package install/init/build/publish, checks, release, run/llm entry points
- `aikit-sdk`: reusable Rust gateway for catalog, deploy, and agent run/event APIs
- `aikit-py`: Python bindings and package
- `aikit-agent`: in-process agent runtime used by `aikit run -a aikit`

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

## Additional references

- Testing details: `TESTING.md`
- Agent runtime crate notes: `aikit-agent/CONTRIBUTING.md`
