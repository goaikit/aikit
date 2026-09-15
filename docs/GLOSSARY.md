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

## Magic tools

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Magic tool** | A registered, problem-agnostic structured agentic call defined by a prompt + input/output JSON Schemas (+ optional `agent_key`); takes form data and returns a schema-valid **Draft**. | Tool (alone — collides with MCP tools and aikit-agent host tools), action, command |
| **Draft** | The proposed, schema-validated JSON a magic tool returns for a human to review and apply to a form; never persisted by the framework. | Result, output, answer (when you mean the reviewable proposal) |
| **One-shot invocation** | A single magic-tool call: validated input → one **Agent run** → **Draft**. The "Magic Button" mode. | Magic tool (the tool ≠ a single call) |
| **Magic tool session** | A multi-turn conversation that refines a **Draft**, implemented as successive **Agent runs** sharing one `session_id`. The "Copilot" mode. | "Session" unqualified — see flagged ambiguity below |

## Session capture and briefs

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Captured session** | One session an adapter has parsed from a tool's on-disk transcript into the event store (spec 010): the row `aikit session list` prints. | Session (bare), transcript, history entry |
| **Digest** | The bounded, secret-scrubbed text built from a captured session's events that is the only thing a summarizing model sees ([ADR 0023](adr/0023-a-session-brief-is-one-completion-over-a-scrubbed-digest.md)). | Transcript, context, prompt |
| **Area** | A directory of the repository (or a user-named group of path prefixes) a session touched, with read and modification counts and attributed time. | Module, component, file group |
| **Mechanical tag** | A tag decided in code from the events before any model call, e.g. `test` when only test files changed. | Rule tag, auto tag |
| **Session brief** | The persisted result of `aikit session summarize`: summary paragraph, areas, tags, model, digest hash, generated-at time. | Session summary (that is the captured-session row), report, annotation |

## Flagged ambiguities

- **"Session"** is overloaded. The glossary lists it as an alias to avoid for **Agent run**. The magic-tool feature uses it for the multi-turn case, so the canonical term is **Magic tool session** (a *sequence* of Agent runs sharing a `session_id`), never bare "session". A single Agent run is still an **Agent run**, not a session. A session a coding tool left on disk and an adapter parsed is a **Captured session**.
- **"Summary"** is taken twice in session capture: `SessionSummary` is the captured-session row, and the generated record is a **Session brief**. Say *brief* for the generated one.

## Relationships

- An **Agent run** uses exactly one **Runnable agent** key and is produced by the **AIKit CLI** or **aikit-sdk**, not by the **Hosted agent** runtime unless you explicitly bridge them.
- A **Magic tool** is invoked either as a **One-shot invocation** or within a **Magic tool session**; both honor the tool's `agent_key` and produce a **Draft** validated against the tool's output schema.
- A **Hosted agent** runs inside the **Base agent runtime** image; it is not the same artifact as **aikit-sdk** local spawns.
- **Streamable MCP endpoint** and **Agent HTTP API** MAY share a **Long-lived HTTP service** listener but remain different protocol contracts.
- **MCP config merge** does not create a **Long-lived HTTP service**; it points assistants at existing endpoints.

