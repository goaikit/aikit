//! Decode copied fixture responses through the SDK, with no native connection.
use aikit_sdk::integration::inspect_codex_message_snapshot;
use serde::Deserialize;
use serde_json::Value;
use std::io::Read;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    thread_id: String,
    client_message_id: String,
    frozen_content: String,
    queue: Value,
    history: Value,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(40 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 40 * 1024 * 1024 {
        return Err("inspection envelope exceeds 40 MiB".into());
    }
    let input: Input = serde_json::from_slice(&bytes)?;
    let report = inspect_codex_message_snapshot(
        &input.thread_id,
        &input.client_message_id,
        &input.frozen_content,
        &serde_json::to_vec(&input.queue)?,
        &serde_json::to_vec(&input.history)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
