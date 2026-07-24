//! `aikit session` — interactive bidirectional agent sessions.
//!
//! Subcommands:
//! - `new`  — open a live session and enter a multi-turn REPL
//! - `list` — list active live sessions (requires `AIKIT_SERVE_URL`)

use std::io::{self, BufRead, Write as IoWrite};
use std::path::PathBuf;
use std::sync::Arc;

use aikit_sdk::{
    open_claude_session, open_codex_session, AgentEvent, AgentEventPayload, ClaudeSessionOptions,
    CodexSessionOptions, LiveSession,
};

#[cfg(all(feature = "agent-adapters", feature = "watcher"))]
use aikit_session_capture::watch::{find_adapter_for_path, NotifyWatchDriver, WatchDriver};
#[cfg(feature = "agent-adapters")]
use aikit_session_capture::{Registry, ToolKind};
#[cfg(feature = "agent-adapters")]
use aikit_session_sync::{
    credential_owner_from_env, JsonSyncStateStore, OutputFormat, S3Sink, S3SinkConfig, SyncConfig,
    SyncEngine, SyncSink,
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

#[derive(Debug)]
pub struct SyncSessionsArgs {
    pub bucket: Option<String>,
    pub endpoint: Option<String>,
    pub region: Option<String>,
    pub owner: Option<String>,
    pub key_prefix: Option<String>,
    pub tools: Vec<String>,
    pub watch: bool,
    pub dry_run: bool,
    pub allow_http: bool,
    pub format: String,
    pub log_level: Option<String>,
}

// ── new session ───────────────────────────────────────────────────────────────

pub fn execute_new(args: NewSessionArgs) -> anyhow::Result<()> {
    type Events = std::sync::mpsc::Receiver<aikit_sdk::AgentEvent>;

    // Both backends expose the same `LiveSession` control surface, so we box the
    // concrete handle behind `dyn LiveSession` and drive the REPL identically.
    let (session, events): (Box<dyn LiveSession>, Events) = match args.agent.as_str() {
        "claude" => {
            let opts = ClaudeSessionOptions {
                model: args.model.clone(),
                ..ClaudeSessionOptions::default()
            };
            let (ctrl, evts) = open_claude_session(&args.prompt, opts)
                .map_err(|e| anyhow::anyhow!("Failed to open claude session: {e}"))?
                .into_parts();
            (Box::new(ctrl), evts)
        }
        "codex" => {
            let opts = CodexSessionOptions::default()
                .with_approval_policy(args.approval_policy.clone())
                .with_sandbox(args.sandbox.clone());
            let (ctrl, evts) = open_codex_session(&args.prompt, opts)
                .map_err(|e| anyhow::anyhow!("Failed to open codex session: {e}"))?
                .into_parts();
            (Box::new(ctrl), evts)
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

    run_repl(session.as_ref())?;
    let _ = events_thread.join();
    Ok(())
}

/// Drive a multi-turn REPL loop.  Reads lines from stdin; `/interrupt` sends
/// an interrupt; an empty EOF or `/quit` ends the session.
fn run_repl(session: &dyn LiveSession) -> anyhow::Result<()> {
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
            "/interrupt" => {
                let _ = session.interrupt();
            }
            _ => session
                .send_turn(text.to_string())
                .map_err(|e| anyhow::anyhow!("{e}"))?,
        }
    }
    let _ = session.disconnect();
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

#[cfg(feature = "agent-adapters")]
pub async fn execute_sync(args: SyncSessionsArgs) -> anyhow::Result<i32> {
    let bucket = args
        .bucket
        .or_else(|| std::env::var("AIKIT_SYNC_BUCKET").ok());
    let endpoint = args
        .endpoint
        .or_else(|| std::env::var("AIKIT_SYNC_ENDPOINT").ok());
    let region = args
        .region
        .or_else(|| std::env::var("AIKIT_SYNC_REGION").ok())
        .unwrap_or_else(|| "us-east-1".to_string());
    let owner = args
        .owner
        .or_else(|| std::env::var("AIKIT_SYNC_OWNER").ok());
    let key_prefix = args
        .key_prefix
        .or_else(|| std::env::var("AIKIT_SYNC_PREFIX").ok())
        .unwrap_or_else(|| "sessions/".to_string());
    let allow_http = args.allow_http || env_bool("AIKIT_SYNC_ALLOW_HTTP");
    let format = match args.format.as_str() {
        "default" => OutputFormat::Default,
        "json" => OutputFormat::Json,
        other => {
            eprintln!("Error: --format must be default or json, got {other}");
            return Ok(2);
        }
    };
    let tools = parse_tools(&args.tools)?;
    let config = SyncConfig {
        bucket: bucket.clone(),
        endpoint: endpoint.clone(),
        region: region.clone(),
        allow_http,
        endpoint_ca_bundle: std::env::var_os("AIKIT_SYNC_ENDPOINT_CA_BUNDLE").map(PathBuf::from),
        path_style: endpoint.as_deref().map(default_path_style).unwrap_or(true),
        owner,
        credential_owner: credential_owner_from_env(),
        key_prefix,
        tools,
        watch: args.watch,
        dry_run: args.dry_run,
        format,
        log_level: args
            .log_level
            .or_else(|| std::env::var("RUST_LOG").ok())
            .unwrap_or_else(|| "info".to_string()),
        ..SyncConfig::default()
    };

    if !config.dry_run
        && (config.bucket.as_deref().unwrap_or("").is_empty()
            || config.endpoint.as_deref().unwrap_or("").is_empty())
    {
        eprintln!(
            "Error: --bucket/AIKIT_SYNC_BUCKET and --endpoint/AIKIT_SYNC_ENDPOINT are required"
        );
        return Ok(2);
    }

    if let Err(aikit_session_sync::SyncError::Auth(e)) = aikit_session_sync::resolve_owner(
        config.owner.as_deref(),
        config.credential_owner.as_deref(),
    ) {
        eprintln!("Error: auth: {e}");
        return Ok(2);
    }

    let sink: Arc<dyn SyncSink> = if config.dry_run {
        Arc::new(aikit_session_sync::InMemorySink::new())
    } else {
        Arc::new(S3Sink::new(S3SinkConfig {
            bucket: config.bucket.clone().unwrap_or_default(),
            endpoint: config.endpoint.clone().unwrap_or_default(),
            region,
            allow_http,
            endpoint_ca_bundle: config.endpoint_ca_bundle.clone(),
            path_style: config.path_style,
        })?)
    };
    let state = Arc::new(JsonSyncStateStore::open()?);
    let engine = match SyncEngine::new(config.clone(), sink, state) {
        Ok(engine) => engine,
        Err(aikit_session_sync::SyncError::Auth(e)) => {
            eprintln!("Error: auth: {e}");
            return Ok(2);
        }
        Err(e) => return Err(anyhow::anyhow!("{e}")),
    };

    let registry = default_registry();
    let summary = engine.sync_detected(&registry).await;
    if matches!(config.format, OutputFormat::Json) {
        println!("{}", serde_json::to_string(&summary)?);
    } else {
        println!(
            "synced={} skipped_unchanged={} failed={} bytes_uploaded={}",
            summary.synced, summary.skipped_unchanged, summary.failed, summary.bytes_uploaded
        );
    }

    if config.watch {
        watch_sync(&engine, &registry, config.tools.as_deref()).await?;
    }
    Ok(if summary.failed == 0 { 0 } else { 1 })
}

#[cfg(not(feature = "agent-adapters"))]
pub async fn execute_sync(_args: SyncSessionsArgs) -> anyhow::Result<i32> {
    eprintln!("Error: session sync requires the agent-adapters feature");
    Ok(2)
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

#[cfg(feature = "agent-adapters")]
fn default_registry() -> Registry {
    let mut registry = Registry::new();
    #[cfg(feature = "claudecode")]
    registry.register(Box::new(
        aikit_session_capture::claudecode::ClaudeCodeAdapter::new(),
    ));
    #[cfg(feature = "codex")]
    registry.register(Box::new(aikit_session_capture::codex::CodexAdapter::new()));
    registry
}

#[cfg(all(feature = "agent-adapters", feature = "watcher"))]
async fn watch_sync(
    engine: &SyncEngine,
    registry: &Registry,
    allow: Option<&[ToolKind]>,
) -> anyhow::Result<()> {
    let adapters: Vec<_> = registry
        .detected(allow)
        .into_iter()
        .filter(|adapter| matches!(adapter.kind(), ToolKind::ClaudeCode | ToolKind::Codex))
        .collect();
    let mut watcher =
        NotifyWatchDriver::new(adapters.clone(), std::time::Duration::from_millis(250))
            .map_err(|e| anyhow::anyhow!("watcher setup failed: {e}"))?;
    while let Some(path) = watcher.next_event().await {
        let Some(adapter) = find_adapter_for_path(&adapters, &path) else {
            continue;
        };
        if let Err(error) = engine
            .retry_with_backoff(
                adapter,
                &path,
                6,
                aikit_session_sync::WatchRetryPolicy::default(),
            )
            .await
        {
            tracing::warn!(target: "aikit_session_sync::watch", path = %path.display(), "sync failed after retry: {error}");
        }
    }
    Ok(())
}

#[cfg(all(feature = "agent-adapters", not(feature = "watcher")))]
async fn watch_sync(
    _engine: &SyncEngine,
    _registry: &Registry,
    _allow: Option<&[ToolKind]>,
) -> anyhow::Result<()> {
    anyhow::bail!("--watch requires the watcher feature")
}

#[cfg(feature = "agent-adapters")]
fn parse_tools(raw: &[String]) -> anyhow::Result<Option<Vec<ToolKind>>> {
    if raw.is_empty() {
        return Ok(None);
    }
    let mut tools = Vec::new();
    for item in raw {
        match item.as_str() {
            "claude_code" | "claudecode" | "claude" => tools.push(ToolKind::ClaudeCode),
            "codex" => tools.push(ToolKind::Codex),
            "open_code" | "opencode" => tools.push(ToolKind::OpenCode),
            other => anyhow::bail!("unknown --tool '{other}'"),
        }
    }
    Ok(Some(tools))
}

#[cfg(feature = "agent-adapters")]
fn env_bool(name: &str) -> bool {
    std::env::var(name)
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
        .unwrap_or(false)
}

#[cfg(feature = "agent-adapters")]
fn default_path_style(endpoint: &str) -> bool {
    !endpoint.contains(".amazonaws.com")
}
