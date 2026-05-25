# Glossary

Terms below reflect discussion of **AIKit**, **HTTP exposure for agent operations**, **cli-framework integration**, **Cogni+ agent hosting**, and **remote monitoring**. They are opinionated; use the bold canonical terms in specs and UI copy.

## AIKit product boundary

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **AIKit CLI** | The `aikit` executable: packages, init, checks, and one-shot agent invocations. | The SDK, "the library", aikit-sdk |
| **aikit-sdk** | The in-process Rust gateway (and Python twin) for catalog, deploy, detection, and spawning runnable agents. | aikit, "the CLI", REST API |
| **Runnable agent** | A supported external coding-agent CLI identity (e.g. codex, claude) invoked by the SDK or `aikit run`. | Agent (alone), "the model", assistant |
| **Agent run** | A single execution of a runnable agent with a prompt until exit, including captured streams and exit status. | Job, task, session (unless you define those separately) |
| **Event stream** | Machine-readable NDJSON lines describing progress and output of an agent run. | Logs (when you mean structured events), "streaming" without saying stdout vs SSE |

## Network and protocol surfaces

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Long-lived HTTP service** | A process that binds a port and handles concurrent requests until shutdown. | "Service" when you mean only a library or a one-shot CLI |
| **Agent HTTP API** | Versioned REST-style routes (e.g. under `/api/v1`) for run and catalog operations backed by aikit-sdk. | MCP, OpenAI API, "the proxy" |
| **Streamable MCP endpoint** | An HTTP path serving the Model Context Protocol (JSON-RPC / SSE) for tools, optionally merged onto the same listener as other routes. | "MCP server" when you mean only config file editing |
| **MCP config merge** | Writing MCP client entries into assistant config files; does not by itself start a listener. | "Adding MCP", deploying MCP |

## Control and observation (federated runs)

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Run coordinator** | A service that tracks run lifecycle, agents, and containers over HTTP (e.g. agentrt-style APIs). | "Monitor", orchestrator (unless defined) |
| **Human-in-the-loop channel** | A bus for asks, approvals, and notifications between people and automation (e.g. ailoop). | Chat, "notifications" (when you mean gated authorization) |

## Relationships

- An **Agent run** uses exactly one **Runnable agent** key and is produced by the **AIKit CLI** or **aikit-sdk**, not by the **Hosted agent** runtime unless you explicitly bridge them.
- A **Hosted agent** runs inside the **Base agent runtime** image; it is not the same artifact as **aikit-sdk** local spawns.
- **Streamable MCP endpoint** and **Agent HTTP API** MAY share a **Long-lived HTTP service** listener but remain different protocol contracts.
- **MCP config merge** does not create a **Long-lived HTTP service**; it points assistants at existing endpoints.

