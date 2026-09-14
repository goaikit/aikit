//! `aikit session` — agent sessions, live and on disk.
//!
//! Subcommands:
//! - `new`       — open a live session and enter a multi-turn REPL
//! - `list`      — list captured sessions on disk (`--live` lists the live
//!   sessions of a running `aikit serve` instead)
//! - `sync`      — upload scrubbed transcripts to S3-compatible storage
//! - `summarize` — generate session briefs (summary, areas, tags) — ADR 0022

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
use aikit_session_capture::{Adapter, CursorStore, EventStore, SessionSummary};
#[cfg(feature = "agent-adapters")]
use aikit_session_capture::{Registry, ToolKind};
#[cfg(feature = "agent-adapters")]
use aikit_session_summarize::{
    adapters_for, parse_location, parse_since, parse_tool_kind, select_sessions, AreaMapping,
    LocationSpec, ModelConfig, Outcome, PromptSource, Selection, SummarizeOptions, Summarizer,
    TagList, TagMirror,
};
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

#[derive(Debug, Default)]
pub struct ListSessionsArgs {
    /// `--live`: list the live sessions of a running `aikit serve` instead
    /// of captured sessions on disk.
    pub live: bool,
    /// Base URL of a running `aikit serve` instance (default: `AIKIT_SERVE_URL`).
    pub serve_url: Option<String>,
    /// `--tool`, repeatable.
    pub tools: Vec<String>,
    /// `--path [<tool>=]<dir>`, repeatable.
    pub paths: Vec<String>,
    /// `--since <duration|timestamp>`.
    pub since: Option<String>,
    /// `--db <file>` (or `AIKIT_CAPTURE_DB`).
    pub db: Option<String>,
    /// `default` or `json`.
    pub format: String,
}

#[derive(Debug, Default)]
pub struct SummarizeSessionsArgs {
    pub sessions: Vec<String>,
    pub all: bool,
    pub since: Option<String>,
    pub tools: Vec<String>,
    pub paths: Vec<String>,
    pub db: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
    pub max_tokens: Option<String>,
    pub timeout: Option<String>,
    pub tags: Option<String>,
    pub tags_file: Option<String>,
    pub areas_file: Option<String>,
    pub area_depth: Option<String>,
    pub include_assistant: bool,
    pub parallel: Option<String>,
    pub force: bool,
    pub no_mirror: bool,
    pub dry_run: bool,
    pub format: String,
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
    pub log_format: String,
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

/// `aikit session list`. Captured sessions on disk by default; `--live`
/// keeps the previous behaviour (live sessions of a running `aikit serve`).
/// Returns the process exit code.
pub async fn execute_list(args: ListSessionsArgs) -> anyhow::Result<i32> {
    if args.live {
        let serve_url = args.serve_url.clone();
        return tokio::task::spawn_blocking(move || execute_list_live(serve_url))
            .await
            .map_err(|e| anyhow::anyhow!("task join error: {}", e))?
            .map(|()| 0);
    }
    execute_list_disk(args).await
}

fn execute_list_live(serve_url: Option<String>) -> anyhow::Result<()> {
    let base_url = serve_url
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
async fn execute_list_disk(args: ListSessionsArgs) -> anyhow::Result<i32> {
    let json = match parse_format(&args.format) {
        Ok(j) => j,
        Err(code) => return Ok(code),
    };
    let scan = match Scan::prepare(&args.tools, &args.paths, args.db.as_deref()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return Ok(2);
        }
    };
    let since_ms = match args.since.as_deref().map(parse_since_now) {
        Some(Err(e)) => {
            eprintln!("Error: {e}");
            return Ok(2);
        }
        Some(Ok(ms)) => Some(ms),
        None => None,
    };
    let mut sessions = scan.run().await?;
    if let Some(since) = since_ms {
        sessions.retain(|s| s.last_event_at_ms >= since);
    }
    sessions.sort_by(|a, b| b.last_event_at_ms.cmp(&a.last_event_at_ms));

    if json {
        println!("{}", serde_json::to_string(&sessions)?);
        return Ok(0);
    }
    if sessions.is_empty() {
        println!("No captured sessions.");
        return Ok(0);
    }
    let header = [
        "SESSION", "TOOL", "START", "END", "ACTIONS", "GIT ROOT", "SOURCE",
    ];
    println!(
        "{:<36}  {:<11}  {:<20}  {:<20}  {:>7}  {:<30}  {}",
        header[0], header[1], header[2], header[3], header[4], header[5], header[6]
    );
    for s in &sessions {
        println!(
            "{:<36}  {:<11}  {:<20}  {:<20}  {:>7}  {:<30}  {}",
            s.session_id,
            s.tool.as_str(),
            aikit_session_summarize::digest::fmt_ms(s.first_event_at_ms),
            aikit_session_summarize::digest::fmt_ms(s.last_event_at_ms),
            s.action_count,
            s.git_root
                .as_deref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "-".into()),
            s.source_file.display()
        );
    }
    Ok(0)
}

