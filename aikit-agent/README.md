# aikit-agent

`aikit-agent` is the in-process runtime used by `aikit run -a aikit`.
It provides:

- OpenAI-compatible LLM gateway plumbing
- context budgeting and compression flow
- built-in tools (file/bash/git/skill access)
- local skill discovery
- bounded sub-agent execution

This crate is consumed by `aikit-sdk`, which adapts runtime events into the public SDK event model.

## Minimal usage

```rust
use aikit_agent::llm::openai_compat::OpenAiCompatProvider;
use aikit_agent::{run, AgentConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workdir = std::env::current_dir()?;
    let config = AgentConfig::from_env(workdir, true, None)?;
    let gateway = OpenAiCompatProvider::new(
        config.timeout_secs,
        config.connect_timeout_secs,
    )?;

    let events = run(
        config,
        "Inspect repository structure and summarize modules.",
        Box::new(gateway),
    )?;

    for event in events {
        println!("{event:?}");
    }
    Ok(())
}
```

## Environment

Common variables used by runtime/provider setup:

- `AIKIT_LLM_URL`
- `AIKIT_MODEL`
- `AIKIT_STREAM`
- `AIKIT_MAX_ITERATIONS`
- `AIKIT_CONTEXT_BUDGET_TOKENS`
- `OPENAI_API_KEY` or `AIKIT_API_KEY`

## Test

From workspace root:

```bash
cargo test -p aikit-agent
```

## Related docs

- Contributor notes: `CONTRIBUTING.md`
- SDK integration layer: `../aikit-sdk/README.md`
- Workspace overview: `../README.md`

## License

Apache-2.0
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

## Host Tools

Embedders can inject extra LLM-callable tools by implementing `HostToolProvider`
and setting `config.host_tool_provider` before calling `run`:

```rust
use std::sync::Arc;
use aikit_agent::{HostToolDefinition, HostToolProvider};

struct MyProvider;

impl HostToolProvider for MyProvider {
    fn list_tools(&self) -> Vec<HostToolDefinition> {
        vec![HostToolDefinition {
            name: "deploy".to_string(),
            description: Some("Deploy the current build".to_string()),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "env": { "type": "string", "enum": ["staging", "production"] }
                },
                "required": ["env"]
            }),
        }]
    }

    fn call_tool(&self, name: &str, args: serde_json::Value) -> Result<String, String> {
        match name {
            "deploy" => Ok(format!("deployed to {}", args["env"].as_str().unwrap_or("?"))),
            other => Err(format!("unknown tool: {}", other)),
        }
    }
}

let mut config = AgentConfig::from_env(workdir, false, None)?;
config.host_tool_provider = Some(Arc::new(MyProvider));
```

**Sandboxing:** Host tools do NOT receive `ToolContext` and are therefore NOT
sandbox-constrained by aikit. The embedder is fully responsible for path
validation, resource limits, and any other safety constraints.

**Risky built-ins and `yolo` mode:** Built-in tools such as `run_bash` are
subject to `AgentPersona.disallowed_tools` filtering, which can be used to
restrict them in non-`yolo` flows. Host tools are subject to the same
`AgentPersona` allowlist/denylist filtering, but aikit does not enforce any
confirmation step for host tool calls. **Embedders MUST implement their own
confirmation logic** before executing destructive or side-effectful host
operations.
