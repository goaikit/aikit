//! CLI command module — builds the cli-framework App for aikit.

mod agents;
mod check;
pub mod context;
mod init;
mod mcp;
mod release;
mod run;
pub mod serve;
pub mod session;
mod template_package;
mod version;

pub mod commands {
    pub mod install;
    pub mod package;
}

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use cli_framework::app::{AppBuilder, AppMeta};
use cli_framework::command::{Command, FromArgValueMap, IntoCommandSpec};
use cli_framework::parser::diagnostic::{Diagnostic, DiagnosticCategory};
use cli_framework::path;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::{CommandPath, CommandSpec, GroupMetadata};
use cli_framework::spec::value::ArgValue;

use context::AikitContext;

pub type AikitApp = cli_framework::app::App<AikitContext>;

fn parse_arg<T: std::str::FromStr>(value: &str, flag: &str, error_msg: &str) -> Result<T> {
    value
        .parse::<T>()
        .map_err(|_| anyhow::anyhow!("{} must be {}", flag, error_msg))
}

pub fn build_app() -> Result<AikitApp> {
    let mut builder = AppBuilder::new()
        .with_version("aikit", env!("CARGO_PKG_VERSION"))
        .with_meta(AppMeta {
            name: "aikit",
            version: env!("CARGO_PKG_VERSION"),
            description: "AIKit - Universal template package manager for AI agents",
            usage: None,
        });

    builder = builder.register(path!["check"], |_ctx, _args: CheckArgs| async move {
        check::execute(check::CheckArgs {}).map_err(|e| anyhow::anyhow!("{}", e))
    })?;

    builder = builder.register(path!["init"], |_ctx, args: InitArgs| async move {
        let init_args = init::InitArgs {
            project_name: args.project_name,
            ai: args.ai,
            script: args.script,
            here: args.here,
            force: args.force,
            no_git: args.no_git,
            github_token: args.github_token,
            skip_tls: args.skip_tls,
            debug: false,
            ignore_agent_tools: args.ignore_agent_tools,
        };
        init::execute(init_args)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))
    })?;

    builder = builder.register(
        path!["install"],
        |_ctx, args: commands::install::InstallArgs| async move {
            commands::install::execute_install(args)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    builder = builder.register(
        path!["update"],
        |_ctx, args: commands::install::UpdateArgs| async move {
            commands::install::execute_update(args)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    builder = builder.register(
        path!["remove"],
        |_ctx, args: commands::install::RemoveArgs| async move {
            commands::install::execute_remove(args)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    builder = builder.register(
        path!["list"],
        |_ctx, args: commands::install::ListArgs| async move {
            commands::install::execute_list(args)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    builder = builder.register(path!["release"], |_ctx, args: ReleaseArgs| async move {
        let release_args = release::ReleaseArgs {
            release_version: args.release_version,
            notes_file: args.notes_file,
            github_token: args.github_token,
        };
        release::execute(release_args)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))
    })?;

    builder = builder.register(path!["serve"], |_ctx, args: ServeArgs| async move {
        let serve_args = serve::ServeArgs {
            host: args.host,
            port: parse_arg::<u16>(&args.port, "--port", "a valid port number")?,
            run_timeout_secs: parse_arg::<u64>(
                &args.run_timeout_secs,
                "--run-timeout-secs",
                "a positive integer",
            )?,
            max_sessions: parse_arg::<usize>(
                &args.max_sessions,
                "--max-sessions",
                "a positive integer",
            )?,
            api_key: args
                .api_key
                .or_else(|| std::env::var("AIKIT_SERVE_API_KEY").ok()),
            insecure: args.insecure,
        };
        serve::execute(serve_args)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))
    })?;

    // agent group — canonical entry point for all agent-related commands
    let agent_path = CommandPath::new(&["agent"])?;
    builder = builder.register_group(
        &agent_path,
        GroupMetadata {
            summary: "Agent run and management commands",
            hidden: false,
        },
    )?;

    builder = builder.register(path!["agent", "run"], |_ctx, args: RunArgs| async move {
        if args.agent.is_empty() {
            return Err(anyhow::anyhow!("--agent is required (e.g. --agent codex)"));
        }
        let run_args = run::RunArgs {
            agent: args.agent,
            model: args.model,
            prompt: args.prompt,
            yolo: args.yolo,
            stream: args.stream,
            events: args.events,
            progress: args.progress,
            dry_run: args.dry_run,
            session_agents: args.session_agents,
            session_persona: args.session_persona,
            resume: args.resume,
            resume_last: args.resume_last,
        };
        // run::execute is synchronous and creates its own tokio runtime internally
        // (via block_on_async in aikit-agent). Using spawn_blocking avoids a
        // "cannot start a runtime from within a runtime" panic.
        tokio::task::spawn_blocking(move || run::execute(run_args))
            .await
            .map_err(|e| anyhow::anyhow!("task join error: {}", e))?
    })?;

    builder = builder.register(
        path!["agent", "list"],
        |_ctx, args: AgentListArgs| async move {
            let agents_args = agents::AgentsArgs { json: args.json };
            agents::execute(agents_args).map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    builder = builder.register(
        path!["agent", "check"],
        |_ctx, _args: AgentCheckArgs| async move {
            check::execute_agent_check(check::AgentCheckArgs {})
                .map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    let agent_mcp_path = CommandPath::new(&["agent", "mcp"])?;
    builder = builder.register_group(
        &agent_mcp_path,
        GroupMetadata {
            summary: "MCP server management",
            hidden: false,
        },
    )?;

    builder = builder.register(
        path!["agent", "mcp", "list"],
        |_ctx, _args: McpListArgs| async move {
            mcp::execute_list().map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    // mcp add has a validator — construct Command manually and use register_command_at
    {
        let spec = Arc::new(McpAddArgs::command_spec());
        let id: Arc<str> = Arc::from("add");
        let mcp_add_cmd = Command {
            id,
            spec,
            validator: Some(Arc::new(|args| {
                let has_url = args.contains_key("url");
                let has_cmd = args.contains_key("command");
                if !has_url && !has_cmd {
                    return vec![Diagnostic {
                        code: "E_MCP_TRANSPORT_REQUIRED",
                        category: DiagnosticCategory::Validation,
                        message: "one of --url or --command is required".to_string(),
                        suggestion: Some(
                            "specify either --url <URL> for HTTP or --command <CMD> for stdio"
                                .to_string(),
                        ),
                        span: None,
                    }];
                }
                vec![]
            })),
            expose_mcp: false,
            expose_chat: false,
            execute: Arc::new(|_ctx, args| {
                Box::pin(async move {
                    let typed = McpAddArgs::from_arg_value_map(&args);
                    let add_args = mcp::McpAddArgs {
                        agent: typed.agent,
                        scope: typed.scope,
                        project: std::path::PathBuf::from(typed.project),
                        name: typed.name,
                        url: typed.url,
                        command: typed.command,
                        cmd_args: typed.arg,
                        env: typed.env,
                        header: typed.header,
                        overwrite: typed.overwrite,
                    };
                    mcp::execute_add(add_args).map_err(|e| anyhow::anyhow!("{}", e))
                })
            }),
        };
        let mcp_add_path = CommandPath::new(&["agent", "mcp", "add"])?;
        builder = builder.register_command_at(&mcp_add_path, mcp_add_cmd)?;
    }

    let package_path = CommandPath::new(&["package"])?;
    builder = builder.register_group(
        &package_path,
        GroupMetadata {
            summary: "Package management commands",
            hidden: false,
        },
    )?;

    builder = builder.register(
        path!["package", "init"],
        |_ctx, args: commands::package::PackageInitArgs| async move {
            commands::package::execute_init(args)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    builder = builder.register(
        path!["package", "validate"],
        |_ctx, args: commands::package::PackageValidateArgs| async move {
            commands::package::execute_validate(args)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    builder = builder.register(
        path!["package", "build"],
        |_ctx, args: commands::package::PackageBuildArgs| async move {
            commands::package::execute_build(args)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    builder = builder.register(
        path!["package", "publish"],
        |_ctx, args: commands::package::PackagePublishArgs| async move {
            commands::package::execute_publish(args)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    // ── session group ─────────────────────────────────────────────────────────
    let session_path = CommandPath::new(&["session"])?;
    builder = builder.register_group(
        &session_path,
        GroupMetadata {
            summary: "Interactive bidirectional agent sessions (multi-turn REPL)",
            hidden: false,
        },
    )?;

    builder = builder.register(
        path!["session", "new"],
        |_ctx, args: SessionNewArgs| async move {
            if args.agent.is_empty() {
                return Err(anyhow::anyhow!("--agent is required (e.g. --agent claude)"));
            }
            if args.prompt.is_empty() {
                return Err(anyhow::anyhow!("--prompt is required for session new"));
            }
            tokio::task::spawn_blocking(move || {
                session::execute_new(session::NewSessionArgs {
                    agent: args.agent,
                    prompt: args.prompt,
                    model: args.model,
                    approval_policy: args.approval_policy,
                    sandbox: args.sandbox,
                    events: args.events,
                })
            })
            .await
            .map_err(|e| anyhow::anyhow!("task join error: {}", e))?
        },
    )?;

    builder = builder.register(
        path!["session", "list"],
        |_ctx, args: SessionListArgs| async move {
            tokio::task::spawn_blocking(move || {
                session::execute_list(session::ListSessionsArgs {
                    serve_url: args.serve_url,
                })
            })
            .await
            .map_err(|e| anyhow::anyhow!("task join error: {}", e))?
        },
    )?;

    builder = builder.register(
        path!["session", "sync"],
        |_ctx, args: SessionSyncArgs| async move {
            let code = session::execute_sync(session::SyncSessionsArgs {
                bucket: args.bucket,
                endpoint: args.endpoint,
                region: args.region,
                owner: args.owner,
                key_prefix: args.key_prefix,
                tools: args.tool,
                watch: args.watch,
                dry_run: args.dry_run,
                allow_http: args.allow_http,
                format: args.format,
                log_level: args.log_level,
            })
            .await?;
            if code == 0 {
                Ok(())
            } else {
                std::process::exit(code);
            }
        },
    )?;

    #[cfg(all(feature = "agent-adapters", feature = "mcp-tools"))]
    {
        let conn = serve::storage::schema::open(&serve::capture_db_path())?;
        let event_store: Arc<dyn aikit_session_capture::EventStore> =
            Arc::new(serve::storage::SqliteEventStore::new(conn));
        builder = aikit_session_capture::mcp::try_register_commands(builder, event_store)?;
    }

    builder.build(AikitContext)
}

// ── helpers ───────────────────────────────────────────────────────────────────

pub(crate) fn flag_spec(name: &'static str, help: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        kind: ArgKind::Flag,
        short: None,
        long: Some(name),
        value_type: ArgValueType::Bool,
        cardinality: Cardinality::Optional,
        default: None,
        conflicts_with: vec![],
        requires: vec![],
        help,
        ..Default::default()
    }
}

pub(crate) fn opt_spec(name: &'static str, help: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        kind: ArgKind::Option,
        short: None,
        long: Some(name),
        value_type: ArgValueType::String,
        cardinality: Cardinality::Optional,
        default: None,
        conflicts_with: vec![],
        requires: vec![],
        help,
        ..Default::default()
    }
}

fn opt_short_spec(name: &'static str, short: char, help: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        kind: ArgKind::Option,
        short: Some(short),
        long: Some(name),
        value_type: ArgValueType::String,
        cardinality: Cardinality::Optional,
        default: None,
        conflicts_with: vec![],
        requires: vec![],
        help,
        ..Default::default()
    }
}

fn pos_opt_spec(name: &'static str, help: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        kind: ArgKind::Positional,
        short: None,
        long: None,
        value_type: ArgValueType::String,
        cardinality: Cardinality::Optional,
        default: None,
        conflicts_with: vec![],
        requires: vec![],
        help,
        ..Default::default()
    }
}

pub(crate) fn pos_req_spec(name: &'static str, help: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        kind: ArgKind::Positional,
        short: None,
        long: None,
        value_type: ArgValueType::String,
        cardinality: Cardinality::Required,
        default: None,
        conflicts_with: vec![],
        requires: vec![],
        help,
        ..Default::default()
    }
}

pub(crate) fn opt_str(v: &ArgValue) -> Option<String> {
    if let ArgValue::Str(s) = v {
        Some(s.clone())
    } else {
        None
    }
}

pub(crate) fn get_bool_val(map: &HashMap<String, ArgValue>, name: &str) -> bool {
    matches!(map.get(name), Some(ArgValue::Bool(true)))
}

pub(crate) fn get_opt_val(map: &HashMap<String, ArgValue>, name: &str) -> Option<String> {
    map.get(name).and_then(opt_str)
}

pub(crate) fn get_str_val(map: &HashMap<String, ArgValue>, name: &str) -> String {
    map.get(name)
        .and_then(opt_str)
        .unwrap_or_else(|| panic!("fw bug: missing required key '{}'", name))
}

pub(crate) fn get_str_default(
    map: &HashMap<String, ArgValue>,
    name: &str,
    default: &str,
) -> String {
    map.get(name)
        .and_then(opt_str)
        .unwrap_or_else(|| default.to_string())
}

fn get_repeated_val(map: &HashMap<String, ArgValue>, name: &str) -> Vec<String> {
    match map.get(name) {
        Some(ArgValue::List(items)) => items
            .iter()
            .filter_map(|i| {
                if let ArgValue::Str(s) = i {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect(),
        _ => vec![],
    }
}

// ── typed arg structs ─────────────────────────────────────────────────────────

// ── check ─────────────────────────────────────────────────────────────────────

struct CheckArgs;

impl IntoCommandSpec for CheckArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Check installed tools and AI agent CLIs",
            syntax: Some("check"),
            category: Some("diagnostics"),
            args: vec![],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for CheckArgs {
    fn from_arg_value_map(_map: &HashMap<String, ArgValue>) -> Self {
        CheckArgs
    }
}

// ── init ──────────────────────────────────────────────────────────────────────

struct InitArgs {
    project_name: Option<String>,
    ai: Option<String>,
    script: Option<String>,
    here: bool,
    force: bool,
    no_git: bool,
    github_token: Option<String>,
    skip_tls: bool,
    ignore_agent_tools: bool,
}

impl IntoCommandSpec for InitArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Initialize a new Spec-Driven Development project",
            syntax: Some("init [PROJECT_NAME]"),
            category: Some("setup"),
            args: vec![
                pos_opt_spec(
                    "project_name",
                    "Project directory to create (use '.' for current dir)",
                ),
                ArgSpec {
                    name: "ai",
                    short: None,
                    long: Some("ai"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "AI assistant to use (e.g., claude, gemini, copilot)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "script",
                    short: None,
                    long: Some("script"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Script type: sh or ps",
                    ..Default::default()
                },
                flag_spec("here", "Initialize in current directory"),
                flag_spec(
                    "force",
                    "Skip confirmation when merging into non-empty directory",
                ),
                ArgSpec {
                    name: "no-git",
                    short: None,
                    long: Some("no-git"),
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Skip Git repository initialization",
                    ..Default::default()
                },
                ArgSpec {
                    name: "github-token",
                    short: None,
                    long: Some("github-token"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "GitHub personal access token",
                    ..Default::default()
                },
                ArgSpec {
                    name: "skip-tls",
                    short: None,
                    long: Some("skip-tls"),
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Skip TLS certificate verification",
                    ..Default::default()
                },
                ArgSpec {
                    name: "ignore-agent-tools",
                    short: None,
                    long: Some("ignore-agent-tools"),
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Skip CLI tool validation for selected agent",
                    ..Default::default()
                },
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for InitArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        InitArgs {
            project_name: get_opt_val(map, "project_name"),
            ai: get_opt_val(map, "ai"),
            script: get_opt_val(map, "script"),
            here: get_bool_val(map, "here"),
            force: get_bool_val(map, "force"),
            no_git: get_bool_val(map, "no-git"),
            github_token: get_opt_val(map, "github-token"),
            skip_tls: get_bool_val(map, "skip-tls"),
            ignore_agent_tools: get_bool_val(map, "ignore-agent-tools"),
        }
    }
}

// ── release ───────────────────────────────────────────────────────────────────

struct ReleaseArgs {
    release_version: String,
    notes_file: String,
    github_token: Option<String>,
}

impl IntoCommandSpec for ReleaseArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Create GitHub release with package files",
            syntax: Some("release <VERSION>"),
            category: Some("deployment"),
            args: vec![
                pos_req_spec("VERSION", "Version string with 'v' prefix (e.g., v1.0.0)"),
                ArgSpec {
                    name: "notes-file",
                    short: None,
                    long: Some("notes-file"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Str("release_notes.md".to_string())),
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to release notes file",
                    ..Default::default()
                },
                ArgSpec {
                    name: "github-token",
                    short: None,
                    long: Some("github-token"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "GitHub token for API requests",
                    ..Default::default()
                },
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for ReleaseArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        ReleaseArgs {
            release_version: get_str_val(map, "VERSION"),
            notes_file: get_str_default(map, "notes-file", "release_notes.md"),
            github_token: get_opt_val(map, "github-token"),
        }
    }
}

// ── serve ─────────────────────────────────────────────────────────────────────

struct ServeArgs {
    host: String,
    port: String,
    run_timeout_secs: String,
    max_sessions: String,
    api_key: Option<String>,
    insecure: bool,
}

impl IntoCommandSpec for ServeArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Start HTTP server for multi-turn agent sessions (SSE streaming)",
            syntax: Some("serve"),
            category: Some("agents"),
            long_about: Some(
                "aikit serve exposes an in-process coding agent over HTTP. The agent keeps \
                 its full default toolset (including shell execution) — protecting the HOST \
                 from the agent is the job of the container/sandbox you run aikit serve in, \
                 not aikit itself. Because the agent is deliberately unconstrained \
                 internally, the network perimeter is the only in-app control: a bind to a \
                 non-loopback address REQUIRES --api-key (or --insecure to explicitly \
                 override). Deployment contract: run aikit serve inside a disposable, \
                 network-isolated sandbox; never expose it directly to an untrusted network.",
            ),
            args: vec![
                opt_spec("host", "Bind address (default: 127.0.0.1)"),
                opt_spec("port", "Port to listen on (default: 8787)"),
                opt_spec(
                    "run-timeout-secs",
                    "Cancel agent runs exceeding N seconds (default: 300)",
                ),
                opt_spec("max-sessions", "Max concurrent sessions (default: 10)"),
                opt_spec(
                    "api-key",
                    "Require Authorization: Bearer <key>. Also reads AIKIT_SERVE_API_KEY. \
                     Mandatory for a non-loopback --host unless --insecure is set.",
                ),
                flag_spec(
                    "insecure",
                    "Allow a non-loopback --host with no --api-key. Only the network \
                     perimeter guards this server (see ADR 0012) — do not set this outside \
                     a disposable, network-isolated sandbox.",
                ),
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for ServeArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        ServeArgs {
            host: get_str_default(map, "host", "127.0.0.1"),
            port: get_str_default(map, "port", "8787"),
            run_timeout_secs: get_str_default(map, "run-timeout-secs", "300"),
            max_sessions: get_str_default(map, "max-sessions", "10"),
            api_key: get_opt_val(map, "api-key"),
            insecure: get_bool_val(map, "insecure"),
        }
    }
}

// ── agent run ─────────────────────────────────────────────────────────────────

struct RunArgs {
    agent: String,
    model: Option<String>,
    prompt: Option<String>,
    yolo: bool,
    stream: bool,
    events: bool,
    progress: bool,
    dry_run: bool,
    session_agents: Option<String>,
    session_persona: Option<String>,
    resume: Option<String>,
    resume_last: bool,
}

impl IntoCommandSpec for RunArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Run a coding agent with a prompt (stdin or -p)",
            syntax: Some("run --agent <AGENT> [--prompt <TEXT>]"),
            category: Some("agents"),
            args: vec![
                ArgSpec {
                    name: "agent",
                    short: Some('a'),
                    long: Some("agent"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help:
                        "Runnable agent key (e.g. codex, claude, gemini, opencode, auto) [required]",
                    ..Default::default()
                },
                opt_short_spec("model", 'm', "Model passed to the agent"),
                opt_short_spec(
                    "prompt",
                    'p',
                    "Prompt to run (if omitted, reads from stdin)",
                ),
                flag_spec("yolo", "Run in yolo mode (auto-confirm, skip checks)"),
                flag_spec("stream", "Enable streaming output"),
                flag_spec("events", "Emit standardized NDJSON event stream to stdout"),
                ArgSpec {
                    name: "progress",
                    short: None,
                    long: Some("progress"),
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec!["events"],
                    requires: vec![],
                    help: "Display live human-readable progress on stderr",
                    ..Default::default()
                },
                ArgSpec {
                    name: "dry-run",
                    short: None,
                    long: Some("dry-run"),
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Validate inputs but don't execute agent",
                    ..Default::default()
                },
                ArgSpec {
                    name: "session-agents",
                    short: None,
                    long: Some("session-agents"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Session-scoped agent definitions: inline JSON or @<path>",
                    ..Default::default()
                },
                ArgSpec {
                    name: "session-persona",
                    short: None,
                    long: Some("session-persona"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Session persona: name of a definition to apply as main-thread defaults",
                    ..Default::default()
                },
                opt_short_spec("resume", 'r', "Resume session with the given session ID"),
                ArgSpec {
                    name: "resume-last",
                    short: None,
                    long: Some("resume-last"),
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec!["resume"],
                    requires: vec![],
                    help: "Resume the most recent session for the current directory",
                    ..Default::default()
                },
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for RunArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        RunArgs {
            agent: get_str_default(map, "agent", ""),
            model: get_opt_val(map, "model"),
            prompt: get_opt_val(map, "prompt"),
            yolo: get_bool_val(map, "yolo"),
            stream: get_bool_val(map, "stream"),
            events: get_bool_val(map, "events"),
            progress: get_bool_val(map, "progress"),
            dry_run: get_bool_val(map, "dry-run"),
            session_agents: get_opt_val(map, "session-agents"),
            session_persona: get_opt_val(map, "session-persona"),
            resume: get_opt_val(map, "resume"),
            resume_last: get_bool_val(map, "resume-last"),
        }
    }
}

// ── agent list ────────────────────────────────────────────────────────────────

struct AgentListArgs {
    json: bool,
}

impl IntoCommandSpec for AgentListArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "List persisted agent definitions",
            syntax: Some("agent list [--json]"),
            category: Some("agents"),
            args: vec![flag_spec(
                "json",
                "Emit JSON array to stdout instead of a table",
            )],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for AgentListArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        AgentListArgs {
            json: get_bool_val(map, "json"),
        }
    }
}

// ── agent check ───────────────────────────────────────────────────────────────

struct AgentCheckArgs;

impl IntoCommandSpec for AgentCheckArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Check availability of AI agent CLIs",
            syntax: Some("agent check"),
            category: Some("agents"),
            args: vec![],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for AgentCheckArgs {
    fn from_arg_value_map(_map: &HashMap<String, ArgValue>) -> Self {
        AgentCheckArgs
    }
}

// ── mcp list ──────────────────────────────────────────────────────────────────

struct McpListArgs;

impl IntoCommandSpec for McpListArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Agents that support mcp add and their config paths",
            syntax: Some("mcp list"),
            category: Some("mcp"),
            args: vec![],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for McpListArgs {
    fn from_arg_value_map(_map: &HashMap<String, ArgValue>) -> Self {
        McpListArgs
    }
}

// ── mcp add ───────────────────────────────────────────────────────────────────

struct McpAddArgs {
    agent: String,
    scope: String,
    project: String,
    name: String,
    url: Option<String>,
    command: Option<String>,
    arg: Vec<String>,
    env: Vec<String>,
    header: Vec<String>,
    overwrite: bool,
}

impl IntoCommandSpec for McpAddArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Add or replace one MCP server entry",
            syntax: Some(
                "mcp add --agent <AGENT> --scope <SCOPE> --name <NAME> (--url <URL> | --command <CMD>)",
            ),
            category: Some("mcp"),
            args: vec![
                ArgSpec {
                    name: "agent",
                    short: None,
                    long: Some("agent"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Agent key (e.g. cursor, claude, gemini, copilot, codex)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "scope",
                    short: None,
                    long: Some("scope"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::Enum(vec!["project", "global"]),
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Scope: project or global",
                    ..Default::default()
                },
                ArgSpec {
                    name: "project",
                    short: None,
                    long: Some("project"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Str(".".to_string())),
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Project root when --scope project",
                    ..Default::default()
                },
                ArgSpec {
                    name: "name",
                    short: None,
                    long: Some("name"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "MCP server id inside mcpServers",
                    ..Default::default()
                },
                ArgSpec {
                    name: "url",
                    short: None,
                    long: Some("url"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec!["command"],
                    requires: vec![],
                    help: "Streamable HTTP / remote MCP URL (exclusive with --command)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "command",
                    short: None,
                    long: Some("command"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec!["url"],
                    requires: vec![],
                    help: "Executable for stdio MCP (exclusive with --url)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "arg",
                    short: None,
                    long: Some("arg"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "argv token for stdio MCP (repeat for each arg)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "env",
                    short: None,
                    long: Some("env"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "KEY=value for stdio env (repeat per variable)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "header",
                    short: None,
                    long: Some("header"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "KEY=value for HTTP headers (repeat per header)",
                    ..Default::default()
                },
                flag_spec("overwrite", "Replace an existing mcpServers entry"),
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for McpAddArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        McpAddArgs {
            agent: get_str_val(map, "agent"),
            scope: get_str_default(map, "scope", "project"),
            project: get_str_default(map, "project", "."),
            name: get_str_val(map, "name"),
            url: get_opt_val(map, "url"),
            command: get_opt_val(map, "command"),
            arg: get_repeated_val(map, "arg"),
            env: get_repeated_val(map, "env"),
            header: get_repeated_val(map, "header"),
            overwrite: get_bool_val(map, "overwrite"),
        }
    }
}

// ── session ───────────────────────────────────────────────────────────────────

struct SessionNewArgs {
    agent: String,
    prompt: String,
    model: Option<String>,
    approval_policy: Option<String>,
    sandbox: Option<String>,
    events: bool,
}

impl IntoCommandSpec for SessionNewArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Open a live bidirectional agent session (multi-turn REPL)",
            syntax: Some("session new --agent <AGENT> --prompt <TEXT>"),
            category: Some("agents"),
            args: vec![
                ArgSpec {
                    name: "agent",
                    short: Some('a'),
                    long: Some("agent"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Agent to connect to: 'claude' or 'codex'",
                    ..Default::default()
                },
                opt_short_spec("prompt", 'p', "Initial prompt to send"),
                opt_short_spec("model", 'm', "Model identifier (claude only)"),
                opt_spec(
                    "approval-policy",
                    "Codex approval policy: never|on-request|on-failure|untrusted",
                ),
                opt_spec("sandbox", "Codex sandbox mode"),
                flag_spec("events", "Print events as NDJSON instead of human-readable"),
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for SessionNewArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        SessionNewArgs {
            agent: get_str_default(map, "agent", "claude"),
            prompt: get_str_default(map, "prompt", ""),
            model: get_opt_val(map, "model"),
            approval_policy: get_opt_val(map, "approval-policy"),
            sandbox: get_opt_val(map, "sandbox"),
            events: get_bool_val(map, "events"),
        }
    }
}

struct SessionListArgs {
    serve_url: Option<String>,
}

impl IntoCommandSpec for SessionListArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "List active live sessions on a running aikit serve instance",
            syntax: Some("session list"),
            category: Some("agents"),
            args: vec![opt_spec(
                "serve-url",
                "URL of aikit serve (default: AIKIT_SERVE_URL or http://127.0.0.1:8080)",
            )],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for SessionListArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        SessionListArgs {
            serve_url: get_opt_val(map, "serve-url"),
        }
    }
}

struct SessionSyncArgs {
    bucket: Option<String>,
    endpoint: Option<String>,
    region: Option<String>,
    owner: Option<String>,
    key_prefix: Option<String>,
    tool: Vec<String>,
    watch: bool,
    dry_run: bool,
    allow_http: bool,
    format: String,
    log_level: Option<String>,
}

impl IntoCommandSpec for SessionSyncArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Sync raw scrubbed session transcripts to S3-compatible storage",
            syntax: Some("session sync --bucket <BUCKET> --endpoint <URL>"),
            category: Some("agents"),
            args: vec![
                opt_spec("bucket", "S3 bucket (or AIKIT_SYNC_BUCKET)"),
                opt_spec(
                    "endpoint",
                    "S3-compatible endpoint (or AIKIT_SYNC_ENDPOINT)",
                ),
                opt_spec("region", "S3 region (default: us-east-1)"),
                opt_spec("owner", "Owner prefix (or AIKIT_SYNC_OWNER)"),
                opt_spec("key-prefix", "Object key prefix (default: sessions/)"),
                ArgSpec {
                    name: "tool",
                    short: None,
                    long: Some("tool"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Repeated,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Tool to sync; repeatable: claude_code or codex",
                    ..Default::default()
                },
                flag_spec("watch", "Continuously watch for session changes"),
                flag_spec(
                    "dry-run",
                    "Detect, scrub, hash, and summarize without uploading",
                ),
                flag_spec("allow-http", "Allow plain HTTP endpoints for local MinIO"),
                opt_spec("format", "Output format: default or json"),
                opt_spec("log-level", "Log level (default: info or RUST_LOG)"),
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for SessionSyncArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        Self {
            bucket: get_opt_val(map, "bucket"),
            endpoint: get_opt_val(map, "endpoint"),
            region: get_opt_val(map, "region"),
            owner: get_opt_val(map, "owner"),
            key_prefix: get_opt_val(map, "key-prefix"),
            tool: get_repeated_val(map, "tool"),
            watch: get_bool_val(map, "watch"),
            dry_run: get_bool_val(map, "dry-run"),
            allow_http: get_bool_val(map, "allow-http"),
            format: get_str_default(map, "format", "default"),
            log_level: get_opt_val(map, "log-level"),
        }
    }
}
