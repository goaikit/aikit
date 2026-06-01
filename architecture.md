# AIKIT Architecture

## Overview

AIKIT is a Rust workspace centered on a CLI (`aikit`) plus reusable runtime libraries:

- `aikit` (root crate): user-facing commands (`init`, `install`, `update`, `remove`, `list`, `check`, `package`, `release`, `agent run`, `serve`)
- `aikit-sdk`: Rust API for agent catalog/deploy/run/event flows
- `aikit-py`: Python package and bindings around the same SDK behaviors
- `aikit-agent`: in-process runtime used by `aikit agent run --agent aikit`
- `aikit-magictool`: magic-tool registry and HTTP router (one-shot + multi-turn drafts); optional on `aikit serve` via Cargo feature `tools`
- `aikit-evals`: evaluation infrastructure; runs eval suites against live agents and scores trajectories
- `aikit-textgrad`: artifact-agnostic text-gradient optimization; two-layer architecture — edit substrate (Layer 1, deterministic) and async optimization loop (Layer 2) driven by `aikit-evals` and `aikit-sdk`

The design keeps command orchestration in the CLI crate and reusable execution logic in library crates.

## Workspace layout

```text
aikit/
  src/                 # root CLI crate internals
    cli/               # command definitions + dispatch
      serve.rs         # HTTP server for aikit serve (axum, SSE + JSON)
    core/              # package/install/business logic
    fs/                # file merge/copy/path operations
    github/            # GitHub API and release helpers
    tui/               # interactive terminal selection/output
  aikit-sdk/           # reusable Rust SDK
    src/
      agent_runner.rs  # AgentRunner builder + AgentDetector
      pipeline.rs      # structured template→agent→validate→report pipeline
      report.rs        # Markdown and JSON report rendering
      session_store.rs # session persistence (~/. aikit/sessions/)
      template.rs      # single-pass {{slot}} template renderer
      validation.rs    # JSON extraction + jsonschema validation
  aikit-py/            # Python bindings/package
  aikit-agent/         # in-process agent runtime
  aikit-magictool/     # magic-tool core + HTTP (feature-gated agent backend)
    src/
      core/            # ToolDef, registry, validation, executor traits
      http/            # axum routes under /aitools/…
      backend/         # aikit-sdk pipeline (feature `agent`)
  src/tools/           # domain tools registered on serve (e.g. draft_agent)
  tests/               # integration tests
    serve/             # serve subsystem integration tests
  scripts/             # project automation scripts
```

## Runtime architecture

1. `src/main.rs` loads environment (`dotenv`) and delegates to `cli::run`.
2. `src/cli/mod.rs` registers commands with `cli-framework` (`CommandSpec`), initializes tracing, and dispatches commands.
3. Command handlers call modules in `src/core`, `src/fs`, `src/github`, and `src/tui`.
4. Agent execution paths (`run`) rely on SDK/runtime crates for provider-specific behavior.
5. `src/cli/serve.rs` builds an `axum` router and hosts it with `cli-framework`'s
   `ApiServerBuilder` (`/healthz`, `/readyz`, versioned `/api/v1`). Agent messages
   dispatch to `aikit-sdk` run/event APIs; `aikit_sdk::session_store` persists
   session state to `~/.aikit/sessions/`.
6. When built with `--features tools`, the v1 router also merges `aikit-magictool`
   (`/api/v1/aitools/…`) with tools from `src/tools/`.

This split keeps parsing and UX concerns separated from domain logic and external integrations.

## Core modules

### `src/cli`
- Defines command/flag schemas and command dispatch.
- Wraps async handlers with a Tokio runtime where needed.
- `serve.rs`: HTTP server hosted by cli-framework; SSE and JSON on `/messages`, API-key
  auth, session management, optional magic-tool routes (`tools` feature).

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

### Run
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

