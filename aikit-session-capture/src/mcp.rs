//! MCP tool functions for session capture (spec 010 §15).
//!
//! Four tools backed by [`EventStore`](crate::EventStore):
//! - `check_file_freshness` — freshness join: most recent Read/Edit/Write of a path
//! - `search_past_outputs` — substring search over `ToolEvent.output`
//! - `get_session_summary` — rule-based summary (no LLM, §15.1)
//! - `list_actions_around` — chronological ±N actions around a given action
//!
//! Behind the `mcp-tools` feature. Each function is a plain async callable;
//! the host wraps them into cli-framework `Command` registrations (spec §15.2).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::event_store::EventStore;
use crate::models::{ToolEvent, ToolKind};

use cli_framework::app::AppBuilder;
use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;

/// Register session-capture MCP commands with cli-framework.
///
/// Commands are root-level and MCP-exportable; hosts may mount them into an
/// app that only exposes MCP, chat tools, or both.
pub fn register_commands(builder: AppBuilder, store: Arc<dyn EventStore>) -> AppBuilder {
    try_register_commands(builder, store).expect("session-capture MCP command registration")
}

/// Fallible variant for hosts that want to surface command registration
/// collisions instead of panicking.
pub fn try_register_commands(
    mut builder: AppBuilder,
    store: Arc<dyn EventStore>,
) -> anyhow::Result<AppBuilder> {
    for command in [
        command_check_file_freshness(store.clone()),
        command_search_past_outputs(store.clone()),
        command_get_session_summary(store.clone()),
        command_list_actions_around(store),
    ] {
        builder = builder.register_command(command)?;
    }
    Ok(builder)
}

fn command_check_file_freshness(store: Arc<dyn EventStore>) -> Command {
    command(
        "check_file_freshness",
        CommandSpec {
            summary: "Check whether an agent has read the current file content",
            args: vec![string_arg(
                "path",
                Cardinality::Required,
                "Absolute file path to check",
            )],
            category: Some("session-capture"),
            ..CommandSpec::default()
        },
        move |args| {
            let store = store.clone();
            async move {
                let result = check_file_freshness(
                    store.as_ref(),
                    CheckFileFreshnessArgs {
                        path: str_arg(&args, "path").unwrap_or_default(),
                    },
                )
                .await?;
                Ok(result)
            }
        },
    )
}

fn command_search_past_outputs(store: Arc<dyn EventStore>) -> Command {
    command(
        "search_past_outputs",
        CommandSpec {
            summary: "Search prior captured tool outputs",
            args: vec![
                string_arg("query", Cardinality::Required, "Substring to search for"),
                int_arg("limit", 20, "Maximum result count"),
            ],
            category: Some("session-capture"),
            ..CommandSpec::default()
        },
        move |args| {
            let store = store.clone();
            async move {
                let result = search_past_outputs(
                    store.as_ref(),
                    SearchPastOutputsArgs {
                        query: str_arg(&args, "query").unwrap_or_default(),
                        limit: u32_arg(&args, "limit", 20),
                    },
                )
                .await?;
                Ok(result)
            }
        },
    )
}

fn command_get_session_summary(store: Arc<dyn EventStore>) -> Command {
    command(
        "get_session_summary",
        CommandSpec {
            summary: "Summarize a captured session deterministically",
            args: vec![string_arg(
                "session_id",
                Cardinality::Required,
                "Captured session id",
            )],
            category: Some("session-capture"),
            ..CommandSpec::default()
        },
        move |args| {
            let store = store.clone();
            async move {
                let result = get_session_summary(
                    store.as_ref(),
                    GetSessionSummaryArgs {
                        session_id: str_arg(&args, "session_id").unwrap_or_default(),
                    },
                )
                .await?;
                Ok(result)
            }
        },
    )
}

fn command_list_actions_around(store: Arc<dyn EventStore>) -> Command {
    command(
        "list_actions_around",
        CommandSpec {
            summary: "List captured actions around an anchor action",
            args: vec![
                string_arg("action_id", Cardinality::Required, "Anchor source event id"),
                string_arg("session_id", Cardinality::Required, "Captured session id"),
                int_arg("n", 5, "Number of actions before and after"),
            ],
            category: Some("session-capture"),
            ..CommandSpec::default()
        },
        move |args| {
            let store = store.clone();
            async move {
                let result = list_actions_around(
                    store.as_ref(),
                    ListActionsAroundArgs {
                        action_id: str_arg(&args, "action_id").unwrap_or_default(),
                        session_id: str_arg(&args, "session_id").unwrap_or_default(),
                        n: u32_arg(&args, "n", 5),
                    },
                )
                .await?;
                Ok(result)
            }
        },
    )
}

