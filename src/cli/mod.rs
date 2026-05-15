//! CLI command module — builds the cli-framework App for aikit.

mod agents;
mod check;
pub mod context;
mod init;
mod mcp;
mod release;
mod run;
mod template_package;
mod version;

pub mod commands {
    pub mod install;
    pub mod package;
}

use std::sync::Arc;

use anyhow::Result;
use cli_framework::app::{AppBuilder, AppMeta};
use cli_framework::command::{Command, CommandArgs};
use cli_framework::parser::diagnostic::{Diagnostic, DiagnosticCategory};
use cli_framework::spec::arg_spec::{ArgKind, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::GroupMetadata;
use cli_framework::spec::{ArgSpec, CommandPath, CommandSpec};

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

    builder = builder.register_command(cmd_check())?;
    builder = builder.register_command(cmd_init())?;
    builder = builder.register_command(cmd_install())?;
    builder = builder.register_command(cmd_update())?;
    builder = builder.register_command(cmd_remove())?;
    builder = builder.register_command(cmd_list())?;
    builder = builder.register_command(cmd_release())?;
    builder = builder.register_command(cmd_run())?;
    builder = builder.register_command(cmd_agents())?;

    let mcp_path = CommandPath::new(&["mcp"])?;
    builder = builder.register_group(
        &mcp_path,
        GroupMetadata {
            summary: "MCP server management",
            hidden: false,
        },
    )?;
    let mcp_list_path = CommandPath::new(&["mcp", "list"])?;
    let mcp_add_path = CommandPath::new(&["mcp", "add"])?;
    builder = builder.register_command_at(&mcp_list_path, cmd_mcp_list())?;
    builder = builder.register_command_at(&mcp_add_path, cmd_mcp_add())?;

    let package_path = CommandPath::new(&["package"])?;
    builder = builder.register_group(
        &package_path,
        GroupMetadata {
            summary: "Package management commands",
            hidden: false,
        },
    )?;
    let pkg_init_path = CommandPath::new(&["package", "init"])?;
    let pkg_validate_path = CommandPath::new(&["package", "validate"])?;
    let pkg_build_path = CommandPath::new(&["package", "build"])?;
    let pkg_publish_path = CommandPath::new(&["package", "publish"])?;
    builder = builder.register_command_at(&pkg_init_path, cmd_package_init())?;
    builder = builder.register_command_at(&pkg_validate_path, cmd_package_validate())?;
    builder = builder.register_command_at(&pkg_build_path, cmd_package_build())?;
    builder = builder.register_command_at(&pkg_publish_path, cmd_package_publish())?;

    builder.build(AikitContext)
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn flag(name: &'static str, help: &'static str) -> ArgSpec {
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
    }
}

fn opt(name: &'static str, help: &'static str) -> ArgSpec {
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
    }
}

fn opt_short(name: &'static str, short: char, help: &'static str) -> ArgSpec {
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
    }
}

fn pos_opt(name: &'static str, help: &'static str) -> ArgSpec {
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
    }
}

fn pos_req(name: &'static str, help: &'static str) -> ArgSpec {
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
    }
}

fn get_bool(args: &CommandArgs, name: &str) -> bool {
    args.named.get(name).map(|v| v == "true").unwrap_or(false)
}

fn get_opt(args: &CommandArgs, name: &str) -> Option<String> {
    args.named.get(name).cloned()
}

