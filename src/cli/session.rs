//! `aikit session` — interactive bidirectional agent sessions.
//!
//! Subcommands:
//! - `new`  — open a live session and enter a multi-turn REPL
//! - `list` — list active live sessions (requires `AIKIT_SERVE_URL`)

use std::io::{self, BufRead, Write as IoWrite};

use aikit_sdk::{
    open_claude_session, open_codex_session, AgentEvent, AgentEventPayload, ClaudeSessionOptions,
    CodexSessionOptions,
};

// ── public args ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct NewSessionArgs {
    pub agent: String,
    pub prompt: String,
    pub model: Option<String>,
    /// Codex only: approval policy (e.g. `never`, `on-request`).
    pub approval_policy: Option<String>,
    /// Codex only: sandbox mode.
    pub sandbox: Option<String>,
    /// Print events as NDJSON instead of human-readable text.
    pub events: bool,
}

#[derive(Debug)]
pub struct ListSessionsArgs {
    /// Base URL of a running `aikit serve` instance (default: `AIKIT_SERVE_URL`).
    pub serve_url: Option<String>,
}

// ── new session ───────────────────────────────────────────────────────────────

pub fn execute_new(args: NewSessionArgs) -> anyhow::Result<()> {
    use std::sync::Arc;

    type SendFn = Box<dyn Fn(&str) -> anyhow::Result<()>>;
    type VoidFn = Box<dyn Fn()>;
    type Events = std::sync::mpsc::Receiver<aikit_sdk::AgentEvent>;

    // Both control handles share the same method names, so we normalise them
    // to closures and drive the rest of the function identically.
    let (send_turn_fn, interrupt_fn, disconnect_fn, events): (SendFn, VoidFn, VoidFn, Events) =
        match args.agent.as_str() {
            "claude" => {
                let opts = ClaudeSessionOptions {
                    model: args.model.clone(),
                    ..ClaudeSessionOptions::default()
                };
                let (ctrl, evts) = open_claude_session(&args.prompt, opts)
                    .map_err(|e| anyhow::anyhow!("Failed to open claude session: {e}"))?
                    .into_parts();
                let ctrl = Arc::new(ctrl);
                let c1 = Arc::clone(&ctrl);
                let c2 = Arc::clone(&ctrl);
                let c3 = Arc::clone(&ctrl);
                (
                    Box::new(move |text: &str| {
                        c1.send_turn(text).map_err(|e| anyhow::anyhow!("{e}"))
                    }),
                    Box::new(move || {
                        let _ = c2.interrupt();
                    }),
                    Box::new(move || {
                        let _ = c3.disconnect();
                    }),
                    evts,
                )
            }
            "codex" => {
                let opts = CodexSessionOptions::default()
                    .with_approval_policy(args.approval_policy.clone())
                    .with_sandbox(args.sandbox.clone());
                let (ctrl, evts) = open_codex_session(&args.prompt, opts)
                    .map_err(|e| anyhow::anyhow!("Failed to open codex session: {e}"))?
                    .into_parts();
                let ctrl = Arc::new(ctrl);
                let c1 = Arc::clone(&ctrl);
                let c2 = Arc::clone(&ctrl);
                let c3 = Arc::clone(&ctrl);
                (
                    Box::new(move |text: &str| {
                        c1.send_turn(text).map_err(|e| anyhow::anyhow!("{e}"))
                    }),
                    Box::new(move || {
                        let _ = c2.interrupt();
                    }),
                    Box::new(move || {
                        let _ = c3.disconnect();
                    }),
                    evts,
                )
            }
            other => anyhow::bail!(
                "Unknown agent '{}'. Live sessions support 'claude' or 'codex'.",
                other
            ),
        };

    let events_thread = std::thread::spawn({
        let ndjson = args.events;
        move || {
            while let Ok(event) = events.recv() {
                print_event(&event, ndjson);
            }
        }
    });

    run_repl(&send_turn_fn, &interrupt_fn, &disconnect_fn)?;
    let _ = events_thread.join();
    Ok(())
}

/// Drive a multi-turn REPL loop.  Reads lines from stdin; `/interrupt` sends
/// an interrupt; an empty EOF or `/quit` ends the session.
fn run_repl(
    send_turn: &dyn Fn(&str) -> anyhow::Result<()>,
    interrupt: &dyn Fn(),
    disconnect: &dyn Fn(),
) -> anyhow::Result<()> {
    let stdin = io::stdin();
    loop {
        print!("> ");
        let _ = io::stdout().flush();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => return Err(anyhow::anyhow!("stdin error: {e}")),
        }

        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        match text {
            "/quit" | "/exit" => break,
            "/interrupt" => interrupt(),
            _ => send_turn(text)?,
        }
    }
    disconnect();
    Ok(())
}

// ── list sessions ─────────────────────────────────────────────────────────────

pub fn execute_list(args: ListSessionsArgs) -> anyhow::Result<()> {
    let base_url = args
        .serve_url
        .or_else(|| std::env::var("AIKIT_SERVE_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:8080".to_string());

    let url = format!("{}/api/v1/live-sessions", base_url.trim_end_matches('/'));
    let resp = reqwest::blocking::Client::new()
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .map_err(|e| anyhow::anyhow!("Could not reach {url}: {e}"))?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "Server returned {}: {}",
            resp.status(),
            resp.text().unwrap_or_default()
        );
    }

    let body: serde_json::Value = resp.json()?;
    let sessions = body.get("sessions").and_then(|v| v.as_array());
    match sessions {
        None => println!("No active live sessions."),
        Some(list) if list.is_empty() => println!("No active live sessions."),
        Some(list) => {
            for s in list {
                let id = s.get("session_id").and_then(|v| v.as_str()).unwrap_or("?");
                let agent = s.get("agent").and_then(|v| v.as_str()).unwrap_or("?");
                let status = s.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                println!("{id}  agent={agent}  status={status}");
            }
        }
    }
    Ok(())
}

// ── event formatting ──────────────────────────────────────────────────────────

fn print_event(event: &AgentEvent, ndjson: bool) {
    if ndjson {
        if let Ok(s) = event.to_json_string() {
            println!("{s}");
        }
        return;
    }
    match &event.payload {
        AgentEventPayload::StreamMessage(m) => {
            if !m.text.is_empty() {
                print!("{}", m.text);
                let _ = io::stdout().flush();
            }
        }
        AgentEventPayload::ToolUse {
            tool_name, input, ..
        } => {
            println!("\n[tool: {tool_name}] {input}");
        }
        AgentEventPayload::ToolResult {
            output, is_error, ..
        } => {
            let tag = if *is_error { "error" } else { "result" };
            println!("[{tag}] {output}");
        }
        AgentEventPayload::TokenUsageLine { usage, .. } => {
            println!(
                "\n[usage] in={} out={}",
                usage.input_tokens, usage.output_tokens
            );
        }
        AgentEventPayload::AikitStepFinish { finish_reason, .. } => {
            println!("\n[{finish_reason}]");
        }
        AgentEventPayload::RawLine(s) => {
            eprintln!("[stderr] {s}");
        }
        _ => {}
    }
}
