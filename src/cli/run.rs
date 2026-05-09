use aikit_sdk::{run_agent, run_agent_events, run_builtin_agent, AgentEvent, OutputMode};
use aikit_sdk::{ProgressViewConfig, RunError, RunOptions, RunProgress};
use anyhow::Result;
use clap::Parser;
use std::io::{self, Read, Write};

use crate::core::agent_definition::{
    load_persisted_registry, parse_agent_markdown, parse_session_agents_json, AgentDefinition,
    DefinitionRecord, DefinitionSource,
};
use crate::tui::progress_render::{ProgressRenderer, ProgressRendererSink};

#[derive(Parser, Debug)]
#[command(about = "Run a coding agent with a prompt (stdin or -p)")]
pub struct RunArgs {
    /// Runnable agent key (e.g. `codex`, `claude`, `gemini`, `opencode`, `agent`, `auto`)
    #[arg(long, short = 'a', value_name = "AGENT")]
    pub agent: String,

    /// Model passed to the agent; if omitted, the agent binary applies its own default
    #[arg(long, short = 'm', value_name = "MODEL")]
    pub model: Option<String>,

    /// Prompt to run (if not provided, reads from stdin)
    #[arg(long, short = 'p')]
    pub prompt: Option<String>,

    /// Run in yolo mode (auto-confirm, skip checks)
    #[arg(long)]
    pub yolo: bool,

    /// Enable streaming output
    #[arg(long)]
    pub stream: bool,

    /// Emit standardized NDJSON event stream to stdout (one JSON object per line)
    #[arg(long)]
    pub events: bool,

    /// Display live human-readable progress on stderr (conflicts with --events)
    #[arg(long, conflicts_with = "events")]
    pub progress: bool,

    /// Dry-run mode: validate inputs but don't execute agent (for testing)
    #[arg(long, hide = true)]
    pub dry_run: bool,

    /// Session-scoped agent definitions: inline JSON or @<path>
    #[arg(long, value_name = "JSON_OR_PATH")]
    pub session_agents: Option<String>,

    /// Session persona: name of a definition to apply as main-thread defaults
    #[arg(long, value_name = "NAME")]
    pub session_persona: Option<String>,
}

/// Load and merge `--session-agents` value into the registry.
///
/// The value is either:
/// - An inline JSON string (parsed with `parse_session_agents_json`)
/// - `@<path>` — the file is read; `.md` files use `parse_agent_markdown`, others use JSON
fn load_session_agents(
    value: &str,
) -> Result<std::collections::HashMap<String, AgentDefinition>, String> {
    if let Some(path_str) = value.strip_prefix('@') {
        let path = std::path::Path::new(path_str);
        let content = std::fs::read_to_string(path).map_err(|e| {
            format!(
                "error: --session-agents: cannot read {}: {}",
                path.display(),
                e
            )
        })?;
        let is_md = path.extension().map(|e| e == "md").unwrap_or(false);
        if is_md {
            let def = parse_agent_markdown(&content)
                .map_err(|e| format!("error: --session-agents: {}", e))?;
            let key = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("agent")
                .trim_end_matches(".agent")
                .to_string();
            let mut map = std::collections::HashMap::new();
            map.insert(key, def);
            Ok(map)
        } else {
            parse_session_agents_json(&content)
                .map_err(|e| format!("error: --session-agents: {}", e))
        }
    } else {
        parse_session_agents_json(value).map_err(|e| format!("error: --session-agents: {}", e))
    }
}

