//! Thin embedding example: no agent launch, settings installation or credential.
use aikit_sdk::integration::{render_agent_tool_plugin, AgentToolCall, AgentToolSpec};
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [action] if action == "inspect" => {
            let call = AgentToolCall::read(std::io::stdin().lock())?;
            serde_json::to_writer(std::io::stdout().lock(), &call)?;
        }
        [action, agent_key, destination] if action == "export" => {
            let spec = AgentToolSpec::read(std::io::stdin().lock())?;
            let plugin = render_agent_tool_plugin(agent_key, &spec)?;
            plugin.export_to_new_directory(Path::new(destination))?;
            println!(
                "{}",
                serde_json::json!({"plugin_name":plugin.name(),"tool_name":plugin.native_tool_name()})
            );
        }
        _ => {
            return Err(
                "usage: agent_tool_plugin export AGENT NEW_DIRECTORY | inspect; JSON stdin".into(),
            )
        }
    }
    Ok(())
}
