# Contributing

Keep `aikit-agent` independent from `aikit-sdk` and the CLI crate. The SDK owns
conversion into public `AgentEvent` values; this crate owns agent logic,
provider abstraction, tools, context management, skills, and sub-agents.

Before opening changes, run:

```bash
cargo test -p aikit-agent
cargo test -p aikit-sdk
```

Preserve these compatibility rules:

- Existing external agent runners must continue to work through `aikit-sdk`.
- `aikit llm` behavior and `E_LLM_*` errors should not change as part of
  `aikit-agent` work.
- New built-in agent runtime errors should use the `E_AIKIT_*` prefix.
- Skills discovery should load metadata at startup and full `SKILL.md` content
  only through `read_skill`.
