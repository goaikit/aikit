use aikit_sdk::{run_agent, run_agent_events, AgentEvent};
use aikit_sdk::{ProgressViewConfig, RunError, RunOptions, RunProgress};
use anyhow::Result;
use clap::Parser;
use std::io::{self, Read, Write};

use crate::tui::progress_render::ProgressRenderer;

#[derive(Parser, Debug)]
#[command(about = "Run a coding agent with a prompt (stdin or -p)")]
pub struct RunArgs {
    /// Runnable agent key (e.g. `codex`, `claude`, `gemini`, `opencode`, `agent`)
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
}

pub fn execute(args: RunArgs) -> Result<()> {
    let agent = args.agent;
    let model = args.model;

    let prompt = match args.prompt {
        Some(p) => p,
        None => {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            buffer
        }
    };

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
        println!("Configuration validated successfully (dry-run)");
        return Ok(());
    }

    let mut options = RunOptions::new()
        .with_yolo(args.yolo)
        .with_stream(args.stream || args.progress);
    if let Some(ref m) = model {
        options = options.with_model(m.clone());
    }

    if args.events {
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
