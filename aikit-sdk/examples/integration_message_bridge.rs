//! Fixed-command stdio adapter for recorded preparation and raw handoff storage.
//! No agent launch, native send, proof projection or readiness promotion occurs
//! in this process; the opt-in owned native module calls the native prompt API.
use aikit_sdk::integration::IntegrationService;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "usage: integration_message_bridge STATE_DIRECTORY INSTALLATION_ID < bounded-envelope"
                .into(),
        );
    }
    let service = IntegrationService::open(&args[0])?;
    let reply = service.handle_recorded_message_request(&args[1], std::io::stdin())?;
    println!("{}", serde_json::to_string(&reply)?);
    Ok(())
}