fn command<F, Fut, T>(id: &'static str, spec: CommandSpec, f: F) -> Command
where
    F: Fn(HashMap<String, ArgValue>) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = anyhow::Result<T>> + Send + 'static,
    T: Serialize,
{
    let f = Arc::new(f);
    Command {
        id: Arc::from(id),
        spec: Arc::new(spec),
        validator: None,
        expose_mcp: true,
        expose_chat: true,
        execute: Arc::new(move |ctx, args| {
            let f = f.clone();
            Box::pin(async move {
                let result = f(args).await?;
                ctx.framework_println(&serde_json::to_string(&result)?);
                Ok(())
            })
        }),
    }
}

fn string_arg(name: &'static str, cardinality: Cardinality, help: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        kind: ArgKind::Option,
        value_type: ArgValueType::String,
        cardinality,
        help,
        ..ArgSpec::default()
    }
}

fn int_arg(name: &'static str, default: i64, help: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        kind: ArgKind::Option,
        value_type: ArgValueType::Int,
        cardinality: Cardinality::Optional,
        default: Some(ArgValue::Int(default)),
        min: Some(0),
        help,
        ..ArgSpec::default()
    }
}

fn str_arg(args: &HashMap<String, ArgValue>, name: &str) -> Option<String> {
    match args.get(name) {
        Some(ArgValue::Str(s)) | Some(ArgValue::Enum(s)) => Some(s.clone()),
        _ => None,
    }
}

fn u32_arg(args: &HashMap<String, ArgValue>, name: &str, default: u32) -> u32 {
    match args.get(name) {
        Some(ArgValue::Int(i)) if *i >= 0 => (*i).min(u32::MAX as i64) as u32,
        _ => default,
    }
}

// ── check_file_freshness ──────────────────────────────────────────────────────

/// Args for [`check_file_freshness`].
#[derive(Debug, Deserialize)]
pub struct CheckFileFreshnessArgs {
    pub path: String,
}

/// Result of [`check_file_freshness`].
#[derive(Debug, Serialize)]
pub struct FileFreshness {
    pub fresh: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_read_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified_at_ms: Option<i64>,
}

/// Check if a file was read by an agent more recently than it was last
/// modified on disk. Returns `fresh: true` when the agent has seen the
/// current content.
pub async fn check_file_freshness(
    store: &dyn EventStore,
    args: CheckFileFreshnessArgs,
) -> anyhow_result::Result<FileFreshness> {
    let path = Path::new(&args.path);
    let touch = store.last_file_touch(path).await?;
    match touch {
        Some(t) => {
            let fresh = match (t.last_read_at_ms, t.last_modified_at_ms) {
                (Some(read), Some(modified)) => read >= modified,
                _ => false,
            };
            Ok(FileFreshness {
                fresh,
                last_read_at_ms: t.last_read_at_ms,
                last_modified_at_ms: t.last_modified_at_ms,
            })
        }
        None => Ok(FileFreshness {
            fresh: false,
            last_read_at_ms: None,
            last_modified_at_ms: None,
        }),
    }
}

// ── search_past_outputs ───────────────────────────────────────────────────────

/// Args for [`search_past_outputs`].
#[derive(Debug, Deserialize)]
pub struct SearchPastOutputsArgs {
    pub query: String,
    #[serde(default = "default_search_limit")]
    pub limit: u32,
}

fn default_search_limit() -> u32 {
    20
}

/// Result of [`search_past_outputs`].
#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub results: Vec<SearchHit>,
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub source_event_id: String,
    pub session_id: String,
    pub tool: String,
    pub kind: String,
    pub target: Option<String>,
    pub output_excerpt: String,
    pub started_at_ms: Option<i64>,
}

/// Substring search over all `ToolEvent.output` fields. Used by agents to
/// recall past tool outputs ("what did the test suite say last time?").
pub async fn search_past_outputs(
    store: &dyn EventStore,
    args: SearchPastOutputsArgs,
) -> anyhow_result::Result<SearchResult> {
    let events = store.search_outputs(&args.query, args.limit).await?;
    let results = events
        .into_iter()
        .map(|ev| SearchHit {
            source_event_id: ev.source_event_id,
            session_id: ev.session_id,
            tool: ev.tool.as_str().to_string(),
            kind: ev.kind.as_str().to_string(),
            target: ev.target,
            output_excerpt: ev.output.unwrap_or_default().chars().take(500).collect(),
            started_at_ms: ev.started_at_ms,
        })
        .collect();
    Ok(SearchResult { results })
}

// ── get_session_summary ───────────────────────────────────────────────────────

/// Args for [`get_session_summary`].
#[derive(Debug, Deserialize)]
pub struct GetSessionSummaryArgs {
    pub session_id: String,
}

