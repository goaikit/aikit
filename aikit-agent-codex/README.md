# aikit-agent-codex

Async Rust client for the OpenAI Codex `app-server` JSON-RPC protocol.

Spawn `codex app-server`, complete the `initialize`/`initialized` handshake, open
threads, send turns, stream agent output, and answer approval prompts — all from
a Rust process. The wire format is newline-delimited JSON-RPC 2.0 over stdio
(the `"jsonrpc":"2.0"` header is omitted on the wire, per the app-server spec).

## Install

Add the crate to your `Cargo.toml`:

```toml
[dependencies]
aikit-agent-codex = { path = "../aikit-agent-codex" }
tokio = { version = "1", features = ["full"] }
serde_json = "1"
```

You also need the `codex` binary on `PATH` (it ships with the Codex CLI).

## Quick start

```rust
use aikit_agent_codex::{CodexClient, ServerMessage, ServerNotificationKind};
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;

    let (client, mut events) = CodexClient::spawn().await?;
    client.initialize("my_app", "My App", "0.1.0").await?;

    let thread_id = client
        .thread_start_simple(&cwd, /*approvalPolicy*/ "never", /*sandbox*/ "workspace-write")
        .await?;

    let turn_id = client.turn_start(&thread_id, "Summarize this repo.").await?;

    while let Some(ev) = events.recv().await {
        match ev {
            ServerMessage::Notification(n) => match n.kind() {
                ServerNotificationKind::AgentMessageDelta => {
                    if let Some(delta) = n.params.get("delta").and_then(|d| d.as_str()) {
                        print!("{delta}");
                    }
                }
                ServerNotificationKind::TurnCompleted => break,
                _ => {}
            },
            ServerMessage::ServerRequest(req) => {
                // Auto-approve any tool call.
                client.reply_server_request(req.id, json!({ "outcome": "approved" })).await?;
            }
        }
    }

    client.shutdown().await
}
```

Run the bundled end-to-end demo:

```bash
cargo run --example chat -- "Explain the project layout"
```

## Multi-turn replies

Keep the same `thread_id` and call `turn_start` again for each user reply after
the previous turn completes. `turn_steer` is for adding input to an in-flight
turn, not for a normal follow-up message.

```rust
use aikit_agent_codex::{CodexClient, ServerMessage, ServerNotificationKind};
use serde_json::json;
use std::io::{self, Write};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;

    let (client, mut events) = CodexClient::spawn().await?;
    client.initialize("my_app", "My App", "0.1.0").await?;

    let thread_id = client
        .thread_start_simple(&cwd, "never", "workspace-write")
        .await?;

    loop {
        print!("you> ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();
        if input.is_empty() || input == "exit" || input == "quit" {
            break;
        }

        let turn_id = client.turn_start(&thread_id, input).await?;

        while let Some(ev) = events.recv().await {
            match ev {
                ServerMessage::Notification(n) => match n.kind() {
                    ServerNotificationKind::AgentMessageDelta => {
                        if let Some(delta) = n.params.get("delta").and_then(|d| d.as_str()) {
                            print!("{delta}");
                        }
                    }
                    ServerNotificationKind::TurnCompleted => {
                        println!("\n[turn {turn_id} completed]");
                        break;
                    }
                    _ => {}
                },
                ServerMessage::ServerRequest(req) => {
                    client.reply_server_request(req.id, json!({ "outcome": "approved" })).await?;
                }
            }
        }
    }

    client.shutdown().await
}
```

## Steering an active turn

Use `turn_steer` only while a turn is still running. It appends steering input
to the active turn and returns the `TurnId` that accepted the input. For a
normal reply after `TurnCompleted`, call `turn_start` again instead.

```rust
let turn_id = client.turn_start(&thread_id, "Draft a migration plan.").await?;

let same_turn_id = client
    .turn_steer(&thread_id, "Focus on rollback steps and operational risk.")
    .await?;

assert_eq!(same_turn_id, turn_id);
```

## What's implemented

| Method | Helper |
|---|---|
| `initialize` + `initialized` | `CodexClient::initialize` |
| `thread/start` | `thread_start`, `thread_start_simple` |
| `thread/resume` | `thread_resume` |
| `turn/start` | `turn_start` |
| `turn/steer` | `turn_steer` |
| `turn/interrupt` | `turn_interrupt` |
| any method | `request`, `request_with_timeout`, `notify` |
| server-side approval requests | `reply_server_request`, `reply_server_request_error` |

For methods without a typed helper (e.g. `thread/fork`, `thread/compact/start`,
`fs/readFile`, `model/list`), call `client.request("method", params_json).await`.

## Transports

Stdio is the only transport wired in. To connect to a running
`codex app-server --listen unix://` or `ws://` endpoint, spawn that server
separately and adapt the reader/writer halves; the JSON-RPC layer above is
transport-agnostic.

## References

- Protocol: <https://developers.openai.com/codex/cli>
- App-server README: `tmp/codex/codex-rs/app-server/README.md`
- Generate a typed schema with `codex app-server generate-json-schema --out schema/`

## License

Apache-2.0
