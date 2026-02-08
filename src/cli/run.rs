use aikit_sdk::run_agent;
use aikit_sdk::{RunError, RunOptions};
use anyhow::Result;
use clap::Parser;
use std::io::{self, Read, Write};

#[derive(Parser, Debug)]
#[command(about = "Run a coding agent with a prompt (stdin or -p)")]
pub struct RunArgs {
    /// Agent to run (default: CODING_AGENT env var, then opencode)
    #[arg(long, short = 'a')]
    pub agent: Option<String>,

    /// Model to use (default: CODING_AGENT_MODEL env var, then zai-coding-plan/glm-4.7)
    #[arg(long, short = 'm')]
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
}

pub fn execute(args: RunArgs) -> Result<()> {
    let agent = args
        .agent
        .or_else(|| std::env::var("CODING_AGENT").ok())
        .unwrap_or_else(|| "opencode".to_string());

    let model = args
        .model
        .or_else(|| std::env::var("CODING_AGENT_MODEL").ok())
        .unwrap_or_else(|| "zai-coding-plan/glm-4.7".to_string());

    let prompt = match args.prompt {
        Some(p) => p,
        None => {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            buffer
        }
    };

    let options = RunOptions {
        model: Some(model),
        yolo: args.yolo,
        stream: args.stream,
    };

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
