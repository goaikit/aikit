# AIKIT Architecture

## Overview

AIKIT is a Rust workspace centered on a CLI (`aikit`) plus reusable runtime libraries:

- `aikit` (root crate): user-facing commands (`init`, `install`, `update`, `remove`, `list`, `check`, `package`, `release`, `run`, `llm`)
- `aikit-sdk`: Rust API for agent catalog/deploy/run/event flows
- `aikit-py`: Python package and bindings around the same SDK behaviors
- `aikit-agent`: in-process runtime used by `aikit run -a aikit`

The design keeps command orchestration in the CLI crate and reusable execution logic in library crates.

## Workspace layout

```text
aikit/
  src/                 # root CLI crate internals
    cli/               # command definitions + dispatch
    core/              # package/install/business logic
    fs/                # file merge/copy/path operations
    github/            # GitHub API and release helpers
    tui/               # interactive terminal selection/output
  aikit-sdk/           # reusable Rust SDK
  aikit-py/            # Python bindings/package
  aikit-agent/         # in-process agent runtime
  tests/               # integration tests
  scripts/             # project automation scripts
```

## Runtime architecture

1. `src/main.rs` loads environment (`dotenv`) and delegates to `cli::run`.
2. `src/cli/mod.rs` parses CLI args with `clap`, initializes tracing, and dispatches commands.
3. Command handlers call modules in `src/core`, `src/fs`, `src/github`, and `src/tui`.
4. Agent execution paths (`run`, `llm`) rely on SDK/runtime crates for provider-specific behavior.

This split keeps parsing and UX concerns separated from domain logic and external integrations.

## Core modules

### `src/cli`
- Defines command/flag schemas and command dispatch.
- Wraps async handlers with a Tokio runtime where needed.

### `src/core`
- Handles package lifecycle operations and template-aware behaviors.
- Coordinates install/update/remove/list workflows and command-level business rules.

### `src/fs`
- Implements cross-platform path handling and file copy/merge operations.
- Contains JSON merge behavior used when updating existing project files.

### `src/github`
- Encapsulates GitHub API calls, auth token handling, and rate-limit aware errors.
- Supports download/release workflows used by install/publish/release commands.

### `src/tui`
- Interactive terminal UI components (selection/output) used in non-fully-specified flows.

## Key command flows

### Install
1. Resolve source (`owner/repo` or local path).
2. Fetch/read package metadata.
3. Map package templates into the selected agent layout.
4. Write/merge files and update local registry state.

### Package publish
1. Validate `aikit.toml` package metadata.
2. Build distributable assets from package templates.
3. Publish release artifacts to GitHub (via configured token and `gh` workflows where applicable).

### Run / LLM
1. Resolve agent/provider and runtime options.
2. Stream or buffer model output.
3. Optionally emit structured events for automation and tool integration.

## Design principles

- Keep crates decoupled through explicit APIs (`aikit-sdk` as the gateway surface).
- Prefer direct, deterministic logic over fallback-heavy control flow.
- Keep command handlers thin and move reusable logic to modules/crates.
- Treat docs and tests as part of the feature contract for command behavior.

## Operational constraints

- Networked commands depend on GitHub/API availability and credentials.
- Release/publish paths depend on local CLI tooling and authenticated context.
- Cross-platform behavior is handled in shared fs/path modules to avoid command duplication.