#[cfg(not(feature = "agent-adapters"))]
async fn execute_list_disk(_args: ListSessionsArgs) -> anyhow::Result<i32> {
    eprintln!("Error: listing captured sessions requires the agent-adapters feature (use --live for live sessions)");
    Ok(2)
}

// ── shared: locations, store, scan ────────────────────────────────────────────

/// `--format default|json` → is json. `Err(2)` on anything else.
fn parse_format(raw: &str) -> Result<bool, i32> {
    match raw {
        "default" => Ok(false),
        "json" => Ok(true),
        other => {
            eprintln!("Error: --format must be default or json, got {other}");
            Err(2)
        }
    }
}

#[cfg(feature = "agent-adapters")]
fn parse_since_now(raw: &str) -> anyhow::Result<i64> {
    let now = chrono::Utc::now().timestamp_millis();
    parse_since(raw, now).map_err(|e| anyhow::anyhow!("{e}"))
}

/// The capture SQLite file: `--db`, else `AIKIT_CAPTURE_DB`, else the one
/// `aikit serve` uses.
#[cfg(feature = "agent-adapters")]
fn capture_db(flag: Option<&str>) -> PathBuf {
    flag.map(PathBuf::from)
        .or_else(|| std::env::var_os("AIKIT_CAPTURE_DB").map(PathBuf::from))
        .unwrap_or_else(super::serve::capture_db_path)
}

/// Adapters and stores resolved from `--tool`, `--path` and `--db`; one scan
/// then lists what the store holds for the selected tools.
#[cfg(feature = "agent-adapters")]
struct Scan {
    tools: Option<Vec<ToolKind>>,
    locations: Vec<LocationSpec>,
    adapters: Vec<Box<dyn Adapter>>,
    event_store: Arc<dyn EventStore>,
    cursor_store: Arc<dyn CursorStore>,
}

#[cfg(feature = "agent-adapters")]
impl Scan {
    fn prepare(tools: &[String], paths: &[String], db: Option<&str>) -> anyhow::Result<Self> {
        let tools = parse_tools(tools)?;
        let locations = paths
            .iter()
            .map(|p| parse_location(p).map_err(|e| anyhow::anyhow!("--path {p}: {e}")))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let adapters = adapters_for(tools.as_deref(), &locations);
        if adapters.is_empty() {
            anyhow::bail!("no adapter matches the selected --tool / --path");
        }
        let db_path = capture_db(db);
        let conn = super::serve::storage::schema::open(&db_path)
            .map_err(|e| anyhow::anyhow!("opening capture store {}: {e}", db_path.display()))?;
        Ok(Self {
            tools,
            locations,
            adapters,
            event_store: Arc::new(super::serve::storage::SqliteEventStore::new(conn.clone())),
            cursor_store: Arc::new(super::serve::storage::SqliteCursorStore::new(conn)),
        })
    }

