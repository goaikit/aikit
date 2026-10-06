//! Native hook translation and one bounded application decision callback.
use super::{HookEvent, Installation, InstallationStatus, IntegrationError, IntegrationService};
use crate::runner::{AgentEventPayload, HookAction, HookPhase};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    time::{Duration, Instant},
};

pub type DecisionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Decision, HandlerError>> + Send + 'a>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Block { reason: String },
}

#[derive(Debug)]
pub struct HandlerError(pub String);
impl std::fmt::Display for HandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for HandlerError {}

/// Typed evidence for one invocation. `id` is a local invocation ID, not a
/// fabricated native deduplication key. Repeated Stop calls receive new IDs and
/// require fresh application validation, including when stop_hook_active is true.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookRequest {
    pub id: String,
    pub installation_id: String,
    pub session_id: String,
    pub prompt_id: Option<String>,
    pub agent_id: Option<String>,
    pub event: HookEvent,
    pub cwd: PathBuf,
    pub payload: AgentEventPayload,
    pub final_answer: Option<String>,
    pub stop_hook_active: bool,
}

pub trait HookHandler: Send + Sync {
    /// Called only for input admission, before-tool and completion proposals.
    /// Must cooperate with async cancellation: do blocking work on a worker and
    /// guard mutations by request identity/revision. A late result cannot Allow.
    fn decide<'a>(&'a self, request: &'a HookRequest) -> DecisionFuture<'a>;
}

