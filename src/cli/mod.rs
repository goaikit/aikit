//! CLI command module — builds the cli-framework App for aikit.

mod agents;
mod check;
pub mod context;
mod init;
mod mcp;
mod release;
mod run;
pub mod serve;
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

    builder = builder.register(path!["install"], |_ctx, args: InstallArgs| async move {
        let install_args = commands::install::InstallArgs {
            source: args.source,
            install_version: args.install_version,
            token: args.token,
            force: args.force,
            yes: args.yes,
            ai: args.ai,
        };
        commands::install::execute_install(install_args)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))
    })?;

    builder = builder.register(path!["update"], |_ctx, args: UpdateArgs| async move {
        let update_args = commands::install::UpdateArgs {
            package: args.package,
            breaking: args.breaking,
        };
        commands::install::execute_update(update_args)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))
    })?;

    builder = builder.register(path!["remove"], |_ctx, args: RemoveArgs| async move {
        let remove_args = commands::install::RemoveArgs {
            package: args.package,
            force: args.force,
        };
        commands::install::execute_remove(remove_args)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))
    })?;

    builder = builder.register(path!["list"], |_ctx, args: ListArgs| async move {
        let list_args = commands::install::ListArgs {
            author: args.author,
            detailed: args.detailed,
        };
        commands::install::execute_list(list_args)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))
    })?;

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
            port: args
                .port
                .parse::<u16>()
                .map_err(|_| anyhow::anyhow!("--port must be a valid port number"))?,
            run_timeout_secs: args
                .run_timeout_secs
                .parse::<u64>()
                .map_err(|_| anyhow::anyhow!("--run-timeout-secs must be a positive integer"))?,
            max_sessions: args
                .max_sessions
                .parse::<usize>()
                .map_err(|_| anyhow::anyhow!("--max-sessions must be a positive integer"))?,
            api_key: args
                .api_key
                .or_else(|| std::env::var("AIKIT_SERVE_API_KEY").ok()),
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
        |_ctx, args: PackageInitArgs| async move {
            let pkg_args = commands::package::PackageInitArgs {
                name: args.name,
                description: args.description,
                package_version: args.package_version,
                author: args.author,
                yes: args.yes,
            };
            commands::package::execute_init(pkg_args)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    builder = builder.register(
        path!["package", "validate"],
        |_ctx, args: PackageValidateArgs| async move {
            let pkg_args = commands::package::PackageValidateArgs { path: args.path };
            commands::package::execute_validate(pkg_args)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    builder = builder.register(
        path!["package", "build"],
        |_ctx, args: PackageBuildArgs| async move {
            let pkg_args = commands::package::PackageBuildArgs {
                output: args.output,
                agents: args.agents,
                include_sources: args.include_sources,
            };
            commands::package::execute_build(pkg_args)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    builder = builder.register(
        path!["package", "publish"],
        |_ctx, args: PackagePublishArgs| async move {
            let pkg_args = commands::package::PackagePublishArgs {
                repo: args.repo,
                package: args.package,
                tag: args.tag,
                title: args.title,
                notes: args.notes,
                token: args.token,
                no_release: args.no_release,
            };
            commands::package::execute_publish(pkg_args)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
        },
    )?;

    builder.build(AikitContext)
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn flag_spec(name: &'static str, help: &'static str) -> ArgSpec {
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

fn opt_spec(name: &'static str, help: &'static str) -> ArgSpec {
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

fn pos_req_spec(name: &'static str, help: &'static str) -> ArgSpec {
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

fn opt_str(v: &ArgValue) -> Option<String> {
    if let ArgValue::Str(s) = v {
        Some(s.clone())
    } else {
        None
    }
}

fn get_bool_val(map: &HashMap<String, ArgValue>, name: &str) -> bool {
    matches!(map.get(name), Some(ArgValue::Bool(true)))
}

fn get_opt_val(map: &HashMap<String, ArgValue>, name: &str) -> Option<String> {
    map.get(name).and_then(opt_str)
}

fn get_str_val(map: &HashMap<String, ArgValue>, name: &str) -> String {
    map.get(name)
        .and_then(opt_str)
        .unwrap_or_else(|| panic!("fw bug: missing required key '{}'", name))
}

fn get_str_default(map: &HashMap<String, ArgValue>, name: &str, default: &str) -> String {
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

// ── install ───────────────────────────────────────────────────────────────────

struct InstallArgs {
    source: String,
    install_version: Option<String>,
    token: Option<String>,
    force: bool,
    yes: bool,
    ai: Option<String>,
}

impl IntoCommandSpec for InstallArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Install package from GitHub URL or local path",
            syntax: Some("install <SOURCE>"),
            category: Some("packages"),
            args: vec![
                pos_req_spec("source", "Package source (GitHub URL or local directory)"),
                ArgSpec {
                    name: "install-version",
                    short: Some('i'),
                    long: Some("install-version"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Specific version to install",
                    ..Default::default()
                },
                opt_spec("token", "GitHub token (or set GITHUB_TOKEN env var)"),
                flag_spec("force", "Force reinstall if already installed"),
                flag_spec("yes", "Skip .gitignore modification prompt"),
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
                    help: "AI agent to install for (e.g., claude, copilot)",
                    ..Default::default()
                },
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for InstallArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        InstallArgs {
            source: get_str_val(map, "source"),
            install_version: get_opt_val(map, "install-version"),
            token: get_opt_val(map, "token"),
            force: get_bool_val(map, "force"),
            yes: get_bool_val(map, "yes"),
            ai: get_opt_val(map, "ai"),
        }
    }
}

// ── update ────────────────────────────────────────────────────────────────────

struct UpdateArgs {
    package: String,
    breaking: bool,
}

impl IntoCommandSpec for UpdateArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Update installed package",
            syntax: Some("update <PACKAGE>"),
            category: Some("packages"),
            args: vec![
                pos_req_spec("package", "Package name to update"),
                flag_spec("breaking", "Allow breaking changes"),
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for UpdateArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        UpdateArgs {
            package: get_str_val(map, "package"),
            breaking: get_bool_val(map, "breaking"),
        }
    }
}

// ── remove ────────────────────────────────────────────────────────────────────

struct RemoveArgs {
    package: String,
    force: bool,
}

impl IntoCommandSpec for RemoveArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Remove installed package",
            syntax: Some("remove <PACKAGE>"),
            category: Some("packages"),
            args: vec![
                pos_req_spec("package", "Package name to remove"),
                flag_spec("force", "Force removal without confirmation"),
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for RemoveArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        RemoveArgs {
            package: get_str_val(map, "package"),
            force: get_bool_val(map, "force"),
        }
    }
}

// ── list ──────────────────────────────────────────────────────────────────────

struct ListArgs {
    author: Option<String>,
    detailed: bool,
}

impl IntoCommandSpec for ListArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "List installed packages",
            syntax: Some("list"),
            category: Some("packages"),
            args: vec![
                opt_spec("author", "Filter by author"),
                flag_spec("detailed", "Show detailed information"),
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for ListArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        ListArgs {
            author: get_opt_val(map, "author"),
            detailed: get_bool_val(map, "detailed"),
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
}

impl IntoCommandSpec for ServeArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Start HTTP server for multi-turn agent sessions (SSE streaming)",
            syntax: Some("serve"),
            category: Some("agents"),
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
                    "Require Authorization: Bearer <key>. Also reads AIKIT_SERVE_API_KEY.",
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
                    help: "Agent key (e.g. cursor-agent, claude, gemini, copilot, codex)",
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

// ── package init ──────────────────────────────────────────────────────────────

struct PackageInitArgs {
    name: String,
    description: Option<String>,
    package_version: String,
    author: Option<String>,
    yes: bool,
}

impl IntoCommandSpec for PackageInitArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Initialize a new package with aikit.toml",
            syntax: Some("package init <NAME>"),
            category: Some("packages"),
            args: vec![
                pos_req_spec("name", "Package name (required)"),
                ArgSpec {
                    name: "description",
                    short: Some('d'),
                    long: Some("description"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Package description",
                    ..Default::default()
                },
                ArgSpec {
                    name: "package-version",
                    short: Some('v'),
                    long: Some("package-version"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Str("0.1.0".to_string())),
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Package version (default: 0.1.0)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "author",
                    short: Some('a'),
                    long: Some("author"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Author name",
                    ..Default::default()
                },
                flag_spec("yes", "Skip interactive prompts"),
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for PackageInitArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        PackageInitArgs {
            name: get_str_val(map, "name"),
            description: get_opt_val(map, "description"),
            package_version: get_str_default(map, "package-version", "0.1.0"),
            author: get_opt_val(map, "author"),
            yes: get_bool_val(map, "yes"),
        }
    }
}

// ── package validate ──────────────────────────────────────────────────────────

struct PackageValidateArgs {
    path: String,
}

impl IntoCommandSpec for PackageValidateArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Validate package structure and that templates exist (install-ready)",
            syntax: Some("package validate [--path <DIR>]"),
            category: Some("packages"),
            args: vec![ArgSpec {
                name: "path",
                short: Some('p'),
                long: Some("path"),
                kind: ArgKind::Option,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                default: Some(ArgValue::Str(".".to_string())),
                conflicts_with: vec![],
                requires: vec![],
                help: "Package directory (default: current directory)",
                ..Default::default()
            }],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for PackageValidateArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        PackageValidateArgs {
            path: get_str_default(map, "path", "."),
        }
    }
}

// ── package build ─────────────────────────────────────────────────────────────

struct PackageBuildArgs {
    output: String,
    agents: Option<String>,
    include_sources: bool,
}

impl IntoCommandSpec for PackageBuildArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Build package for distribution",
            syntax: Some("package build [--output <DIR>]"),
            category: Some("packages"),
            args: vec![
                ArgSpec {
                    name: "output",
                    short: Some('o'),
                    long: Some("output"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(ArgValue::Str("dist".to_string())),
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Output directory (default: dist/)",
                    ..Default::default()
                },
                opt_spec("agents", "Target agents (comma-separated, default: all)"),
                ArgSpec {
                    name: "include-sources",
                    short: None,
                    long: Some("include-sources"),
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Include source files",
                    ..Default::default()
                },
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for PackageBuildArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        PackageBuildArgs {
            output: get_str_default(map, "output", "dist"),
            agents: get_opt_val(map, "agents"),
            include_sources: get_bool_val(map, "include-sources"),
        }
    }
}

// ── package publish ───────────────────────────────────────────────────────────

struct PackagePublishArgs {
    repo: String,
    package: Option<String>,
    tag: Option<String>,
    title: Option<String>,
    notes: Option<String>,
    token: Option<String>,
    no_release: bool,
}

impl IntoCommandSpec for PackagePublishArgs {
    fn command_spec() -> CommandSpec {
        CommandSpec {
            summary: "Publish package to registry",
            syntax: Some("package publish <REPO>"),
            category: Some("packages"),
            args: vec![
                pos_req_spec("repo", "Repository in format owner/repo (required)"),
                ArgSpec {
                    name: "package",
                    short: Some('p'),
                    long: Some("package"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to package ZIP file",
                    ..Default::default()
                },
                ArgSpec {
                    name: "tag",
                    short: Some('t'),
                    long: Some("tag"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Version tag for the release",
                    ..Default::default()
                },
                opt_spec("title", "Release title"),
                opt_spec("notes", "Release notes"),
                opt_spec("token", "GitHub token (or set GITHUB_TOKEN env var)"),
                ArgSpec {
                    name: "no-release",
                    short: None,
                    long: Some("no-release"),
                    kind: ArgKind::Flag,
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Don't create a release, just upload to existing release",
                    ..Default::default()
                },
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for PackagePublishArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        PackagePublishArgs {
            repo: get_str_val(map, "repo"),
            package: get_opt_val(map, "package"),
            tag: get_opt_val(map, "tag"),
            title: get_opt_val(map, "title"),
            notes: get_opt_val(map, "notes"),
            token: get_opt_val(map, "token"),
            no_release: get_bool_val(map, "no-release"),
        }
    }
}