    /// Scan every adapter (cursor-resumed, idempotent), then list the
    /// selected tools' sessions. With `--path`, only sessions whose source
    /// file lies under one of the given roots are listed.
    async fn run(&self) -> anyhow::Result<Vec<SessionSummary>> {
        let mut outcome = aikit_session_capture::IngestOutcome::default();
        for adapter in &self.adapters {
            outcome.absorb(
                aikit_session_capture::scan_adapter(
                    adapter.as_ref(),
                    self.event_store.as_ref(),
                    self.cursor_store.as_ref(),
                    false,
                )
                .await,
            );
        }
        for w in &outcome.warnings {
            tracing::warn!(target: "aikit::session", "scan warning: {w:?}");
        }
        eprintln!(
            "scanned {} files ({} unchanged, {} warnings)",
            outcome.files_scanned + outcome.files_skipped,
            outcome.files_skipped,
            outcome.warnings.len()
        );

        let kinds: Vec<ToolKind> = {
            let mut k: Vec<ToolKind> = self.adapters.iter().map(|a| a.kind()).collect();
            k.dedup();
            k
        };
        let mut sessions = Vec::new();
        for kind in kinds {
            if let Some(allow) = &self.tools {
                if !allow.contains(&kind) {
                    continue;
                }
            }
            sessions.extend(
                self.event_store
                    .sessions_for(kind, None, u32::MAX, 0)
                    .await
                    .map_err(|e| anyhow::anyhow!("listing sessions: {e}"))?,
            );
        }
        if !self.locations.is_empty() {
            sessions.retain(|s| {
                self.locations
                    .iter()
                    .any(|l| s.source_file.starts_with(&l.path))
            });
        }
        Ok(sessions)
    }
}

// ── summarize ─────────────────────────────────────────────────────────────────

