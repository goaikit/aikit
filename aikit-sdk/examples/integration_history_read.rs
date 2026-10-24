//! Read-only disposable qualification: reuse the existing history implementation.
use aikit_sdk::{history::MessagesQuery, runner::backend::Backend};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: integration_history_read SESSION_UUID MARKED_WORKSPACE".into());
    }
    let cwd = PathBuf::from(&args[1]);
    if !cwd.join(".aikit-qualification-only").is_file() {
        return Err("disposable qualification workspace required".into());
    }
    let reader = Backend::Claude
        .history_reader()
        .ok_or("Claude history reader unavailable")?;
    let mut query = MessagesQuery::default();
    query.limit = Some(1000);
    println!(
        "{}",
        serde_json::json!({
            "info": reader.info(&args[0], Some(&cwd)).map_err(|e| e.to_string()),
            "messages": reader.messages(&args[0], &query, Some(&cwd)).map_err(|e| e.to_string()),
            "page_limit":1000,"offset":0,"read_only":true,"production_qualified":false
        })
    );
    Ok(())
}
