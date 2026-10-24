//! Local staging example only. Requires an existing recorded binding. Never
//! launches an agent, invents session evidence or dispatches a native message.
use aikit_sdk::integration::{IntegrationService, MessageRequest, MESSAGE_CONTENT_LIMIT};
use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "usage: integration_messages STATE_DIRECTORY BINDING_ID OPERATION_ID < frozen-payload"
                .into(),
        );
    }
    let mut content = String::new();
    std::io::stdin()
        .take(MESSAGE_CONTENT_LIMIT as u64 + 1)
        .read_to_string(&mut content)?;
    let service = IntegrationService::open(&args[0])?;
    let binding = service.binding(&args[1])?;
    let receipt = binding.stage_message(MessageRequest {
        operation_id: args[2].clone(),
        content,
    })?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