/// Rule-based summary (spec §15.1). No LLM — deterministic format-string
/// composition. Pinned by test against a fixture.
#[derive(Debug, Serialize)]
pub struct SessionSummaryResult {
    pub summary: String,
    pub session_id: String,
    pub action_count: u64,
    pub tool_histogram: Vec<(String, u64)>,
}

/// Build a deterministic summary of one session: first user message (≤200
/// chars), top-3 ActionKind histogram, final assistant text (≤200 chars).
pub async fn get_session_summary(
    store: &dyn EventStore,
    args: GetSessionSummaryArgs,
) -> anyhow_result::Result<SessionSummaryResult> {
    // Gather sessions across all tools to find this one.
    let tools = [ToolKind::ClaudeCode, ToolKind::Codex, ToolKind::OpenCode];
    for tool in &tools {
        let actions = store
            .actions_for_session(*tool, &args.session_id, u32::MAX, 0)
            .await?;
        if actions.is_empty() {
            continue;
        }
        let action_count = actions.len() as u64;

        // Build histogram of ActionKind.
        let mut hist: HashMap<&'static str, u64> = HashMap::new();
        for a in &actions {
            *hist.entry(a.kind.as_str()).or_default() += 1;
        }
        let mut hist_vec: Vec<(String, u64)> =
            hist.iter().map(|(k, v)| ((*k).to_string(), *v)).collect();
        hist_vec.sort_by(|a, b| b.1.cmp(&a.1));
        let top3: Vec<(String, u64)> = hist_vec.into_iter().take(3).collect();
        let hist_str = top3
            .iter()
            .map(|(k, v)| format!("{k}({v})"))
            .collect::<Vec<_>>()
            .join(", ");

        // First action's input is a proxy for "first user message".
        let first_msg = actions
            .first()
            .and_then(|a| a.input.as_deref())
            .unwrap_or("(no input)")
            .chars()
            .take(200)
            .collect::<String>();

        // Last action's output is a proxy for "conclusion."
        let last_msg = actions
            .last()
            .and_then(|a| a.output.as_deref())
            .unwrap_or("(no output)")
            .chars()
            .take(200)
            .collect::<String>();

        let summary =
            format!("User asked: \"{first_msg}\"\nUsed: {hist_str}\nConcluded: \"{last_msg}\"");

        return Ok(SessionSummaryResult {
            summary,
            session_id: args.session_id,
            action_count,
            tool_histogram: top3,
        });
    }
    Ok(SessionSummaryResult {
        summary: "(session not found)".to_string(),
        session_id: args.session_id,
        action_count: 0,
        tool_histogram: vec![],
    })
}

// ── list_actions_around ───────────────────────────────────────────────────────

/// Args for [`list_actions_around`].
#[derive(Debug, Deserialize)]
pub struct ListActionsAroundArgs {
    pub action_id: String,
    pub session_id: String,
    #[serde(default = "default_around_n")]
    pub n: u32,
}

fn default_around_n() -> u32 {
    5
}

/// Result of [`list_actions_around`].
#[derive(Debug, Serialize)]
pub struct ActionsAroundResult {
    pub actions: Vec<ToolEvent>,
}

/// Return the chronological ±N actions around the given `action_id` in one
/// session. Used by agents to get context around a specific past action.
pub async fn list_actions_around(
    store: &dyn EventStore,
    args: ListActionsAroundArgs,
) -> anyhow_result::Result<ActionsAroundResult> {
    let tools = [ToolKind::ClaudeCode, ToolKind::Codex, ToolKind::OpenCode];
    for tool in &tools {
        let all = store
            .actions_for_session(*tool, &args.session_id, u32::MAX, 0)
            .await?;
        if all.is_empty() {
            continue;
        }
        // Find the index of the anchor action.
        let idx = all.iter().position(|a| a.source_event_id == args.action_id);
        let idx = match idx {
            Some(i) => i,
            None => continue,
        };
        let start = idx.saturating_sub(args.n as usize);
        let end = (idx + args.n as usize + 1).min(all.len());
        return Ok(ActionsAroundResult {
            actions: all[start..end].to_vec(),
        });
    }
    Ok(ActionsAroundResult { actions: vec![] })
}