/// `aikit session summarize`. Returns the process exit code: `0` all done,
/// `1` at least one session failed, `2` configuration error.
#[cfg(feature = "agent-adapters")]
pub async fn execute_summarize(args: SummarizeSessionsArgs) -> anyhow::Result<i32> {
    use aikit_sdk::llm::openai_compat::OpenAiCompatProvider;
    use aikit_sdk::llm::{resolve_api_key, LlmGateway};

    let json = match parse_format(&args.format) {
        Ok(j) => j,
        Err(code) => return Ok(code),
    };

    // Tag list: --tags, --tags-file, AIKIT_SESSION_TAGS, else built-in.
    let tags = match (args.tags.as_deref(), args.tags_file.as_deref()) {
        (Some(_), Some(_)) => {
            eprintln!("Error: use either --tags or --tags-file, not both");
            return Ok(2);
        }
        (Some(names), None) => {
            TagList::from_names(names.split(',').filter(|s| !s.trim().is_empty()))
        }
        (None, file) => {
            let path = file
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("AIKIT_SESSION_TAGS").map(PathBuf::from));
            match path {
                Some(p) => match std::fs::read_to_string(&p) {
                    Ok(text) => TagList::from_toml(&text),
                    Err(e) => {
                        eprintln!("Error: reading tags file {}: {e}", p.display());
                        return Ok(2);
                    }
                },
                None => Ok(TagList::builtin()),
            }
        }
    };
    let tags = match tags {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error: {e}");
            return Ok(2);
        }
    };

    let area_depth = match parse_num(args.area_depth.as_deref(), "--area-depth", 2usize) {
        Ok(v) => v.max(1),
        Err(code) => return Ok(code),
    };
    let areas = match args.areas_file.as_deref() {
        Some(p) => match std::fs::read_to_string(p)
            .map_err(|e| e.to_string())
            .and_then(|t| AreaMapping::from_toml(&t, area_depth).map_err(|e| e.to_string()))
        {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Error: areas file {p}: {e}");
                return Ok(2);
            }
        },
        None => AreaMapping {
            depth: area_depth,
            ..Default::default()
        },
    };
    let parallel = match parse_num(args.parallel.as_deref(), "--parallel", 4usize) {
        Ok(v) => v.max(1),
        Err(code) => return Ok(code),
    };
    let max_tokens = match parse_num(args.max_tokens.as_deref(), "--max-tokens", 1024u32) {
        Ok(v) => v,
        Err(code) => return Ok(code),
    };
    let timeout = match parse_num(args.timeout.as_deref(), "--timeout", 120u64) {
        Ok(v) => v,
        Err(code) => return Ok(code),
    };

    let selection = Selection {
        ids: args.sessions.clone(),
        since_ms: match args.since.as_deref().map(parse_since_now) {
            Some(Err(e)) => {
                eprintln!("Error: {e}");
                return Ok(2);
            }
            Some(Ok(ms)) => Some(ms),
            None => None,
        },
        all: args.all,
    };
    if selection.ids.is_empty() && selection.since_ms.is_none() && !selection.all {
        eprintln!("Error: select sessions with --session <id>, --since <when> or --all");
        return Ok(2);
    }

    // Model: resolved before any scan so a misconfiguration exits 2 fast.
    let (gateway, model): (Arc<dyn LlmGateway>, ModelConfig) = if args.dry_run {
        (
            Arc::new(aikit_sdk::llm::mock::MockGateway::new(vec![])),
            ModelConfig::new("dry-run", "", ""),
        )
    } else {
        let Some(model) = args
            .model
            .clone()
            .or_else(|| std::env::var("AIKIT_MODEL").ok())
        else {
            eprintln!("Error: --model (or AIKIT_MODEL) is required unless --dry-run");
            return Ok(2);
        };
        let base_url = args
            .base_url
            .clone()
            .or_else(|| std::env::var("AIKIT_LLM_URL").ok())
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        let api_key = match resolve_api_key(args.api_key_env.as_deref()) {
            Ok(k) => k,
            Err(e) => {
                eprintln!("Error: {e}");
                return Ok(2);
            }
        };
        let provider =
            OpenAiCompatProvider::new(timeout, 10).map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut cfg = ModelConfig::new(model, base_url, api_key);
        cfg.max_tokens = max_tokens;
        (Arc::new(provider), cfg)
    };

    let scan = match Scan::prepare(&args.tools, &args.paths, args.db.as_deref()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return Ok(2);
        }
    };
    let sessions = scan.run().await?;
    let selected = match select_sessions(sessions, &selection) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return Ok(2);
        }
    };
    if selected.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No captured sessions match the selection.");
        }
        return Ok(0);
    }

    let options = SummarizeOptions {
        tags,
        areas,
        digest: aikit_session_summarize::DigestOptions {
            include_assistant: args.include_assistant,
            ..Default::default()
        },
        force: args.force,
        dry_run: args.dry_run,
        parallel,
        mirror: !args.no_mirror,
    };
    let summarizer = Arc::new(
        Summarizer::new(gateway, model, options)
            .with_prompt_source(Arc::new(HistoryPrompts))
            .with_tag_mirror(Arc::new(HistoryTagMirror)),
    );
    let outcomes = summarizer
        .summarize_many(Arc::clone(&scan.event_store), selected)
        .await;

    let mut failed = 0;
    if json {
        let rows: Vec<serde_json::Value> = outcomes.iter().map(outcome_json).collect();
        println!("{}", serde_json::to_string(&rows)?);
        failed = outcomes
            .iter()
            .filter(|o| matches!(o.outcome, Outcome::Failed { .. }))
            .count();
    } else {
        for o in &outcomes {
            match &o.outcome {
                Outcome::Generated { brief, mirrored } => {
                    print_brief(&o.session_id, o.tool, "generated", brief);
                    if let Some(Err(e)) = mirrored {
                        eprintln!("  warning: tag not mirrored: {e}");
                    }
                }
                Outcome::Unchanged { brief } => {
                    print_brief(&o.session_id, o.tool, "unchanged", brief);
                }
                Outcome::DryRun {
                    user_message,
                    mechanical,
                    digest_hash,
                } => {
                    println!(
                        "==> {}  {}  dry-run  digest={}  mechanical=[{}]",
                        o.session_id,
                        o.tool.as_str(),
                        &digest_hash[..12],
                        mechanical
                            .iter()
                            .map(|t| t.name.as_str())
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    println!("{user_message}");
                }
                Outcome::Failed { error } => {
                    failed += 1;
                    println!("==> {}  {}  failed: {error}", o.session_id, o.tool.as_str());
                }
            }
        }
    }
    Ok(if failed == 0 { 0 } else { 1 })
}

