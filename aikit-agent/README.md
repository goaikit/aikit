# aikit-agent

`aikit-agent` is the in-process agent runtime used by `aikit run -a aikit`.
It provides an OpenAI-compatible LLM gateway, context budgeting, built-in tools,
skills discovery, and bounded sub-agent execution without requiring an external
agent CLI binary.

## Programmatic usage

```rust
use aikit_agent::llm::openai_compat::OpenAiCompatProvider;
use aikit_agent::{run, AgentConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workdir = std::env::current_dir()?;
    let config = AgentConfig::from_env(
        workdir,
        true,
        Some("gpt-4o".to_string()),
    )?;
    let gateway = OpenAiCompatProvider::new(
        config.timeout_secs,
        config.connect_timeout_secs,
    )?;

    let events = run(
        config,
        "Inspect this repository and summarize the test strategy.",
        Box::new(gateway),
    )?;

    for event in events {
        println!("{event:?}");
    }

    Ok(())
}
```

Configuration is resolved from environment variables such as `AIKIT_LLM_URL`,
`AIKIT_MODEL`, `AIKIT_STREAM`, `AIKIT_MAX_ITERATIONS`,
`AIKIT_CONTEXT_BUDGET_TOKENS`, `OPENAI_API_KEY`, and `AIKIT_API_KEY`.

For deterministic tests or ephemeral task-specific agents, pass a custom
implementation of `LlmGateway` instead of `OpenAiCompatProvider`.