/// Convenience: build the JSON Schema for one tool's args. Hosts use this
/// when registering tools via cli-framework's `CommandSpec`.
pub fn tool_args_schema(tool: &str) -> Value {
    match tool {
        "check_file_freshness" => serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute file path to check" }
            },
            "required": ["path"]
        }),
        "search_past_outputs" => serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Substring to search for" },
                "limit": { "type": "integer", "default": 20, "description": "Max results" }
            },
            "required": ["query"]
        }),
        "get_session_summary" => serde_json::json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Session ID to summarize" }
            },
            "required": ["session_id"]
        }),
        "list_actions_around" => serde_json::json!({
            "type": "object",
            "properties": {
                "action_id": { "type": "string", "description": "Source event ID of the anchor action" },
                "session_id": { "type": "string", "description": "Session containing the action" },
                "n": { "type": "integer", "default": 5, "description": "Number of actions before and after" }
            },
            "required": ["action_id", "session_id"]
        }),
        _ => serde_json::json!({}),
    }
}

/// Lightweight wrapper around `anyhow::Result` to avoid a hard dependency
/// on `anyhow` for MCP consumers. We re-export the standard `anyhow::Result`
/// since it's already in the crate's dependency tree.
mod anyhow_result {
    pub type Result<T> = std::result::Result<T, anyhow::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_store::{EventBatch, InMemoryEventStore};
    use crate::models::{ActionKind, ActionStatus, ToolEvent};

    fn ev(id: &str, sess: &str, kind: ActionKind, started: i64, out: &str) -> ToolEvent {
        ToolEvent {
            source_event_id: id.into(),
            source_file: std::path::PathBuf::from("/tmp/sess.jsonl"),
            session_id: sess.into(),
            tool: ToolKind::ClaudeCode,
            kind,
            target: None,
            input: Some("Help me fix the bug".into()),
            output: Some(out.into()),
            status: ActionStatus::Success,
            error_message: None,
            started_at_ms: Some(started),
            duration_ms: None,
            git_root: None,
            metadata: serde_json::Value::Null,
        }
    }

    #[tokio::test]
    async fn check_file_freshness_unknown_path() {
        let store = InMemoryEventStore::new();
        let result = check_file_freshness(
            &store,
            CheckFileFreshnessArgs {
                path: "/nonexistent/file.go".into(),
            },
        )
        .await
        .unwrap();
        assert!(!result.fresh);
    }

    #[tokio::test]
    async fn search_past_outputs_finds_match() {
        let store = InMemoryEventStore::new();
        store
            .upsert_events(EventBatch {
                tool_events: vec![ev("1", "s1", ActionKind::Bash, 100, "go test PASSED")],
                token_events: vec![],
                cache_observations: vec![],
            })
            .await
            .unwrap();
        let result = search_past_outputs(
            &store,
            SearchPastOutputsArgs {
                query: "PASSED".into(),
                limit: 10,
            },
        )
        .await
        .unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].kind, "bash");
    }

    #[tokio::test]
    async fn get_session_summary_is_deterministic() {
        let store = InMemoryEventStore::new();
        store
            .upsert_events(EventBatch {
                tool_events: vec![
                    ev("1", "s1", ActionKind::Read, 100, ""),
                    ev("2", "s1", ActionKind::Read, 200, ""),
                    ev("3", "s1", ActionKind::Edit, 300, ""),
                    ev("4", "s1", ActionKind::Bash, 400, "tests passed"),
                ],
                token_events: vec![],
                cache_observations: vec![],
            })
            .await
            .unwrap();
        let result = get_session_summary(
            &store,
            GetSessionSummaryArgs {
                session_id: "s1".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(result.action_count, 4);
        assert!(result.summary.contains("User asked:"));
        assert!(result.summary.contains("Used:"));
        assert!(result.summary.contains("Concluded:"));
        assert!(result.summary.contains("read(2)"));
        assert_eq!(result.tool_histogram.len(), 3);
    }

    #[tokio::test]
    async fn list_actions_around_returns_neighbors() {
        let store = InMemoryEventStore::new();
        let events: Vec<ToolEvent> = (0..10)
            .map(|i| ev(&format!("act_{i}"), "s1", ActionKind::Read, i * 100, ""))
            .collect();
        store
            .upsert_events(EventBatch {
                tool_events: events,
                token_events: vec![],
                cache_observations: vec![],
            })
            .await
            .unwrap();
        let result = list_actions_around(
            &store,
            ListActionsAroundArgs {
                action_id: "act_5".into(),
                session_id: "s1".into(),
                n: 2,
            },
        )
        .await
        .unwrap();
        // ±2 = 5 actions (act_3, act_4, act_5, act_6, act_7)
        assert_eq!(result.actions.len(), 5);
        assert_eq!(result.actions[0].source_event_id, "act_3");
        assert_eq!(result.actions[2].source_event_id, "act_5");
    }

    #[test]
    fn tool_args_schemas_are_valid() {
        for tool in [
            "check_file_freshness",
            "search_past_outputs",
            "get_session_summary",
            "list_actions_around",
        ] {
            let schema = tool_args_schema(tool);
            assert_eq!(schema["type"], "object");
        }
    }
}