#[cfg(not(feature = "agent-adapters"))]
pub async fn execute_summarize(_args: SummarizeSessionsArgs) -> anyhow::Result<i32> {
    eprintln!("Error: session summarize requires the agent-adapters feature");
    Ok(2)
}

#[cfg(feature = "agent-adapters")]
fn parse_num<T: std::str::FromStr>(raw: Option<&str>, flag: &str, default: T) -> Result<T, i32> {
    match raw {
        None => Ok(default),
        Some(s) => s.trim().parse::<T>().map_err(|_| {
            eprintln!("Error: {flag} must be a number, got {s}");
            2
        }),
    }
}

#[cfg(feature = "agent-adapters")]
fn print_brief(
    session_id: &str,
    tool: ToolKind,
    status: &str,
    brief: &aikit_session_capture::SessionBrief,
) {
    let tags: Vec<String> = brief
        .tags
        .iter()
        .map(|t| match t.source {
            aikit_session_capture::TagSource::Mechanical => format!("{}*", t.name),
            _ => t.name.clone(),
        })
        .collect();
    println!(
        "==> {}  {}  {}  tags=[{}]  model={}",
        session_id,
        tool.as_str(),
        status,
        tags.join(","),
        brief.model
    );
    println!("    {}", brief.summary);
    for a in &brief.areas {
        println!(
            "    {:<40} reads={:<4} mods={:<4} time={}s",
            a.area,
            a.reads,
            a.modifications,
            a.time_ms / 1000
        );
    }
    for t in &brief.tags {
        println!("    #{}: {}", t.name, t.justification);
    }
    if !brief.rejected_tags.is_empty() {
        println!("    rejected: {}", brief.rejected_tags.join(", "));
    }
}

#[cfg(feature = "agent-adapters")]
fn outcome_json(o: &aikit_session_summarize::SessionOutcome) -> serde_json::Value {
    let mut v = serde_json::json!({
        "tool": o.tool.as_str(),
        "session_id": o.session_id,
    });
    match &o.outcome {
        Outcome::Generated { brief, mirrored } => {
            v["status"] = "generated".into();
            v["brief"] = serde_json::to_value(brief).unwrap_or_default();
            v["mirrored"] = match mirrored {
                Some(Ok(b)) => serde_json::json!(b),
                Some(Err(e)) => serde_json::json!({"error": e}),
                None => serde_json::Value::Null,
            };
        }
        Outcome::Unchanged { brief } => {
            v["status"] = "unchanged".into();
            v["brief"] = serde_json::to_value(brief).unwrap_or_default();
        }
        Outcome::DryRun {
            user_message,
            mechanical,
            digest_hash,
        } => {
            v["status"] = "dry_run".into();
            v["user_message"] = user_message.clone().into();
            v["mechanical_tags"] = serde_json::to_value(mechanical).unwrap_or_default();
            v["digest_hash"] = digest_hash.clone().into();
        }
        Outcome::Failed { error } => {
            v["status"] = "failed".into();
            v["error"] = error.clone().into();
        }
    }
    v
}

// ── history integration: prompts in, primary tag out ──────────────────────────

#[cfg(feature = "agent-adapters")]
fn backend_for(tool: ToolKind) -> Option<aikit_sdk::runner::Backend> {
    use aikit_sdk::runner::Backend;
    match tool {
        ToolKind::ClaudeCode => Some(Backend::Claude),
        ToolKind::Codex => Some(Backend::Codex),
        ToolKind::OpenCode => Some(Backend::OpenCode),
        _ => None,
    }
}

/// User prompts through the history reader, for Backends that have one.
/// Text returned here is scrubbed by the summarizer before it reaches a
/// digest.
#[cfg(feature = "agent-adapters")]
struct HistoryPrompts;

