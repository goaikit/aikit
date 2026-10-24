//! Inspect copied history with the existing append checkpoint; never send.
use aikit_sdk::{
    history::{HistoryMessage, HistorySession},
    integration::{inspect_message_history_page, MessageAppendCheckpoint},
};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
struct Snapshot {
    info: Result<Option<HistorySession>, serde_json::Value>,
    messages: Result<Vec<HistoryMessage>, serde_json::Value>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "usage: inspect_message_history CHECKPOINT_JSON HISTORY_JSON CANONICAL_WORKSPACE"
                .into(),
        );
    }
    let checkpoint: MessageAppendCheckpoint = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let snapshot: Snapshot = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let session = snapshot
        .info
        .map_err(|_| "history metadata read failed")?
        .ok_or("session not found")?;
    let messages = snapshot
        .messages
        .map_err(|_| "history message read failed")?;
    let inspection =
        inspect_message_history_page(&checkpoint, Path::new(&args[2]), &session, &messages)?;
    println!("{}", serde_json::to_string(&inspection)?);
    Ok(())
}
