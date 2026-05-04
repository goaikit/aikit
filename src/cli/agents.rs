use anyhow::Result;
use clap::Parser;

use crate::core::agent_definition::{
    load_persisted_registry, DefinitionRecord, DelegationAllowlist,
};

#[derive(Parser, Debug)]
#[command(about = "List persisted agent definitions")]
pub struct AgentsArgs {
    /// Emit JSON array to stdout instead of a human-readable table
    #[arg(long)]
    pub json: bool,
}

pub fn execute(args: AgentsArgs) -> Result<()> {
    let workdir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let registry = load_persisted_registry(&workdir)?;
    let records = registry.all_sorted();

    if args.json {
        print_json(&records);
    } else {
        print_table(&records);
    }

    Ok(())
}

fn tools_display(record: &DefinitionRecord) -> String {
    match &record.definition.tools {
        Some(list) if !list.is_empty() => list.join(", "),
        Some(_) => "(none)".to_string(),
        None => "(all)".to_string(),
    }
}

fn model_display(record: &DefinitionRecord) -> &str {
    record.definition.model.as_deref().unwrap_or("(default)")
}

fn print_table(records: &[&DefinitionRecord]) {
    if records.is_empty() {
        // Print header even when empty (spec: "empty table")
        println!(
            "{:<20} {:<10} {:<15} {:<25} DESCRIPTION",
            "NAME", "SOURCE", "MODEL", "TOOLS"
        );
        return;
    }

    // Compute column widths
    let name_w = records
        .iter()
        .map(|r| r.definition.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let source_w = records
        .iter()
        .map(|r| r.source.to_string().len())
        .max()
        .unwrap_or(6)
        .max(6);
    let model_w = records
        .iter()
        .map(|r| model_display(r).len())
        .max()
        .unwrap_or(5)
        .max(5);
    let tools_w = records
        .iter()
        .map(|r| tools_display(r).len())
        .max()
        .unwrap_or(5)
        .max(5);

    println!(
        "{:<name_w$} {:<source_w$} {:<model_w$} {:<tools_w$} DESCRIPTION",
        "NAME",
        "SOURCE",
        "MODEL",
        "TOOLS",
        name_w = name_w,
        source_w = source_w,
        model_w = model_w,
        tools_w = tools_w,
    );

    for record in records {
        println!(
            "{:<name_w$} {:<source_w$} {:<model_w$} {:<tools_w$} {}",
            record.definition.name,
            record.source,
            model_display(record),
            tools_display(record),
            record.definition.description,
            name_w = name_w,
            source_w = source_w,
            model_w = model_w,
            tools_w = tools_w,
        );
    }
}

fn delegation_to_json(d: &Option<DelegationAllowlist>) -> serde_json::Value {
    match d {
        None => serde_json::Value::Null,
        Some(DelegationAllowlist::All) => serde_json::Value::String("*".to_string()),
        Some(DelegationAllowlist::None) => serde_json::Value::Array(vec![]),
        Some(DelegationAllowlist::Names(v)) => {
            serde_json::Value::Array(v.iter().map(|s| serde_json::json!(s)).collect())
        }
    }
}

fn print_json(records: &[&DefinitionRecord]) {
    let arr: Vec<serde_json::Value> = records
        .iter()
        .map(|record| {
            serde_json::json!({
                "name": record.definition.name,
                "description": record.definition.description,
                "source": record.source.to_string(),
                "model": record.definition.model,
                "tools": record.definition.tools,
                "disallowedTools": record.definition.disallowed_tools,
                "agents": delegation_to_json(&record.definition.delegation),
                "path": record.path.as_ref().map(|p| p.display().to_string()),
            })
        })
        .collect();

    match serde_json::to_string_pretty(&arr) {
        Ok(s) => println!("{}", s),
        Err(e) => eprintln!("error serializing JSON: {}", e),
    }
}