#[cfg(feature = "agent-adapters")]
impl PromptSource for HistoryPrompts {
    fn user_prompts(&self, tool: ToolKind, session_id: &str) -> Option<Vec<String>> {
        use aikit_sdk::history::{HistoryBlock, HistoryContent, MessagesQuery};
        use aikit_sdk::MessageRole;
        let backend = backend_for(tool)?;
        if !backend.capabilities().history_store {
            return None;
        }
        let reader = backend.history_reader()?;
        let mut q = MessagesQuery::default();
        q.limit = Some(500);
        let messages = reader.messages(session_id, &q, None).ok()?;
        let prompts: Vec<String> = messages
            .into_iter()
            .filter(|m| matches!(m.role, MessageRole::User))
            .filter_map(|m| match m.content {
                HistoryContent::Text(t) => Some(t),
                HistoryContent::Blocks(blocks) => {
                    let text: Vec<String> = blocks
                        .into_iter()
                        .filter_map(|b| match b {
                            HistoryBlock::Text { text } => Some(text),
                            _ => None,
                        })
                        .collect();
                    (!text.is_empty()).then(|| text.join("\n"))
                }
                _ => None,
            })
            .filter(|t| !t.trim().is_empty())
            .collect();
        Some(prompts)
    }
}

/// Mirrors the primary tag into the Backend's tag slot when it has a
/// `HistoryMutator` (Claude today).
#[cfg(feature = "agent-adapters")]
struct HistoryTagMirror;

#[cfg(feature = "agent-adapters")]
impl TagMirror for HistoryTagMirror {
    fn mirror(&self, tool: ToolKind, session_id: &str, tag: &str) -> Result<bool, String> {
        let Some(backend) = backend_for(tool) else {
            return Ok(false);
        };
        if !backend.capabilities().history_mutations {
            return Ok(false);
        }
        let Some(mutator) = backend.history_mutator() else {
            return Ok(false);
        };
        mutator
            .tag(session_id, Some(tag), None)
            .map(|()| true)
            .map_err(|e| e.to_string())
    }
}

/// Install a stderr tracing subscriber for the sync run. No-op if one already
/// exists (a global `--debug` or `RUST_LOG` setup wins). `json` emits ndjson.
#[cfg(feature = "agent-adapters")]
fn init_sync_logging(level: &str, json: bool) {
    use tracing_subscriber::EnvFilter;
    let filter = std::env::var("RUST_LOG")
        .ok()
        .map(EnvFilter::new)
        .unwrap_or_else(|| EnvFilter::new(format!("aikit_session_sync={level},warn")));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr);
    let _ = if json {
        builder.json().flatten_event(true).try_init()
    } else {
        builder.try_init()
    };
}

#[cfg(feature = "agent-adapters")]
pub async fn execute_sync(args: SyncSessionsArgs) -> anyhow::Result<i32> {
    // Turn on leveled sync logging to stderr. No-op if a subscriber is already
    // installed (RUST_LOG / global --debug take precedence). --log-format json
    // emits ndjson for log ingestion.
    {
        let level = args
            .log_level
            .clone()
            .or_else(|| std::env::var("RUST_LOG").ok())
            .unwrap_or_else(|| "info".to_string());
        init_sync_logging(&level, args.log_format.eq_ignore_ascii_case("json"));
    }

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
        match S3Sink::new(S3SinkConfig {
            bucket: config.bucket.clone().unwrap_or_default(),
            endpoint: config.endpoint.clone().unwrap_or_default(),
            region,
            allow_http,
            endpoint_ca_bundle: config.endpoint_ca_bundle.clone(),
            path_style: config.path_style,
        }) {
            Ok(s) => Arc::new(s),
            // Missing/partial AWS credentials — fail fast with a clear message
            // instead of the IMDS retry loop. Config/auth error → exit 2.
            Err(aikit_session_sync::SyncError::Auth(e)) => {
                eprintln!("Error: auth: {e}");
                return Ok(2);
            }
            Err(e) => return Err(anyhow::anyhow!("{e}")),
        }
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
        match parse_tool_kind(item) {
            Some(kind) => tools.push(kind),
            None => anyhow::bail!("unknown --tool '{item}'"),
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