/// Exact native process response. Thin entry points write only stdout to stdout,
/// diagnostics to stderr and use exit_code. Failure exits 2, never fail-open 1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub request_id: Option<String>,
}
impl HookResponse {
    fn failure() -> Self {
        Self {
            stdout: String::new(),
            stderr: "Integration validation unavailable; retry after restoring the local handler."
                .into(),
            exit_code: 2,
            request_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookRecord {
    pub cursor: u64,
    /// Absent on historical rows written before schema 3. Such rows remain
    /// readable but cannot establish a binding to the current installation.
    pub installation_revision: Option<String>,
    pub request: HookRequest,
    pub decision: Option<Decision>,
    /// Tool arguments/results are available to the callback but omitted from
    /// replay. Applications persist necessary derived evidence in their own store.
    pub tool_payload_omitted: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPage {
    pub records: Vec<HookRecord>,
    pub next_cursor: u64,
}

impl IntegrationService {
    /// Provider selection comes from a trusted installation receipt. Input cannot
    /// select a provider or another workspace. Observations are committed before
    /// callback execution; decisions commit before a successful native response.
    pub async fn handle_hook(
        &self,
        installation_id: &str,
        input: &[u8],
        handler: &dyn HookHandler,
    ) -> HookResponse {
        self.dispatch_hook(installation_id, input, handler)
            .await
            .unwrap_or_else(|_| HookResponse::failure())
    }

    async fn dispatch_hook(
        &self,
        installation_id: &str,
        input: &[u8],
        handler: &dyn HookHandler,
    ) -> Result<HookResponse, IntegrationError> {
        let started = Instant::now();
        let installation = self.installed(installation_id)?;
        let installation_revision = self.installation_revision(installation_id)?;
        let request = decode(&installation, input)?;
        let journal_body = serde_json::to_string(&journal_copy(&request))?;
        // Reserve response/commit time within the configured native process limit.
        let deadline = started
            + Duration::from_millis(u64::from(installation.spec.timeout_seconds) * 1000 - 250);
        {
            let connection = self.connection()?;
            connection.busy_timeout(Duration::from_millis(100))?;
            connection.execute("INSERT INTO hook_invocations(id,installation_id,session_id,body,installation_revision) VALUES (?1,?2,?3,?4,?5)", params![request.id, installation_id, request.session_id, journal_body, installation_revision])?;
        }
        let decision = if is_decision(request.event) {
            // A receipt proves what we installed, not what is still configured.
            // Reuse the installer's ownership/drift checks before invoking policy.
            // Observations remain durable even when policy cannot safely run.
            self.require_configured(&installation)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            let result = if remaining.is_zero() {
                None
            } else {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handler.decide(&request)
                })) {
                    Ok(mut callback) => {
                        let guarded = std::future::poll_fn(|cx| {
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                callback.as_mut().poll(cx)
                            }))
                            .unwrap_or_else(|_| {
                                std::task::Poll::Ready(Err(HandlerError(
                                    "application callback panicked".into(),
                                )))
                            })
                        });
                        tokio::time::timeout(remaining, guarded).await.ok()
                    }
                    Err(_) => Some(Err(HandlerError("application callback panicked".into()))),
                }
            };
            Some(match result {
                Some(Ok(Decision::Allow)) if Instant::now() < deadline => Decision::Allow,
                Some(Ok(Decision::Block { reason }))
                    if !reason.trim().is_empty() && reason.len() <= 8192 =>
                {
                    Decision::Block { reason }
                }
                _ => Decision::Block {
                    reason: "Application checks did not complete successfully before the deadline."
                        .into(),
                },
            })
        } else {
            None
        };
        if self.installed(installation_id)? != installation
            || self.installation_revision(installation_id)? != installation_revision
        {
            return Err(IntegrationError::Conflict(
                "installation changed during hook validation".into(),
            ));
        }
        if decision.is_some() {
            // The application can await arbitrary work. Never return its result
            // under hooks disabled, edited or removed during that callback.
            self.require_configured(&installation)?;
        }
        if let Some(decision) = &decision {
            let connection = self.connection()?;
            connection.busy_timeout(Duration::from_millis(100))?;
            // Append, never update an already visible cursor: a consumer may have
            // checkpointed the observation while the callback was still running.
            connection.execute("INSERT INTO hook_invocations(id,installation_id,session_id,body,decision,installation_revision) VALUES (?1,?2,?3,?4,?5,?6)", params![format!("{}:decision", request.id), installation_id, request.session_id, journal_body, serde_json::to_string(decision)?, installation_revision])?;
        }
        if matches!(decision, Some(Decision::Allow)) && Instant::now() >= deadline {
            return Err(IntegrationError::Invalid(
                "decision expired before delivery".into(),
            ));
        }
        let stdout = if installation.spec.agent_key == "cursor" {
            super::cursor::encode(request.event, decision.as_ref())?
        } else if installation.spec.agent_key == "pi" {
            super::pi::encode(decision.as_ref())?
        } else {
            encode(request.event, decision.as_ref())?
        }
        .to_string();
        Ok(HookResponse {
            stdout,
            stderr: String::new(),
            exit_code: 0,
            request_id: Some(request.id),
        })
    }

    fn require_configured(&self, expected: &Installation) -> Result<(), IntegrationError> {
        match self.installation_status(&expected.id)? {
            InstallationStatus::Configured { installation } if installation == *expected => Ok(()),
            _ => Err(IntegrationError::Conflict(
                "owned hook configuration changed; restore or reconfigure the installation".into(),
            )),
        }
    }

    /// Durable local append order, not native causal order. Records are retained
    /// without pruning in this schema, so no retention gap can be silently hidden.
    /// Observations and prepared decisions occupy separate immutable cursors.
    /// A null decision is an observation, never Allow. A stored decision does not
    /// prove the response reached the native process or that it completed.
    pub fn events(
        &self,
        installation_id: &str,
        after: u64,
        limit: u32,
    ) -> Result<HookPage, IntegrationError> {
        if limit == 0 || limit > 500 || after > i64::MAX as u64 {
            return Err(IntegrationError::Invalid(
                "event page requires a valid cursor and limit of 1-500".into(),
            ));
        }
        let connection = self.connection()?;
        let known: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM installations WHERE id=?1 UNION ALL SELECT 1 FROM hook_invocations WHERE installation_id=?1)", [installation_id], |r| r.get(0))?;
        if !known {
            return Err(IntegrationError::NotFound);
        }
        let maximum: i64 = connection.query_row(
            "SELECT coalesce(max(sequence),0) FROM hook_invocations",
            [],
            |r| r.get(0),
        )?;
        if after as i64 > maximum {
            return Err(IntegrationError::Invalid(
                "event cursor is ahead of this journal".into(),
            ));
        }
        let mut query = connection.prepare("SELECT sequence,body,decision,installation_revision FROM hook_invocations WHERE installation_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3")?;
        let rows = query.query_map(params![installation_id, after as i64, limit], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        let records = rows
            .map(|row| {
                let (cursor, body, decision, installation_revision) = row?;
                let request: HookRequest = serde_json::from_str(&body)?;
                Ok(HookRecord {
                    cursor: u64::try_from(cursor)
                        .map_err(|_| IntegrationError::Invalid("negative event cursor".into()))?,
                    installation_revision,
                    tool_payload_omitted: matches!(
                        request.event,
                        HookEvent::BeforeTool | HookEvent::AfterTool | HookEvent::ToolFailed
                    ),
                    request,
                    decision: decision
                        .map(|value| serde_json::from_str(&value))
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>, IntegrationError>>()?;
        let next_cursor = records.last().map(|r| r.cursor).unwrap_or(after);
        Ok(HookPage {
            records,
            next_cursor,
        })
    }
}

fn is_decision(event: HookEvent) -> bool {
    matches!(
        event,
        HookEvent::InputSubmitted | HookEvent::BeforeTool | HookEvent::CompletionProposed
    )
}

fn journal_copy(request: &HookRequest) -> HookRequest {
    let mut retained = request.clone();
    match &mut retained.payload {
        AgentEventPayload::ToolUse { input, .. } => *input = Value::Null,
        AgentEventPayload::ToolResult { output, .. } => *output = Value::Null,
        _ => {}
    }
    retained
}

fn decode(installation: &Installation, input: &[u8]) -> Result<HookRequest, IntegrationError> {
    if installation.spec.agent_key == "pi" {
        return super::pi::decode(installation, input);
    }
    if installation.spec.agent_key == "codex" {
        return super::codex::decode(installation, input);
    }
    if installation.spec.agent_key == "cursor" {
        return super::cursor::decode(installation, input);
    }
    if installation.spec.agent_key != "claude" {
        return Err(IntegrationError::Unsupported("native hook adapter".into()));
    }
    if input.len() > 1024 * 1024 {
        return Err(IntegrationError::Invalid("hook input exceeds 1 MiB".into()));
    }
    let value: Value = serde_json::from_slice(input)?;
    let event = installation
        .spec
        .events
        .iter()
        .find(|event| {
            value.get("hook_event_name").and_then(Value::as_str) == Some(event.claude_name())
        })
        .copied()
        .ok_or_else(|| {
            IntegrationError::Invalid("event is not registered by this installation".into())
        })?;
    let session_id = required_string(&value, "session_id", 256)?;
    let cwd = std::fs::canonicalize(required_string(&value, "cwd", 32768)?)?;
    if !cwd.starts_with(&installation.spec.workspace) {
        return Err(IntegrationError::Invalid(
            "hook workspace differs from installation".into(),
        ));
    }
    let prompt_id = optional_string(&value, "prompt_id", 256)?;
    let agent_id = optional_string(&value, "agent_id", 256)?;
    let payload = match event {
        HookEvent::SessionStarted => AgentEventPayload::SessionStarted {
            session_id: session_id.clone(),
        },
        HookEvent::BeforeTool => AgentEventPayload::ToolUse {
            call_id: required_string(&value, "tool_use_id", 256)?,
            tool_name: required_string(&value, "tool_name", 256)?,
            input: value
                .get("tool_input")
                .filter(|v| v.is_object())
                .cloned()
                .ok_or_else(|| IntegrationError::Invalid("missing tool input object".into()))?,
        },
        HookEvent::AfterTool | HookEvent::ToolFailed => AgentEventPayload::ToolResult {
            call_id: required_string(&value, "tool_use_id", 256)?,
            output: value
                .get(if event == HookEvent::ToolFailed {
                    "error"
                } else {
                    "tool_response"
                })
                .cloned()
                .ok_or_else(|| IntegrationError::Invalid("missing tool result".into()))?,
            is_error: event == HookEvent::ToolFailed,
            duration_ms: None,
            started_at_ms: None,
        },
        _ => AgentEventPayload::Hook {
            phase: if event == HookEvent::InputSubmitted {
                HookPhase::BeforeModel
            } else {
                HookPhase::RunEnd
            },
            hook_name: format!("external.{}", event.claude_name()),
            action: HookAction::Observed,
            // Raw paths, configuration and arbitrary extra fields are not journaled.
            payload: None,
        },
    };
    // StopFailure's similarly named field is an API error, not conversational
    // output. Neither that text nor arbitrary error details belong in replay.
    let final_answer = if event == HookEvent::CompletionProposed {
        optional_string(&value, "last_assistant_message", 512 * 1024)?
    } else {
        None
    };
    let stop_hook_active = match value.get("stop_hook_active") {
        None => false,
        Some(v) => v
            .as_bool()
            .ok_or_else(|| IntegrationError::Invalid("stop_hook_active must be boolean".into()))?,
    };
    Ok(HookRequest {
        id: uuid::Uuid::new_v4().to_string(),
        installation_id: installation.id.clone(),
        session_id,
        prompt_id,
        agent_id,
        event,
        cwd,
        payload,
        final_answer,
        stop_hook_active,
    })
}

pub(super) fn required_string(
    value: &Value,
    key: &str,
    max: usize,
) -> Result<String, IntegrationError> {
    optional_string(value, key, max)?
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| IntegrationError::Invalid(format!("missing {key}")))
}
pub(super) fn optional_string(
    value: &Value,
    key: &str,
    max: usize,
) -> Result<Option<String>, IntegrationError> {
    match value.get(key) {
        None => Ok(None),
        Some(Value::String(s)) if s.len() <= max && !s.contains('\0') => Ok(Some(s.clone())),
        _ => Err(IntegrationError::Invalid(format!("invalid {key}"))),
    }
}

fn encode(event: HookEvent, decision: Option<&Decision>) -> Result<Value, IntegrationError> {
    // Claude and Codex share these response shapes. Empty Allow leaves native
    // permissions intact; Block never sets continue:false on a Stop proposal.
    match decision {
        None | Some(Decision::Allow) => Ok(json!({})), // Never bypass native permission checks.
        Some(Decision::Block { reason }) => match event {
            HookEvent::BeforeTool => Ok(
                json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":reason}}),
            ),
            HookEvent::InputSubmitted | HookEvent::CompletionProposed => {
                Ok(json!({"decision":"block","reason":reason}))
            }
            _ => Err(IntegrationError::Unsupported(
                "this observation cannot block the native operation".into(),
            )),
        },
    }
}

#[cfg(test)]
#[path = "hooks_tests.rs"]
mod tests;
