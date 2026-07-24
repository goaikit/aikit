use aikit_sdk::runner::{exit_code_for, format_capabilities, Backend, SandboxPolicy};
use aikit_sdk::session_store::SessionStore;
use aikit_sdk::{run_agent, run_agent_events, run_builtin_agent, AgentEvent, OutputMode};
use aikit_sdk::{ProgressViewConfig, RunError, RunOptions, RunProgress};
use anyhow::Result;
use std::io::{self, Read, Write};

use crate::core::agent_definition::{
    load_persisted_registry, parse_agent_markdown, parse_session_agents_json, AgentDefinition,
    DefinitionRecord, DefinitionSource,
};
use crate::tui::progress_render::{ProgressRenderer, ProgressRendererSink};

#[derive(Debug, Default)]
pub struct RunArgs {
    pub agent: String,
    pub model: Option<String>,
    pub prompt: Option<String>,
    pub yolo: bool,
    pub stream: bool,
    pub events: bool,
    pub progress: bool,
    pub dry_run: bool,
    pub session_agents: Option<String>,
    pub session_persona: Option<String>,
    pub resume: Option<String>,
    pub resume_last: bool,
    // spec 013 (slice 2): common invocation envelope
    pub sandbox: Option<String>,
    pub auto_approve: bool,
    pub cd: Option<String>,
    pub add_dir: Vec<String>,
    pub output_result: Option<String>,
    pub output_schema: Option<String>,
    pub bare: bool,
    pub ephemeral: bool,
    pub skip_git_repo_check: bool,
    pub capabilities: bool,
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
    // spec 013: --capabilities short-circuits before any prompt/run.
    if args.capabilities {
        let backend = Backend::from_key(&args.agent).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown agent '{}' for --capabilities (use a concrete key: \
                 codex|claude|gemini|opencode|cursor|aikit)",
                args.agent
            )
        })?;
        print!("{}", format_capabilities(backend));
        return Ok(());
    }

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

    // D2 / ADR 0012: a session persona carries a least-privilege tool policy
    // (AgentPersona.tools/disallowed_tools). Only the in-process `aikit` backend enforces it;
    // handing it to an external backend would silently drop the tool policy while the caller
    // believes it was applied. Fail loud — matches serve's F2 422 `unsupported_tool_policy`.
    // Unknown agent keys are left to the existing dispatch error path below.
    if session_persona_json.is_some() {
        if let Some(backend) = aikit_sdk::runner::Backend::from_key(&agent) {
            if !backend.capabilities().supports_tool_policy {
                eprintln!(
                    "error: --session-persona: session personas are only enforced by the \
                     in-process 'aikit' backend; agent '{}' cannot apply a persona/tool policy. \
                     Omit --session-persona or use --agent aikit.",
                    agent
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

    // Resolve session ID from --resume or --resume-last.
    let resolved_session_id: Option<String> = if let Some(ref id) = args.resume {
        Some(id.clone())
    } else if args.resume_last {
        let store = SessionStore::open();
        match store.last_for_cwd(&workdir.to_string_lossy()) {
            Ok(Some(id)) => Some(id),
            Ok(None) => {
                eprintln!("error: no previous session found for this directory");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("error: could not read session index: {:?}", e);
                std::process::exit(1);
            }
        }
    } else {
        None
    };

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
    if let Some(ref sid) = resolved_session_id {
        options = options.with_session_id(sid.clone());
    }

    // spec 013 (slice 2): invocation envelope.
    // --yolo is a macro over `--sandbox unrestricted --auto-approve` (D1); an
    // explicit --sandbox wins.
    let explicit_sandbox: Option<SandboxPolicy> = match args.sandbox.as_deref() {
        Some(s) => Some(s.parse::<SandboxPolicy>().map_err(|_| {
            anyhow::anyhow!("--sandbox must be read-only|bounded-write|unrestricted")
        })?),
        None => None,
    };
    // --yolo is a macro over `--sandbox unrestricted --auto-approve` (D1); an
    // explicit --sandbox wins.
    let yolo_sandbox = if args.yolo {
        Some(SandboxPolicy::Unrestricted)
    } else {
        None
    };
    if let Some(p) = explicit_sandbox.or(yolo_sandbox) {
        options = options.with_sandbox(p);
    }
    if args.auto_approve || args.yolo {
        options = options.with_auto_approve(true);
    }
    if let Some(ref d) = args.cd {
        options = options.with_current_dir(std::path::PathBuf::from(d));
    }
    for d in &args.add_dir {
        options = options.with_extra_writable_root(std::path::PathBuf::from(d));
    }
    if let Some(ref f) = args.output_result {
        options = options.with_result_file(std::path::PathBuf::from(f));
    }
    if let Some(ref f) = args.output_schema {
        let raw = std::fs::read_to_string(f)
            .map_err(|e| anyhow::anyhow!("--output-schema: cannot read {}: {}", f, e))?;
        let val: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("--output-schema: invalid JSON in {}: {}", f, e))?;
        options = options.with_output_schema(val);
    }
    if args.bare {
        options = options.with_bare(true);
    }
    if args.ephemeral {
        options = options.with_ephemeral(true);
    }
    if args.skip_git_repo_check {
        options = options.with_skip_git_repo_check(true);
    }

    // spec 013 D3: capture the result-file path before `options` is moved; the
    // SDK now emits the terminal Result event canonically.
    let result_file = options.result_file.clone();

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
                if let Some(p) = result_file.as_deref() {
                    write_result_file(p, &String::from_utf8_lossy(&result.stdout));
                }
                std::process::exit(exit_code_for(result.exit_code(), None));
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(exit_code_for(None, Some(&e)));
            }
        }
    } else if args.events {
        // spec 013 D3: the SDK emits a terminal Result event; capture its text
        // for --output-result. No duplicate CLI emission.
        let mut result_text: Option<String> = None;
        match run_agent_events(&agent, &prompt, options, |event: AgentEvent| {
            if let aikit_sdk::runner::AgentEventPayload::Result { text, .. } = &event.payload {
                result_text = Some(text.clone());
            }
            if let Ok(line) = serde_json::to_string(&event) {
                println!("{}", line);
            }
        }) {
            Ok(result) => {
                let _ = io::stderr().write_all(&result.stderr);
                if let (Some(p), Some(t)) = (result_file.as_deref(), result_text.as_deref()) {
                    write_result_file(p, t);
                }
                std::process::exit(exit_code_for(result.exit_code(), None));
            }
            Err(RunError::AgentNotRunnable(key)) => {
                eprintln!("{}", RunError::AgentNotRunnable(key));
                std::process::exit(exit_code_for(None, None));
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(exit_code_for(None, Some(&e)));
            }
        }
    } else if args.progress {
        let mut progress = RunProgress::new(ProgressViewConfig::default());
        let mut renderer = ProgressRenderer::new().unwrap_or_else(|_| ProgressRenderer::non_tty());
        let agent_key = agent.clone();
        let mut result_text: Option<String> = None;
        match run_agent_events(&agent, &prompt, options, |event: AgentEvent| {
            if let aikit_sdk::runner::AgentEventPayload::Result { text, .. } = &event.payload {
                result_text = Some(text.clone());
            }
            progress.push(&agent_key, &event);
            let _ = renderer.render(&progress);
        }) {
            Ok(result) => {
                let exit_code = exit_code_for(result.exit_code(), None);
                if let (Some(p), Some(t)) = (result_file.as_deref(), result_text.as_deref()) {
                    write_result_file(p, t);
                }
                let _ = renderer.finalize(exit_code, progress.token_footer());
                std::process::exit(exit_code);
            }
            Err(RunError::AgentNotRunnable(key)) => {
                eprintln!("{}", RunError::AgentNotRunnable(key));
                std::process::exit(exit_code_for(None, None));
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(exit_code_for(None, Some(&e)));
            }
        }
    } else {
        match run_agent(&agent, &prompt, options) {
            Ok(result) => {
                io::stdout().write_all(&result.stdout)?;
                io::stderr().write_all(&result.stderr)?;
                if let Some(p) = result_file.as_deref() {
                    write_result_file(p, &String::from_utf8_lossy(&result.stdout));
                }
                std::process::exit(exit_code_for(result.status.code(), None));
            }
            Err(RunError::AgentNotRunnable(key)) => {
                eprintln!("{}", RunError::AgentNotRunnable(key));
                std::process::exit(exit_code_for(None, None));
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(exit_code_for(None, Some(&e)));
            }
        }
    }
}

/// Write the `--output-result` file (spec 013 D3). Best-effort on error: the
/// run already produced its output, so a write failure is reported on stderr
/// rather than turning a successful run into a non-zero exit.
fn write_result_file(path: &std::path::Path, text: &str) {
    if let Err(e) = std::fs::write(path, text) {
        eprintln!("--output-result: cannot write {}: {}", path.display(), e);
    }
}