fn get_str(args: &CommandArgs, name: &str, default: &str) -> String {
    args.named
        .get(name)
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

fn split_repeated(args: &CommandArgs, name: &str) -> Vec<String> {
    args.named
        .get(name)
        .map(|s| {
            s.split(',')
                .map(str::to_string)
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

// ── commands ──────────────────────────────────────────────────────────────────

fn cmd_check() -> Command {
    Command {
        id: "check",
        summary: "Check installed tools and AI agent CLIs",
        syntax: Some("check"),
        category: Some("diagnostics"),
        spec: Some(Arc::new(CommandSpec {
            summary: "Check installed tools and AI agent CLIs",
            args: vec![],
            ..CommandSpec::default()
        })),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, _args| {
            Box::pin(async move {
                check::execute(check::CheckArgs {}).map_err(|e| anyhow::anyhow!("{}", e))
            })
        }),
    }
}

fn cmd_init() -> Command {
    Command {
        id: "init",
        summary: "Initialize a new Spec-Driven Development project",
        syntax: Some("init [PROJECT_NAME]"),
        category: Some("setup"),
        spec: Some(Arc::new(CommandSpec {
            summary: "Initialize a new Spec-Driven Development project",
            args: vec![
                pos_opt(
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
                },
                flag("here", "Initialize in current directory"),
                flag(
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
                },
            ],
            ..CommandSpec::default()
        })),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let init_args = init::InitArgs {
                    project_name: get_opt(&args, "project_name"),
                    ai: get_opt(&args, "ai"),
                    script: get_opt(&args, "script"),
                    here: get_bool(&args, "here"),
                    force: get_bool(&args, "force"),
                    no_git: get_bool(&args, "no-git"),
                    github_token: get_opt(&args, "github-token"),
                    skip_tls: get_bool(&args, "skip-tls"),
                    debug: false,
                    ignore_agent_tools: get_bool(&args, "ignore-agent-tools"),
                };
                init::execute(init_args)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
        }),
    }
}

fn cmd_install() -> Command {
    Command {
        id: "install",
        summary: "Install package from GitHub URL or local path",
        syntax: Some("install <SOURCE>"),
        category: Some("packages"),
        spec: Some(Arc::new(CommandSpec {
            summary: "Install package from GitHub URL or local path",
            args: vec![
                pos_req("source", "Package source (GitHub URL or local directory)"),
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
                },
                opt("token", "GitHub token (or set GITHUB_TOKEN env var)"),
                flag("force", "Force reinstall if already installed"),
                flag("yes", "Skip .gitignore modification prompt"),
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
                },
            ],
            ..CommandSpec::default()
        })),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let install_args = commands::install::InstallArgs {
                    source: get_str(&args, "source", ""),
                    install_version: get_opt(&args, "install-version"),
                    token: get_opt(&args, "token"),
                    force: get_bool(&args, "force"),
                    yes: get_bool(&args, "yes"),
                    ai: get_opt(&args, "ai"),
                };
                commands::install::execute_install(install_args)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
        }),
    }
}

fn cmd_update() -> Command {
    Command {
        id: "update",
        summary: "Update installed package",
        syntax: Some("update <PACKAGE>"),
        category: Some("packages"),
        spec: Some(Arc::new(CommandSpec {
            summary: "Update installed package",
            args: vec![
                pos_req("package", "Package name to update"),
                flag("breaking", "Allow breaking changes"),
            ],
            ..CommandSpec::default()
        })),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let update_args = commands::install::UpdateArgs {
                    package: get_str(&args, "package", ""),
                    breaking: get_bool(&args, "breaking"),
                };
                commands::install::execute_update(update_args)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
        }),
    }
}

fn cmd_remove() -> Command {
    Command {
        id: "remove",
        summary: "Remove installed package",
        syntax: Some("remove <PACKAGE>"),
        category: Some("packages"),
        spec: Some(Arc::new(CommandSpec {
            summary: "Remove installed package",
            args: vec![
                pos_req("package", "Package name to remove"),
                flag("force", "Force removal without confirmation"),
            ],
            ..CommandSpec::default()
        })),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let remove_args = commands::install::RemoveArgs {
                    package: get_str(&args, "package", ""),
                    force: get_bool(&args, "force"),
                };
                commands::install::execute_remove(remove_args)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
        }),
    }
}

fn cmd_list() -> Command {
    Command {
        id: "list",
        summary: "List installed packages",
        syntax: Some("list"),
        category: Some("packages"),
        spec: Some(Arc::new(CommandSpec {
            summary: "List installed packages",
            args: vec![
                opt("author", "Filter by author"),
                flag("detailed", "Show detailed information"),
            ],
            ..CommandSpec::default()
        })),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let list_args = commands::install::ListArgs {
                    author: get_opt(&args, "author"),
                    detailed: get_bool(&args, "detailed"),
                };
                commands::install::execute_list(list_args)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
        }),
    }
}

fn cmd_release() -> Command {
    Command {
        id: "release",
        summary: "Create GitHub release with package files",
        syntax: Some("release <VERSION>"),
        category: Some("deployment"),
        spec: Some(Arc::new(CommandSpec {
            summary: "Create GitHub release with package files",
            args: vec![
                pos_req("VERSION", "Version string with 'v' prefix (e.g., v1.0.0)"),
                ArgSpec {
                    name: "notes-file",
                    short: None,
                    long: Some("notes-file"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(cli_framework::spec::ArgValue::Str(
                        "release_notes.md".to_string(),
                    )),
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to release notes file",
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
                },
            ],
            ..CommandSpec::default()
        })),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let release_args = release::ReleaseArgs {
                    release_version: get_str(&args, "VERSION", ""),
                    notes_file: get_str(&args, "notes-file", "release_notes.md"),
                    github_token: get_opt(&args, "github-token"),
                };
                release::execute(release_args)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
        }),
    }
}