pub fn execute(args: RunArgs) -> Result<()> {
    let mut agent = args.agent;
    let mut model = args.model;

    let prompt = match args.prompt {
        Some(p) => p,
        None => {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            buffer
        }
    };

    if agent == "auto" {
        let tier_str = model.as_deref().unwrap_or("");
        let tier =
            crate::core::fallback::parse_tier(tier_str).map_err(|e| anyhow::anyhow!("{}", e))?;
        let fallback_cfg =
            crate::core::fallback::load_fallback_config().map_err(|e| anyhow::anyhow!("{}", e))?;
        let pair = crate::core::fallback::resolve_auto(&tier, &fallback_cfg)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        tracing::debug!(
            resolved_agent = %pair.agent,
            resolved_model = %pair.model,
            tier = %tier.as_str(),
            "auto-agent resolved"
        );
        agent = pair.agent;
        model = Some(pair.model);
    }

    // ── Session registry build ────────────────────────────────────────────────

    // 1. Load persisted definitions from disk.
    let workdir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut registry = match load_persisted_registry(&workdir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("warning: could not load persisted agent definitions: {}", e);
            crate::core::agent_definition::SessionRegistry::new()
        }
    };

    // 2. Parse and merge --session-agents (highest priority).
    let mut session_agents_map: std::collections::HashMap<String, AgentDefinition> =
        std::collections::HashMap::new();
    if let Some(ref sa_value) = args.session_agents {
        match load_session_agents(sa_value) {
            Ok(map) => {
                for (key, def) in map {
                    session_agents_map.insert(key.clone(), def.clone());
                    registry.merge(
                        key,
                        DefinitionRecord {
                            definition: def,
                            source: DefinitionSource::Session,
                            path: None,
                        },
                    );
                }
            }
            Err(msg) => {
                eprintln!("{}", msg);
                std::process::exit(1);
            }
        }
    }

    // 3. Resolve --session-persona from the merged registry.
    let mut session_persona_json: Option<serde_json::Value> = None;
    if let Some(ref persona_name) = args.session_persona {
        match registry.resolve_by_name(persona_name) {
            Some(record) => match serde_json::to_value(&record.definition) {
                Ok(v) => session_persona_json = Some(v),
                Err(e) => {
                    eprintln!(
                        "error: --session-persona: failed to serialize definition: {}",
                        e
                    );
                    std::process::exit(1);
                }
            },
            None => {
                eprintln!(
                    "error: --session-persona: definition '{}' not found in registry",
                    persona_name
                );
                std::process::exit(1);
            }
        }
    }

    // Serialize session_agents for RunOptions.
    let session_agents_json: std::collections::HashMap<String, serde_json::Value> =
        session_agents_map
            .iter()
            .filter_map(|(k, def)| serde_json::to_value(def).ok().map(|v| (k.clone(), v)))
            .collect();

    tracing::debug!(
        agent = %agent,
        model = ?model,
        prompt_chars = prompt.len(),
        yolo = args.yolo,
        stream = args.stream,
        events = args.events,
        progress = args.progress,
        "aikit run dispatch"
    );

    // Dry-run mode: validate inputs but don't execute
    if args.dry_run {
        println!("Dry-run mode enabled");
        println!("Agent: {}", &agent);
        println!(
            "Model: {}",
            model.as_deref().unwrap_or("(not set; agent default)")
        );
        println!("Prompt length: {} chars", prompt.len());
        println!("Yolo mode: {}", args.yolo);
        println!("Stream mode: {}", args.stream);
        println!("Events mode: {}", args.events);
        println!("Progress mode: {}", args.progress);
        if args.session_persona.is_some() {
            println!(
                "Session persona: {}",
                args.session_persona.as_deref().unwrap_or("")
            );
        }
        println!("Configuration validated successfully (dry-run)");
        return Ok(());
    }

    let mut options = RunOptions::new()
        .with_yolo(args.yolo)
        .with_stream(args.stream || args.progress);
    if let Some(ref m) = model {
        options = options.with_model(m.clone());
    }
    if let Some(persona) = session_persona_json {
        options = options.with_session_persona(persona);
    }
    if !session_agents_json.is_empty() {
        options = options.with_session_agents(session_agents_json);
    }

    let is_builtin = agent == "aikit" || agent == "agent";

    if is_builtin {
        let mode = if args.events {
            OutputMode::Events
        } else if args.progress {
            OutputMode::Progress
        } else {
            OutputMode::Plain
        };

        let progress_sink: Option<Box<dyn aikit_sdk::ProgressSink>> = if args.progress {
            let renderer = ProgressRenderer::new().unwrap_or_else(|_| ProgressRenderer::non_tty());
            Some(Box::new(ProgressRendererSink::new(renderer)))
        } else {
            None
        };

        match run_builtin_agent(
            "aikit",
            &prompt,
            options,
            mode,
            &mut io::stdout(),
            &mut io::stderr(),
            progress_sink,
        ) {
            Ok(result) => {
                let exit_code = result.exit_code().unwrap_or(1);
                std::process::exit(exit_code);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    } else if args.events {
        match run_agent_events(&agent, &prompt, options, |event: AgentEvent| {
            if let Ok(line) = serde_json::to_string(&event) {
                println!("{}", line);
            }
        }) {
            Ok(result) => {
                let _ = io::stderr().write_all(&result.stderr);
                let exit_code = result.exit_code().unwrap_or(1);
                std::process::exit(exit_code);
            }
            Err(RunError::AgentNotRunnable(key)) => {
                eprintln!("{}", RunError::AgentNotRunnable(key));
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    } else if args.progress {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let mut renderer = ProgressRenderer::new().unwrap_or_else(|_| ProgressRenderer::non_tty());
        let agent_key = agent.clone();
        match run_agent_events(&agent, &prompt, options, |event: AgentEvent| {
            progress.push(&agent_key, &event);
            let _ = renderer.render(&progress);
        }) {
            Ok(result) => {
                let exit_code = result.exit_code().unwrap_or(1);
                let _ = renderer.finalize(exit_code, progress.token_footer());
                std::process::exit(exit_code);
            }
            Err(RunError::AgentNotRunnable(key)) => {
                eprintln!("{}", RunError::AgentNotRunnable(key));
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        match run_agent(&agent, &prompt, options) {
            Ok(result) => {
                io::stdout().write_all(&result.stdout)?;
                io::stderr().write_all(&result.stderr)?;
                let exit_code = result.status.code().unwrap_or(1);
                std::process::exit(exit_code);
            }
            Err(RunError::AgentNotRunnable(key)) => {
                eprintln!("{}", RunError::AgentNotRunnable(key));
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }
}
