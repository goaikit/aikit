# Two agent registries: deploy-layout vs runnable-backend

## Status

accepted (resolves audit ARCH-2; builds on [0008](0008-backend-identity-enum-transport-trait.md))

## Context

Agent identity/config was spread across three tables that had silently diverged
(audit `specs/ISSUES-2026-07-07.md`, ARCH-2):

- `src/core/agent.rs` `EXTRAS` — {install_url, requires_cli, arg_placeholder, folder};
  keys include `cursor-agent`, `qwen`, `windsurf`, `copilot`.
- `src/models/config.rs` `default_agents` — {name, folder, install_url, requires_cli,
  output_format, output_dir, arg_placeholder, extensions}; key `cursor`.
- `aikit-sdk/src/lib.rs` `AgentEntry` — {commands, skills, subagents, instruction_file};
  key `cursor-agent`.

The divergence was not merely stale values. The tables disagree on the **set of agents**
and their **keys** (`cursor` vs `cursor-agent`), carry **different fields**, and use the
same field name for **two different questions**. `requires_cli` is the clearest example:
`core/agent.rs` says gemini `requires_cli: true` (gemini is an external CLI we spawn to
*run* an agent — correct in the runnable sense), while `models/config.rs` says `false`
(asking a different question — whether *scaffolding* `.gemini/` files needs the CLI
present). Merging into one fatter table (the audit's original "one registry" framing)
would preserve the tangle and force us to adjudicate conflicts that only exist because two
concerns share a name.

## Decision

Split by responsibility into **two registries joined by one canonical key**:

1. **Deploy-layout registry** — for *every* agent, including deploy-only ones (copilot,
   windsurf) that are never spawned. Owns where an agent's files live: `folder`,
   command/skill/subagent directories, `instruction_file`, `arg_placeholder`,
   `output_format`, `install_url`. The SDK `AgentEntry` table is the most complete and
   becomes the single source; `core/agent.rs::EXTRAS` and
   `models/config.rs::default_agents` are deleted and their unique fields folded in.

2. **Runnable-backend registry** — the existing closed `Backend` enum (ADR 0008):
   claude, codex, gemini, opencode, cursor, aikit. This owns "can we spawn/drive this as
   an agent." `requires_cli` **disappears as a stored field** — it is *implied*: every
   external Backend requires its CLI, `aikit` does not, and deploy-only agents are not
   Backends at all.

3. **One canonical key per agent** across both registries (resolve `cursor` vs
   `cursor-agent`), so a config value, a Backend, and a deploy layout always refer to the
   same identifier.

## Consequences

- The value conflicts dissolve rather than being adjudicated: gemini is a runnable Backend
  (CLI trivially required) *and* has a deploy layout — never the same fact, so they can no
  longer disagree.
- The `cursor`/`cursor-agent` key mismatch (a real routing bug where a configured `cursor`
  missed the SDK's `cursor-agent` row) is closed by the canonical-key rule.
- Adding or changing an agent touches one deploy-layout row (and, if runnable, the Backend
  enum) — not three tables in two crates.
- Deploy-only agents (copilot/windsurf) are representable without pretending they are
  Backends; runnable-only concerns stop leaking into the deploy table.
- Lands in Phase 2 of the remediation, alongside the ARCH-1 install/fetch unification;
  the stringly per-agent `match` blocks in `aikit-sdk/src/lib.rs:88-114` become fields on
  the deploy-layout registry.