fn cmd_run() -> Command {
    Command {
        id: "run",
        summary: "Run a coding agent with a prompt",
        syntax: Some("run --agent <AGENT> [--prompt <TEXT>]"),
        category: Some("agents"),
        spec: Some(Arc::new(CommandSpec {
            summary: "Run a coding agent with a prompt (stdin or -p)",
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
                },
                opt_short("model", 'm', "Model passed to the agent"),
                opt_short(
                    "prompt",
                    'p',
                    "Prompt to run (if omitted, reads from stdin)",
                ),
                flag("yolo", "Run in yolo mode (auto-confirm, skip checks)"),
                flag("stream", "Enable streaming output"),
                flag("events", "Emit standardized NDJSON event stream to stdout"),
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
                },
                opt_short("resume", 'r', "Resume session with the given session ID"),
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
                },
            ],
            ..CommandSpec::default()
        })),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let agent = get_str(&args, "agent", "");
                if agent.is_empty() {
                    return Err(anyhow::anyhow!("--agent is required (e.g. --agent codex)"));
                }
                let run_args = run::RunArgs {
                    agent,
                    model: get_opt(&args, "model"),
                    prompt: get_opt(&args, "prompt"),
                    yolo: get_bool(&args, "yolo"),
                    stream: get_bool(&args, "stream"),
                    events: get_bool(&args, "events"),
                    progress: get_bool(&args, "progress"),
                    dry_run: get_bool(&args, "dry-run"),
                    session_agents: get_opt(&args, "session-agents"),
                    session_persona: get_opt(&args, "session-persona"),
                    resume: get_opt(&args, "resume"),
                    resume_last: get_bool(&args, "resume-last"),
                };
                // run::execute is synchronous and creates its own tokio runtime internally
                // (via block_on_async in aikit-agent). Using spawn_blocking avoids a
                // "cannot start a runtime from within a runtime" panic.
                tokio::task::spawn_blocking(move || run::execute(run_args))
                    .await
                    .map_err(|e| anyhow::anyhow!("task join error: {}", e))?
            })
        }),
    }
}

fn cmd_agents() -> Command {
    Command {
        id: "agents",
        summary: "List persisted agent definitions",
        syntax: Some("agents [--json]"),
        category: Some("agents"),
        spec: Some(Arc::new(CommandSpec {
            summary: "List persisted agent definitions",
            args: vec![flag("json", "Emit JSON array to stdout instead of a table")],
            ..CommandSpec::default()
        })),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let agents_args = agents::AgentsArgs {
                    json: get_bool(&args, "json"),
                };
                agents::execute(agents_args).map_err(|e| anyhow::anyhow!("{}", e))
            })
        }),
    }
}

fn cmd_mcp_list() -> Command {
    Command {
        id: "list",
        summary: "List agents that support mcp add and their config paths",
        syntax: Some("mcp list"),
        category: Some("mcp"),
        spec: Some(Arc::new(CommandSpec {
            summary: "Agents that support mcp add and their config paths",
            args: vec![],
            ..CommandSpec::default()
        })),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, _args| {
            Box::pin(async move { mcp::execute_list().map_err(|e| anyhow::anyhow!("{}", e)) })
        }),
    }
}

fn cmd_mcp_add() -> Command {
    Command {
        id: "add",
        summary: "Add or replace one MCP server entry in agent config files",
        syntax: Some(
            "mcp add --agent <AGENT> --scope <SCOPE> --name <NAME> (--url <URL> | --command <CMD>)",
        ),
        category: Some("mcp"),
        spec: Some(Arc::new(CommandSpec {
            summary: "Add or replace one MCP server entry",
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
                },
                ArgSpec {
                    name: "project",
                    short: None,
                    long: Some("project"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(cli_framework::spec::ArgValue::Str(".".to_string())),
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Project root when --scope project",
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
                },
                flag("overwrite", "Replace an existing mcpServers entry"),
            ],
            ..CommandSpec::default()
        })),
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
                let add_args = mcp::McpAddArgs {
                    agent: get_str(&args, "agent", ""),
                    scope: get_str(&args, "scope", "project"),
                    project: std::path::PathBuf::from(get_str(&args, "project", ".")),
                    name: get_str(&args, "name", ""),
                    url: get_opt(&args, "url"),
                    command: get_opt(&args, "command"),
                    cmd_args: split_repeated(&args, "arg"),
                    env: split_repeated(&args, "env"),
                    header: split_repeated(&args, "header"),
                    overwrite: get_bool(&args, "overwrite"),
                };
                mcp::execute_add(add_args).map_err(|e| anyhow::anyhow!("{}", e))
            })
        }),
    }
}

