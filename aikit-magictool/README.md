# aikit-magictool

Reusable **magic-tool** layer for schema-driven, reviewable AI form-fill:
one-shot invocations (Magic Button) and multi-turn refinement (Copilot).

A magic tool is defined by a system prompt plus input and output JSON Schemas.
The framework validates input, runs an agent, and returns a **Draft** (proposed
JSON for a human to review). It does not persist drafts.

## Install

From this workspace:

```toml
[dependencies]
aikit-magictool = { path = "../aikit-magictool", features = ["agent"] }
```

Core + HTTP layers work without `agent`; the `agent` feature wires
`aikit-sdk` for real LLM execution.

## Quick start (library)

```rust
use aikit_magictool::{router, state_with_registry, ToolDef, ToolRegistry};
use serde_json::json;

let mut reg = ToolRegistry::new();
reg.register(ToolDef::new(
    "demo",
    "echo",
    "Echo fields",
    "Return the input unchanged.",
    json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}),
    json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}),
));

let app = router(state_with_registry(reg));
// mount `app` on your axum server (e.g. under /api/v1)
```

Use `MockExecutor` / `MockChat` in tests without the `agent` feature.

## HTTP routes

When mounted (paths are relative to the parent router):

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/aitools` | List tools |
| `GET` | `/aitools/{ns}/{tool}/schema` | Schemas and supported modes |
| `POST` | `/aitools/{ns}/{tool}` | One-shot invoke |
| `POST` | `/aitools/{ns}/{tool}/sessions` | Start multi-turn session |
| `POST` | `/aitools/{ns}/{tool}/sessions/{id}/messages` | Chat turn (`Accept`: SSE or JSON) |
| `POST` | `/aitools/{ns}/{tool}/sessions/{id}/finalize` | Final draft |

On `aikit serve` (root crate built with `--features tools`), these appear under
`/api/v1/aitools/…`. The CLI registers `agents/draft_definition` in `src/tools/`.

## Design

- Standalone crate to avoid a dependency cycle with `cli-framework` (see
  `docs/adr/0001-magictool-standalone-crate-over-aikit-sdk.md`).
- No function-calling for output capture; drafts come from structured agent
  output (see `docs/adr/0002-no-function-calling-output-capture.md`).
- Glossary terms: `docs/GLOSSARY.md` (Magic tool, Draft, One-shot invocation).

## Tests

```bash
cargo test -p aikit-magictool
```

E2E tests under `tests/` require `OPENAI_API_KEY` and `--include-ignored`.
