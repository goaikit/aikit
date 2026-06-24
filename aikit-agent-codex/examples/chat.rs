//! End-to-end demo: spawn `codex app-server`, send one turn, print streamed
//! agent output, then exit on turn completion.
//!
//! Usage:
//!     cargo run --example chat -- "your prompt here"

use aikit_agent_codex::{CodexClient, ServerMessage, ServerNotificationKind, SpawnOptions};
use serde_json::json;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "List the files in this repo and describe its layout.".to_string());

    let cwd = std::env::current_dir()?;

    let opts = SpawnOptions {
        default_request_timeout: Duration::from_secs(30),
        ..Default::default()
    };
    let (client, mut events) = CodexClient::spawn_with(opts).await?;
    client
        .initialize(
            "aikit_agent_codex_example",
            "aikit-agent-codex example",
            env!("CARGO_PKG_VERSION"),
        )
        .await?;

    let thread_id = client
        .thread_start_simple(&cwd, "never", "workspace-write")
        .await?;
    eprintln!("[thread {} started in {}]", thread_id, cwd.display());

    let turn_id = client.turn_start(&thread_id, &prompt).await?;
    eprintln!("[turn {} started]\n", turn_id);

    while let Some(ev) = events.recv().await {
        match ev {
            ServerMessage::Notification(n) => match n.kind() {
                ServerNotificationKind::AgentMessageDelta => {
                    if let Some(delta) = n.params.get("delta").and_then(|d| d.as_str()) {
                        print!("{delta}");
                    }
                }
                ServerNotificationKind::TurnCompleted => {
                    println!("\n\n[turn {} completed]", turn_id);
                    break;
                }
                _ => {
                    eprintln!("[{}] {}", n.method, n.params);
                }
            },
            ServerMessage::ServerRequest(req) => {
                eprintln!(
                    "[server request {} (id={}) -> auto-approving]",
                    req.method, req.id
                );
                client
                    .reply_server_request(req.id, json!({ "outcome": "approved" }))
                    .await?;
            }
        }
    }

    client.shutdown().await?;
    Ok(())
}