fn cmd_package_init() -> Command {
    Command {
        id: "init",
        summary: "Initialize a new package with aikit.toml",
        syntax: Some("package init <NAME>"),
        category: Some("packages"),
        spec: Some(Arc::new(CommandSpec {
            summary: "Initialize a new package with aikit.toml",
            args: vec![
                pos_req("name", "Package name (required)"),
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
                },
                ArgSpec {
                    name: "package-version",
                    short: Some('v'),
                    long: Some("package-version"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(cli_framework::spec::ArgValue::Str("0.1.0".to_string())),
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Package version (default: 0.1.0)",
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
                },
                flag("yes", "Skip interactive prompts"),
            ],
            ..CommandSpec::default()
        })),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let pkg_args = commands::package::PackageInitArgs {
                    name: get_str(&args, "name", ""),
                    description: get_opt(&args, "description"),
                    package_version: get_str(&args, "package-version", "0.1.0"),
                    author: get_opt(&args, "author"),
                    yes: get_bool(&args, "yes"),
                };
                commands::package::execute_init(pkg_args)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
        }),
    }
}

fn cmd_package_validate() -> Command {
    Command {
        id: "validate",
        summary: "Validate package structure and that templates exist (install-ready)",
        syntax: Some("package validate [--path <DIR>]"),
        category: Some("packages"),
        spec: Some(Arc::new(CommandSpec {
            summary: "Validate package structure and that templates exist (install-ready)",
            args: vec![ArgSpec {
                name: "path",
                short: Some('p'),
                long: Some("path"),
                kind: ArgKind::Option,
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                default: Some(cli_framework::spec::ArgValue::Str(".".to_string())),
                conflicts_with: vec![],
                requires: vec![],
                help: "Package directory (default: current directory)",
            }],
            ..CommandSpec::default()
        })),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let pkg_args = commands::package::PackageValidateArgs {
                    path: get_str(&args, "path", "."),
                };
                commands::package::execute_validate(pkg_args)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
        }),
    }
}

fn cmd_package_build() -> Command {
    Command {
        id: "build",
        summary: "Build package for distribution",
        syntax: Some("package build [--output <DIR>]"),
        category: Some("packages"),
        spec: Some(Arc::new(CommandSpec {
            summary: "Build package for distribution",
            args: vec![
                ArgSpec {
                    name: "output",
                    short: Some('o'),
                    long: Some("output"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: Some(cli_framework::spec::ArgValue::Str("dist".to_string())),
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Output directory (default: dist/)",
                },
                opt("agents", "Target agents (comma-separated, default: all)"),
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
                },
            ],
            ..CommandSpec::default()
        })),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let pkg_args = commands::package::PackageBuildArgs {
                    output: get_str(&args, "output", "dist"),
                    agents: get_opt(&args, "agents"),
                    include_sources: get_bool(&args, "include-sources"),
                };
                commands::package::execute_build(pkg_args)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
        }),
    }
}

fn cmd_package_publish() -> Command {
    Command {
        id: "publish",
        summary: "Publish package to registry",
        syntax: Some("package publish <REPO>"),
        category: Some("packages"),
        spec: Some(Arc::new(CommandSpec {
            summary: "Publish package to registry",
            args: vec![
                pos_req("repo", "Repository in format owner/repo (required)"),
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
                },
                opt("title", "Release title"),
                opt("notes", "Release notes"),
                opt("token", "GitHub token (or set GITHUB_TOKEN env var)"),
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
                },
            ],
            ..CommandSpec::default()
        })),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let pkg_args = commands::package::PackagePublishArgs {
                    repo: get_str(&args, "repo", ""),
                    package: get_opt(&args, "package"),
                    tag: get_opt(&args, "tag"),
                    title: get_opt(&args, "title"),
                    notes: get_opt(&args, "notes"),
                    token: get_opt(&args, "token"),
                    no_release: get_bool(&args, "no-release"),
                };
                commands::package::execute_publish(pkg_args)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
        }),
    }
}
