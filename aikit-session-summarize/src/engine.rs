//! The summarizer: one native completion per session over a scrubbed
//! digest, validated tags, a stored brief, and bounded concurrency over a
//! batch (ADR 0023).
//!
//! Read-only toward the coding tools: the one thing this module writes is a
//! [`SessionBrief`], through a [`BriefStore`]. It never writes to a tool's
//! own session files.

use std::sync::Arc;
use std::time::{Duration, Instant};

use aikit_agent::llm::{LlmError, LlmGateway, LlmMessage, LlmRequest, LlmResponse};
use aikit_session_capture::{EventStore, SecretScrubber, SessionSummary, ToolKind};
use sha2::{Digest as _, Sha256};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::areas::{group_areas, AreaMapping};
use crate::brief::{BriefStore, SessionBrief, TagAssignment};
use crate::digest::{build_digest, DigestOptions, PromptsInput, RenderReport};
use crate::prompt::{corrective_message, parse_reply, user_message, ModelReply, SYSTEM_PROMPT};
use crate::tags::{mechanical_tags, validate_model_tags, Evidence, TagList, Validated};

/// What goes on the wire, minus nothing: the summarizer injects no more
/// than the one system message in [`SYSTEM_PROMPT`].
#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub model: String,
    pub base_url: String,
    pub api_key: String,
    pub max_tokens: u32,
    pub temperature: f64,
}

impl ModelConfig {
    pub fn new(
        model: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            model: model.into(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            // 4096, not 1024: a thinking model spends the budget on reasoning
            // before it writes the answer, and an exhausted budget arrives
            // as an empty reply.
            max_tokens: 4096,
            temperature: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SummarizeOptions {
    pub tags: TagList,
    pub areas: AreaMapping,
    pub digest: DigestOptions,
    /// Regenerate even when the stored brief has the same digest hash.
    pub force: bool,
    /// Build and return the digest; make no model call and store nothing.
    pub dry_run: bool,
    /// Concurrent model calls. Defaults to 1: many self-hosted gateways
    /// serve one request at a time and queue the rest, so more calls in
    /// flight only move the wait into the request timeout.
    pub parallel: usize,
    /// Retries of a transient transport failure (HTTP 429, any 5xx, a
    /// connection that failed before an answer) per model call. Client
    /// errors such as 401, 403 and 404 are never retried, and neither is a
    /// timeout: the server may still be working on that request.
    pub transport_retries: u32,
    /// First retry delay; doubles per retry, with up to 25% jitter added.
    pub backoff_base: Duration,
    /// Check endpoint, model and key with one tiny completion before the
    /// first model call of the batch, once. A failure fails every session
    /// that needed a call with the same error, without further calls, and is
    /// reported by [`Summarizer::preflight_error`]. Sessions that need no call
    /// (unchanged, dry run) never trigger it.
    pub preflight: bool,
}

impl Default for SummarizeOptions {
    fn default() -> Self {
        Self {
            tags: TagList::builtin(),
            areas: AreaMapping::default(),
            digest: DigestOptions::default(),
            force: false,
            dry_run: false,
            parallel: 1,
            transport_retries: 3,
            backoff_base: Duration::from_secs(1),
            preflight: false,
        }
    }
}

/// Supplies user prompts from outside the event store (the history reader).
pub trait PromptSource: Send + Sync {
    /// `Ok(None)`: this source has nothing for this tool, and the digest
    /// falls back to prompt events. `Err`: the source should have been able
    /// to answer and could not; the reason becomes a warning on the brief.
    /// Text returned here is scrubbed by the engine.
    fn user_prompts(&self, tool: ToolKind, session_id: &str)
        -> Result<Option<Vec<String>>, String>;
}

/// The result for one session.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// A brief was generated and stored.
    Generated {
        brief: SessionBrief,
    },
    /// The stored brief already has this digest hash; nothing was asked.
    Unchanged {
        brief: SessionBrief,
    },
    /// `dry_run`: the exact user message that would be sent, its hash, and
    /// what was degraded while building it.
    DryRun {
        user_message: String,
        mechanical: Vec<TagAssignment>,
        digest_hash: String,
        warnings: Vec<String>,
    },
    Failed {
        error: String,
    },
}

impl Outcome {
    /// Stable label: `generated`, `unchanged`, `dry_run` or `failed`.
    pub fn status(&self) -> &'static str {
        match self {
            Outcome::Generated { .. } => "generated",
            Outcome::Unchanged { .. } => "unchanged",
            Outcome::DryRun { .. } => "dry_run",
            Outcome::Failed { .. } => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionOutcome {
    pub tool: ToolKind,
    pub session_id: String,
    pub outcome: Outcome,
}

/// Called once per finished session, in completion order: sessions done so
/// far, the batch size, the outcome, and how long that session took once it
/// was dispatched.
pub type ProgressFn = dyn Fn(usize, usize, &SessionOutcome, Duration) + Send + Sync;

pub struct Summarizer {
    gateway: Arc<dyn LlmGateway>,
    model: ModelConfig,
    options: SummarizeOptions,
    prompts: Option<Arc<dyn PromptSource>>,
    scrubber: SecretScrubber,
    preflight_once: tokio::sync::OnceCell<Result<(), String>>,
}

/// SHA-256 hex of the exact user message: the identity of the request.
pub fn digest_hash(user_message: &str) -> String {
    hex::encode(Sha256::digest(user_message.as_bytes()))
}

/// Whether a gateway error is worth retrying: the server said to try later
/// (429) or failed (5xx), or the request failed without an answer (refused or
/// reset connection, unreadable body).
///
/// A timeout is not retried: the request reached the server, which may still
/// be working on it, and a server that handles one request at a time would
/// queue the resend behind it.
pub fn is_transient(err: &LlmError) -> bool {
    match err {
        LlmError::ErrorResponse { status, .. } => *status == 429 || (500..=599).contains(status),
        LlmError::RequestFailed { .. } => !err.is_timeout(),
        _ => false,
    }
}

/// A panicked or cancelled session task as a readable error.
fn join_error_message(err: tokio::task::JoinError) -> String {
    if err.is_panic() {
        let payload = err.into_panic();
        let text = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "no message".to_string());
        format!("internal error: summarizing this session panicked: {text}")
    } else {
        "internal error: the session task was cancelled".to_string()
    }
}

/// Delay before retry `attempt` (1-based): `base * 2^(attempt - 1)` plus up
/// to 25% jitter, so parallel callers do not retry in lockstep.
fn backoff_delay(base: Duration, attempt: u32) -> Duration {
    if base.is_zero() {
        return Duration::ZERO;
    }
    let exp = base.saturating_mul(1u32 << attempt.saturating_sub(1).min(6));
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    exp + exp.mul_f64(f64::from(nanos % 250) / 1000.0)
}

fn plural_retries(n: u32) -> &'static str {
    if n == 1 {
        "retry"
    } else {
        "retries"
    }
}

/// An empty reply that stopped for `length` is not a model that had nothing
/// to say: the budget ran out first. Thinking models spend it on reasoning,
/// so say what to raise instead of reporting "empty reply".
fn check_budget(resp: &LlmResponse) -> Result<(), String> {
    let empty = resp
        .content
        .as_deref()
        .map(|c| c.trim().is_empty())
        .unwrap_or(true);
    if empty && resp.finish_reason.as_deref() == Some("length") {
        return Err(
            "model hit the token budget before answering (finish_reason=length); \
             raise --max-tokens, thinking models spend it on reasoning first"
                .into(),
        );
    }
    Ok(())
}

/// One warning per trimmable digest section that did not show everything.
fn digest_warnings(report: &RenderReport) -> Vec<String> {
    [
        ("files", report.files_shown, report.files_total),
        ("commands", report.commands_shown, report.commands_total),
        ("areas", report.areas_shown, report.areas_total),
        ("assistant notes", report.notes_shown, report.notes_total),
    ]
    .into_iter()
    .filter(|(_, shown, total)| shown < total)
    .map(|(label, shown, total)| format!("digest: showed {shown} of {total} {label}"))
    .collect()
}

fn message(role: &str, text: String) -> LlmMessage {
    LlmMessage {
        role: role.to_string(),
        content: Some(text),
        tool_calls: None,
        tool_call_id: None,
    }
}

impl Summarizer {
    pub fn new(
        gateway: Arc<dyn LlmGateway>,
        model: ModelConfig,
        options: SummarizeOptions,
    ) -> Self {
        Self {
            gateway,
            model,
            options,
            prompts: None,
            scrubber: SecretScrubber::default(),
            preflight_once: tokio::sync::OnceCell::new(),
        }
    }

    pub fn with_prompt_source(mut self, source: Arc<dyn PromptSource>) -> Self {
        self.prompts = Some(source);
        self
    }

    pub fn with_scrubber(mut self, scrubber: SecretScrubber) -> Self {
        self.scrubber = scrubber;
        self
    }

    pub fn options(&self) -> &SummarizeOptions {
        &self.options
    }

    /// The error of the batch preflight, when [`SummarizeOptions::preflight`]
    /// ran it and it failed. Sessions that needed a model call failed with
    /// exactly this text.
    pub fn preflight_error(&self) -> Option<&str> {
        match self.preflight_once.get() {
            Some(Err(e)) => Some(e.as_str()),
            _ => None,
        }
    }

    fn request(&self, messages: Vec<LlmMessage>, max_tokens: u32) -> LlmRequest {
        LlmRequest {
            model: self.model.model.clone(),
            base_url: self.model.base_url.clone(),
            api_key: self.model.api_key.clone(),
            messages,
            tools: vec![],
            tool_choice: None,
            temperature: Some(self.model.temperature),
            top_p: None,
            max_tokens: Some(max_tokens),
            stream: false,
        }
    }

    /// One model call, retrying transient transport failures with backoff.
    /// A call that needed retries adds a note to `notes`. The gateway blocks
    /// on its own runtime, so it runs under `spawn_blocking` (the judge does
    /// the same).
    async fn complete(
        &self,
        messages: &[LlmMessage],
        max_tokens: u32,
        notes: &mut Vec<String>,
    ) -> Result<LlmResponse, String> {
        let mut retries = 0u32;
        loop {
            let gateway = Arc::clone(&self.gateway);
            let req = self.request(messages.to_vec(), max_tokens);
            let result = tokio::task::spawn_blocking(move || gateway.complete(req))
                .await
                .map_err(|e| format!("model task failed: {e}"))?;
            match result {
                Ok(resp) => {
                    if retries > 0 {
                        notes.push(format!(
                            "transport: answered after {retries} {}",
                            plural_retries(retries)
                        ));
                    }
                    return Ok(resp);
                }
                Err(err) if is_transient(&err) && retries < self.options.transport_retries => {
                    retries += 1;
                    tracing::warn!(
                        target: "aikit_session_summarize",
                        retry = retries,
                        "transient model error, retrying: {err}"
                    );
                    tokio::time::sleep(backoff_delay(self.options.backoff_base, retries)).await;
                }
                Err(err) if err.is_timeout() => {
                    return Err(format!(
                        "{err} (not retried: the server may still be working on it; \
                         raise --timeout for slow models)"
                    ))
                }
                Err(err) if retries > 0 => {
                    return Err(format!(
                        "{err} (gave up after {retries} {})",
                        plural_retries(retries)
                    ))
                }
                Err(err) => return Err(err.to_string()),
            }
        }
    }

    /// Check endpoint, model and key with one tiny completion before a
    /// batch, so a misconfiguration fails once instead of once per session.
    /// Any answer counts, an empty one included: only whether the call is
    /// accepted matters. Transient failures are retried like any call.
    pub async fn preflight(&self) -> Result<(), String> {
        let messages = [message("user", "Reply with OK.".to_string())];
        let mut notes = Vec::new();
        self.complete(&messages, 16, &mut notes)
            .await
            .map(|_| ())
            .map_err(|e| {
                format!(
                    "preflight call to model '{}' at {} failed: {e}",
                    self.model.model, self.model.base_url
                )
            })
    }

    /// Summarize one captured session: events come from `events`, the
    /// brief is looked up in and written to `briefs`.
    pub async fn summarize_one(
        &self,
        events: &dyn EventStore,
        briefs: &dyn BriefStore,
        session: &SessionSummary,
    ) -> SessionOutcome {
        let outcome = match self.summarize_inner(events, briefs, session).await {
            Ok(o) => o,
            Err(error) => Outcome::Failed { error },
        };
        SessionOutcome {
            tool: session.tool,
            session_id: session.session_id.clone(),
            outcome,
        }
    }

    async fn summarize_inner(
        &self,
        event_store: &dyn EventStore,
        briefs: &dyn BriefStore,
        session: &SessionSummary,
    ) -> Result<Outcome, String> {
        let events = event_store
            .actions_for_session(session.tool, &session.session_id, u32::MAX, 0)
            .await
            .map_err(|e| format!("reading events: {e}"))?;
        if events.is_empty() {
            return Err("no events captured for this session".into());
        }
        let git_root = session.git_root.as_deref();
        let mut warnings: Vec<String> = Vec::new();

        let prompts = match self
            .prompts
            .as_ref()
            .map(|p| p.user_prompts(session.tool, &session.session_id))
        {
            Some(Ok(Some(list))) => PromptsInput::History(
                list.iter()
                    .map(|p| self.scrubber.scrub(p))
                    .filter(|p| !p.trim().is_empty())
                    .collect(),
            ),
            Some(Err(reason)) => {
                warnings.push(format!("prompts: {}", self.scrubber.scrub(&reason)));
                PromptsInput::Events
            }
            Some(Ok(None)) | None => PromptsInput::Events,
        };

        let areas = group_areas(&events, git_root, &self.options.areas);
        let evidence = Evidence::from_events(&events, git_root);
        let mechanical = mechanical_tags(&self.options.tags, &evidence);
        let digest = build_digest(
            session,
            &events,
            prompts,
            areas.clone(),
            &self.options.digest,
        );
        if digest.prompts.is_empty() && !warnings.iter().any(|w| w.starts_with("prompts:")) {
            warnings.push(
                "prompts: none were available; the brief rests on files, commands and the final message"
                    .into(),
            );
        }
        let prompt_source = digest.prompt_source.map(str::to_string);
        let (rendered, report) = digest.render_with_report(&self.options.digest);
        warnings.extend(digest_warnings(&report));
        // Scrub the rendered digest once more: everything the model sees has
        // passed the scrubber, whichever path it arrived by.
        let digest_text = self.scrubber.scrub(&rendered);
        let user = user_message(&digest_text, &self.options.tags, &mechanical);
        let hash = digest_hash(&user);

        if self.options.dry_run {
            return Ok(Outcome::DryRun {
                user_message: user,
                mechanical,
                digest_hash: hash,
                warnings,
            });
        }

        if !self.options.force {
            if let Some(existing) = briefs
                .brief_for(session.tool, &session.session_id)
                .await
                .map_err(|e| format!("reading brief: {e}"))?
            {
                if existing.digest_hash == hash {
                    return Ok(Outcome::Unchanged { brief: existing });
                }
            }
        }

        if self.options.preflight {
            self.preflight_once
                .get_or_init(|| self.preflight())
                .await
                .clone()?;
        }

        let max_tokens = self.model.max_tokens;
        let mut messages = vec![
            message("system", SYSTEM_PROMPT.to_string()),
            message("user", user),
        ];
        let first = self.complete(&messages, max_tokens, &mut warnings).await?;
        check_budget(&first)?;
        let first_text = first.content.clone().unwrap_or_default();
        let mut model_reported = first.model.clone();

        let (reply, validated) = match self.check_reply(&first_text, &mechanical) {
            Ok((reply, validated)) if validated.rejected.is_empty() => (reply, validated),
            attempt => {
                // One corrective turn: the model's reply and the correction,
                // appended as turns.
                let (rejected, parse_error) = match &attempt {
                    Ok((_, v)) => (v.rejected.clone(), None),
                    Err(e) => (Vec::new(), Some(e.clone())),
                };
                warnings.push(match &parse_error {
                    Some(e) => format!("reply: asked again, the first reply was unusable ({e})"),
                    None => format!("reply: asked again, unknown tags {}", rejected.join(", ")),
                });
                messages.push(message("assistant", first_text.clone()));
                messages.push(message(
                    "user",
                    corrective_message(&rejected, parse_error.as_deref(), &self.options.tags),
                ));
                let second = self.complete(&messages, max_tokens, &mut warnings).await?;
                check_budget(&second)?;
                if second.model.is_some() {
                    model_reported = second.model.clone();
                }
                let second_text = second.content.unwrap_or_default();
                match self.check_reply(&second_text, &mechanical) {
                    Ok(pair) => pair,
                    Err(e) => match attempt {
                        // The first reply parsed but carried unknown tags:
                        // keep it, drop the unknown names.
                        Ok(pair) => pair,
                        Err(_) => return Err(format!("model reply unusable after retry: {e}")),
                    },
                }
            }
        };

        let mut tags = mechanical;
        tags.extend(validated.accepted);
        let brief = SessionBrief {
            tool: session.tool,
            session_id: session.session_id.clone(),
            summary: reply.summary,
            areas,
            tags,
            model: self.model.model.clone(),
            digest_hash: hash,
            generated_at_ms: chrono::Utc::now().timestamp_millis(),
            model_reported,
            rejected_tags: validated.rejected,
            prompt_source,
            warnings,
        };
        briefs
            .put_brief(&brief)
            .await
            .map_err(|e| format!("storing brief: {e}"))?;
        Ok(Outcome::Generated { brief })
    }

    fn check_reply(
        &self,
        text: &str,
        mechanical: &[TagAssignment],
    ) -> Result<(ModelReply, Validated), String> {
        let reply = parse_reply(text).map_err(|e| e.to_string())?;
        let validated = validate_model_tags(&self.options.tags, mechanical, &reply.tags);
        Ok((reply, validated))
    }

    /// Summarize a batch with at most `options.parallel` model calls in
    /// flight. Outcomes come back in the input order.
    pub async fn summarize_many(
        self: &Arc<Self>,
        events: Arc<dyn EventStore>,
        briefs: Arc<dyn BriefStore>,
        sessions: Vec<SessionSummary>,
    ) -> Vec<SessionOutcome> {
        self.summarize_many_with_progress(events, briefs, sessions, None)
            .await
    }

    /// [`Summarizer::summarize_many`], calling `progress` as each session
    /// finishes. Every brief is stored as soon as its session finishes, so an
    /// interrupted batch keeps what it completed.
    pub async fn summarize_many_with_progress(
        self: &Arc<Self>,
        events: Arc<dyn EventStore>,
        briefs: Arc<dyn BriefStore>,
        sessions: Vec<SessionSummary>,
        progress: Option<Arc<ProgressFn>>,
    ) -> Vec<SessionOutcome> {
        let total = sessions.len();
        let parallel = self.options.parallel.max(1);
        let sem = Arc::new(Semaphore::new(parallel));
        let mut set: JoinSet<(usize, SessionOutcome, Duration)> = JoinSet::new();
        for (i, session) in sessions.into_iter().enumerate() {
            let me = Arc::clone(self);
            let events = Arc::clone(&events);
            let briefs = Arc::clone(&briefs);
            let sem = Arc::clone(&sem);
            set.spawn(async move {
                let _permit = sem.acquire_owned().await.expect("semaphore open");
                let started = Instant::now();
                let (tool, session_id) = (session.tool, session.session_id.clone());
                // Its own task, so a panic fails this session, not the batch.
                let run = tokio::spawn(async move {
                    me.summarize_one(events.as_ref(), briefs.as_ref(), &session)
                        .await
                });
                let outcome = match run.await {
                    Ok(outcome) => outcome,
                    Err(e) => SessionOutcome {
                        tool,
                        session_id,
                        outcome: Outcome::Failed {
                            error: join_error_message(e),
                        },
                    },
                };
                (i, outcome, started.elapsed())
            });
        }
        let mut out: Vec<(usize, SessionOutcome)> = Vec::new();
        let mut done = 0usize;
        while let Some(res) = set.join_next().await {
            match res {
                Ok((i, outcome, took)) => {
                    done += 1;
                    if let Some(report) = &progress {
                        report(done, total, &outcome, took);
                    }
                    out.push((i, outcome));
                }
                Err(e) => tracing::warn!(target: "aikit_session_summarize", "task failed: {e}"),
            }
        }
        out.sort_by_key(|(i, _)| *i);
        out.into_iter().map(|(_, o)| o).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brief::{InMemoryBriefStore, TagSource};
    use aikit_agent::llm::mock::{MockGateway, MockResponse};
    use aikit_agent::llm::types::TIMED_OUT_PREFIX;
    use aikit_session_capture::{
        ActionKind, ActionStatus, EventBatch, InMemoryEventStore, ToolEvent,
    };
    use std::path::PathBuf;

    fn ev(id: &str, kind: ActionKind, target: &str, input: Option<&str>, at: i64) -> ToolEvent {
        ToolEvent {
            source_event_id: id.into(),
            source_file: PathBuf::from("/tmp/s.jsonl"),
            session_id: "sess".into(),
            tool: ToolKind::Codex,
            kind,
            target: Some(target.into()),
            input: input.map(str::to_string),
            output: None,
            status: ActionStatus::Success,
            error_message: None,
            started_at_ms: Some(at),
            duration_ms: None,
            git_root: Some(PathBuf::from("/repo")),
            metadata: serde_json::Value::Null,
        }
    }

    async fn store_with_session() -> (
        Arc<InMemoryEventStore>,
        Arc<InMemoryBriefStore>,
        SessionSummary,
    ) {
        let store = Arc::new(InMemoryEventStore::new());
        let mut prompt = ev(
            "p",
            ActionKind::Other,
            "add a flag",
            Some("add a --force flag"),
            1,
        );
        prompt.metadata = serde_json::json!({"kind": "user_prompt"});
        store
            .upsert_events(EventBatch {
                tool_events: vec![
                    prompt,
                    ev("r", ActionKind::Read, "/repo/src/cli.rs", None, 2),
                    ev("e", ActionKind::Edit, "/repo/src/cli.rs", None, 3),
                    ev("t", ActionKind::Think, "done", Some("Added the flag."), 4),
                ],
                ..Default::default()
            })
            .await
            .unwrap();
        let session = store
            .sessions_for(ToolKind::Codex, None, 10, 0)
            .await
            .unwrap()
            .remove(0);
        (store, Arc::new(InMemoryBriefStore::new()), session)
    }

    fn fast() -> SummarizeOptions {
        SummarizeOptions {
            backoff_base: Duration::ZERO,
            ..Default::default()
        }
    }

    fn summarizer(responses: Vec<MockResponse>, options: SummarizeOptions) -> Arc<Summarizer> {
        Arc::new(Summarizer::new(
            Arc::new(MockGateway::new(responses)),
            ModelConfig::new("mock-model", "http://mock", "k"),
            options,
        ))
    }

    const GOOD: &str = r#"{"summary": "ok", "tags": []}"#;

    fn status_error(status: u16) -> MockResponse {
        MockResponse::error(LlmError::ErrorResponse {
            status,
            url: "http://mock/chat/completions".into(),
            body: "{}".into(),
        })
    }

    #[tokio::test]
    async fn generates_stores_and_then_skips_unchanged() {
        let (store, briefs, session) = store_with_session().await;
        let reply = r#"{"summary": "Added a --force flag to the CLI.", "tags": [{"name": "feature", "why": "a new flag was added"}]}"#;
        let s = summarizer(vec![MockResponse::text(reply)], fast());

        let out = s
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        let brief = match &out.outcome {
            Outcome::Generated { brief } => brief.clone(),
            other => panic!("expected Generated, got {other:?}"),
        };
        assert_eq!(out.outcome.status(), "generated");
        assert_eq!(brief.summary, "Added a --force flag to the CLI.");
        assert_eq!(brief.tags.len(), 1);
        assert_eq!(brief.tags[0].name, "feature");
        assert_eq!(brief.tags[0].source, TagSource::Model);
        assert_eq!(brief.tags[0].justification, "a new flag was added");
        assert_eq!(brief.areas[0].area, "src");
        assert_eq!(brief.model, "mock-model");
        assert_eq!(brief.prompt_source.as_deref(), Some("events"));
        assert_eq!(brief.digest_hash.len(), 64);
        assert!(brief.warnings.is_empty(), "{:?}", brief.warnings);
        assert_eq!(
            briefs
                .brief_for(ToolKind::Codex, "sess")
                .await
                .unwrap()
                .as_ref(),
            Some(&brief)
        );

        // Second run: the mock queue is empty, and it must not be consulted.
        let again = s
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        assert!(matches!(again.outcome, Outcome::Unchanged { .. }));

        // Forced: the empty mock reply is an error, proving the call happened.
        let forced = summarizer(
            vec![],
            SummarizeOptions {
                force: true,
                ..fast()
            },
        );
        let f = forced
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        assert!(matches!(f.outcome, Outcome::Failed { .. }));
    }

    #[tokio::test]
    async fn dry_run_returns_the_message_and_calls_nothing() {
        let (store, briefs, session) = store_with_session().await;
        let s = summarizer(
            vec![],
            SummarizeOptions {
                dry_run: true,
                ..fast()
            },
        );
        let out = s
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        match out.outcome {
            Outcome::DryRun {
                user_message,
                mechanical,
                digest_hash,
                warnings,
            } => {
                assert!(user_message.contains("## Allowed tags"));
                assert!(user_message.contains("1. add a --force flag"));
                assert!(mechanical.is_empty());
                assert!(warnings.is_empty());
                assert_eq!(digest_hash, super::digest_hash(&user_message));
            }
            other => panic!("expected DryRun, got {other:?}"),
        }
        assert!(briefs
            .brief_for(ToolKind::Codex, "sess")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn unknown_tag_is_retried_once_then_recorded_as_rejected() {
        let (store, briefs, session) = store_with_session().await;
        let bad = r#"{"summary": "s", "tags": [{"name": "feature", "why": "x"}, {"name": "enhancement", "why": "y"}]}"#;
        let still_bad = r#"{"summary": "s2", "tags": [{"name": "feature", "why": "x"}, {"name": "enhancement", "why": "y"}]}"#;
        let s = summarizer(
            vec![MockResponse::text(bad), MockResponse::text(still_bad)],
            fast(),
        );
        let out = s
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        match out.outcome {
            Outcome::Generated { brief } => {
                assert_eq!(brief.summary, "s2", "the retry's reply is used");
                let names: Vec<&str> = brief.tags.iter().map(|t| t.name.as_str()).collect();
                assert_eq!(names, vec!["feature"]);
                assert_eq!(brief.rejected_tags, vec!["enhancement"]);
                assert!(brief
                    .warnings
                    .contains(&"reply: asked again, unknown tags enhancement".to_string()));
            }
            other => panic!("expected Generated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unparseable_reply_is_retried_and_a_good_second_reply_wins() {
        let (store, briefs, session) = store_with_session().await;
        let s = summarizer(
            vec![
                MockResponse::text("Sure! Here is prose without JSON."),
                MockResponse::text(GOOD),
            ],
            fast(),
        );
        let out = s
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        assert!(matches!(out.outcome, Outcome::Generated { ref brief } if brief.summary == "ok"));

        let twice_bad = summarizer(
            vec![MockResponse::text("no"), MockResponse::text("still no")],
            SummarizeOptions {
                force: true,
                ..fast()
            },
        );
        let out = twice_bad
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        assert!(matches!(out.outcome, Outcome::Failed { .. }));
    }

    #[tokio::test]
    async fn empty_reply_truncated_by_length_names_the_budget() {
        let (store, briefs, session) = store_with_session().await;
        let mut truncated = MockResponse::text("");
        truncated.finish_reason = "length".into();
        let s = summarizer(vec![truncated], fast());
        let out = s
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        match out.outcome {
            Outcome::Failed { error } => assert!(error.contains("--max-tokens"), "{error}"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn transient_classification() {
        let status = |status| LlmError::ErrorResponse {
            status,
            url: String::new(),
            body: String::new(),
        };
        assert!(is_transient(&status(429)));
        assert!(is_transient(&status(500)));
        assert!(is_transient(&status(503)));
        assert!(!is_transient(&status(400)));
        assert!(!is_transient(&status(401)));
        assert!(!is_transient(&status(404)));
        assert!(is_transient(&LlmError::RequestFailed {
            message: "connection refused".into()
        }));
        assert!(!is_transient(&LlmError::RequestFailed {
            message: format!("{TIMED_OUT_PREFIX}error sending request"),
        }));
        assert!(!is_transient(&LlmError::NoApiKey {
            checked: String::new()
        }));
    }

    #[test]
    fn backoff_doubles_with_bounded_jitter() {
        let base = Duration::from_millis(100);
        for attempt in 1..=4u32 {
            let d = backoff_delay(base, attempt);
            let floor = base * (1 << (attempt - 1));
            assert!(d >= floor && d <= floor.mul_f64(1.25), "{attempt}: {d:?}");
        }
        assert_eq!(backoff_delay(Duration::ZERO, 3), Duration::ZERO);
    }

    #[tokio::test]
    async fn transient_errors_are_retried_then_the_reply_is_used() {
        let (store, briefs, session) = store_with_session().await;
        let s = summarizer(
            vec![
                status_error(503),
                MockResponse::error(LlmError::RequestFailed {
                    message: "connection reset".into(),
                }),
                MockResponse::text(GOOD),
            ],
            fast(),
        );
        let out = s
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        match out.outcome {
            Outcome::Generated { brief } => assert!(
                brief
                    .warnings
                    .contains(&"transport: answered after 2 retries".to_string()),
                "{:?}",
                brief.warnings
            ),
            other => panic!("expected Generated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn client_errors_are_not_retried_and_retries_are_bounded() {
        let (store, briefs, session) = store_with_session().await;
        // 401 fails at once, even with a good reply queued behind it.
        let s = summarizer(vec![status_error(401), MockResponse::text(GOOD)], fast());
        let out = s
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        match out.outcome {
            Outcome::Failed { error } => {
                assert!(error.contains("401"), "{error}");
                assert!(!error.contains("gave up"), "{error}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }

        let bounded = summarizer(
            vec![
                status_error(503),
                status_error(503),
                status_error(503),
                MockResponse::text(GOOD),
            ],
            SummarizeOptions {
                transport_retries: 2,
                ..fast()
            },
        );
        let out = bounded
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        match out.outcome {
            Outcome::Failed { error } => {
                assert!(error.contains("gave up after 2 retries"), "{error}")
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn preflight_fails_on_client_errors_and_survives_transient_ones() {
        let bad = summarizer(vec![status_error(401)], fast());
        let err = bad.preflight().await.unwrap_err();
        assert!(
            err.contains("preflight") && err.contains("401") && err.contains("mock-model"),
            "{err}"
        );

        // A thinking model may answer the tiny call with nothing: still fine.
        let flaky = summarizer(vec![status_error(503), MockResponse::text("")], fast());
        assert!(flaky.preflight().await.is_ok());
    }

    struct FixedPrompts;
    impl PromptSource for FixedPrompts {
        fn user_prompts(&self, _tool: ToolKind, _id: &str) -> Result<Option<Vec<String>>, String> {
            Ok(Some(vec![
                "from history sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789".into(),
            ]))
        }
    }

    struct BrokenPrompts;
    impl PromptSource for BrokenPrompts {
        fn user_prompts(&self, _tool: ToolKind, _id: &str) -> Result<Option<Vec<String>>, String> {
            Err("the history reader found no such session".into())
        }
    }

    #[tokio::test]
    async fn a_failing_prompt_source_becomes_a_warning_and_events_are_used() {
        let (store, briefs, session) = store_with_session().await;
        let s = Arc::new(
            Summarizer::new(
                Arc::new(MockGateway::new(vec![])),
                ModelConfig::new("m", "http://mock", "k"),
                SummarizeOptions {
                    dry_run: true,
                    ..fast()
                },
            )
            .with_prompt_source(Arc::new(BrokenPrompts)),
        );
        match s
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await
            .outcome
        {
            Outcome::DryRun {
                user_message,
                warnings,
                ..
            } => {
                assert_eq!(
                    warnings,
                    vec!["prompts: the history reader found no such session".to_string()]
                );
                assert!(user_message.contains("1. add a --force flag"));
            }
            other => panic!("expected DryRun, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mechanical_tag_is_primary_and_history_prompts_are_scrubbed() {
        let store = Arc::new(InMemoryEventStore::new());
        store
            .upsert_events(EventBatch {
                tool_events: vec![
                    ev("r", ActionKind::Read, "/repo/tests/a_test.rs", None, 1),
                    ev("e", ActionKind::Edit, "/repo/tests/a_test.rs", None, 2),
                ],
                ..Default::default()
            })
            .await
            .unwrap();
        let session = store
            .sessions_for(ToolKind::Codex, None, 10, 0)
            .await
            .unwrap()
            .remove(0);
        let briefs = Arc::new(InMemoryBriefStore::new());
        let s = Arc::new(
            Summarizer::new(
                Arc::new(MockGateway::new(vec![MockResponse::text(
                    r#"{"summary": "Fixed a test.", "tags": [{"name": "bugfix", "why": "..."}]}"#,
                )])),
                ModelConfig::new("m", "http://mock", "k"),
                fast(),
            )
            .with_prompt_source(Arc::new(FixedPrompts)),
        );
        // Dry-run view of the same session to inspect the message.
        let dry = Arc::new(
            Summarizer::new(
                Arc::new(MockGateway::new(vec![])),
                ModelConfig::new("m", "http://mock", "k"),
                SummarizeOptions {
                    dry_run: true,
                    ..fast()
                },
            )
            .with_prompt_source(Arc::new(FixedPrompts)),
        );
        if let Outcome::DryRun {
            user_message,
            mechanical,
            ..
        } = dry
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await
            .outcome
        {
            assert!(user_message.contains("[REDACTED:anthropic_api_key]"));
            assert!(!user_message.contains("sk-ant-"));
            assert_eq!(mechanical[0].name, "test");
            assert!(user_message.contains("- test: only test files were modified"));
        } else {
            panic!("expected DryRun");
        }

        let out = s
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        match out.outcome {
            Outcome::Generated { brief } => {
                assert_eq!(brief.primary_tag(), Some("test"));
                assert_eq!(brief.tags[0].source, TagSource::Mechanical);
                assert_eq!(brief.tags[1].name, "bugfix");
                assert_eq!(brief.prompt_source.as_deref(), Some("history"));
            }
            other => panic!("expected Generated, got {other:?}"),
        }
    }

    /// Progress calls recorded as (done, total, status).
    type Seen = Arc<std::sync::Mutex<Vec<(usize, usize, &'static str)>>>;

    #[tokio::test]
    async fn summarize_many_keeps_input_order_and_reports_progress() {
        let store = Arc::new(InMemoryEventStore::new());
        let mut events = Vec::new();
        for i in 0..6 {
            let mut e = ev(
                &format!("e{i}"),
                ActionKind::Edit,
                "/repo/src/x.rs",
                None,
                i,
            );
            e.session_id = format!("session-{i}");
            events.push(e);
        }
        store
            .upsert_events(EventBatch {
                tool_events: events,
                ..Default::default()
            })
            .await
            .unwrap();
        let sessions = store
            .sessions_for(ToolKind::Codex, None, 100, 0)
            .await
            .unwrap();
        let briefs = Arc::new(InMemoryBriefStore::new());
        let responses = (0..6).map(|_| MockResponse::text(GOOD)).collect();
        let s = summarizer(
            responses,
            SummarizeOptions {
                parallel: 3,
                ..fast()
            },
        );
        let seen: Seen = Default::default();
        let sink = Arc::clone(&seen);
        let progress: Arc<ProgressFn> = Arc::new(
            move |done: usize, total: usize, o: &SessionOutcome, _took: Duration| {
                sink.lock().unwrap().push((done, total, o.outcome.status()));
            },
        );
        let out = s
            .summarize_many_with_progress(
                store.clone(),
                briefs.clone(),
                sessions.clone(),
                Some(progress),
            )
            .await;
        let ids: Vec<&str> = out.iter().map(|o| o.session_id.as_str()).collect();
        let expected: Vec<&str> = sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(ids, expected);
        assert!(out
            .iter()
            .all(|o| matches!(o.outcome, Outcome::Generated { .. })));
        let seen = seen.lock().unwrap();
        let dones: Vec<usize> = seen.iter().map(|(d, _, _)| *d).collect();
        assert_eq!(dones, vec![1, 2, 3, 4, 5, 6]);
        assert!(seen.iter().all(|(_, t, st)| *t == 6 && *st == "generated"));
    }

    #[test]
    fn default_options_are_the_launch_defaults() {
        let o = SummarizeOptions::default();
        assert_eq!(o.parallel, 1);
        assert_eq!(o.transport_retries, 3);
        assert_eq!(o.backoff_base, Duration::from_secs(1));
        assert!(!o.preflight, "the library leaves preflight to its caller");
    }

    #[tokio::test]
    async fn a_timeout_is_not_retried() {
        let (store, briefs, session) = store_with_session().await;
        let s = summarizer(
            vec![
                MockResponse::error(LlmError::RequestFailed {
                    message: format!("{TIMED_OUT_PREFIX}error sending request"),
                }),
                MockResponse::text(GOOD),
            ],
            fast(),
        );
        match s
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await
            .outcome
        {
            Outcome::Failed { error } => {
                assert!(error.contains("not retried"), "{error}");
                assert!(error.contains("--timeout"), "{error}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// `n` captured Codex sessions, one edit each, none with a prompt.
    async fn many_sessions(n: i64) -> (Arc<InMemoryEventStore>, Vec<SessionSummary>) {
        let store = Arc::new(InMemoryEventStore::new());
        let events = (0..n)
            .map(|i| {
                let mut e = ev(
                    &format!("e{i}"),
                    ActionKind::Edit,
                    "/repo/src/x.rs",
                    None,
                    i,
                );
                e.session_id = format!("session-{i}");
                e
            })
            .collect();
        store
            .upsert_events(EventBatch {
                tool_events: events,
                ..Default::default()
            })
            .await
            .unwrap();
        let sessions = store
            .sessions_for(ToolKind::Codex, None, 100, 0)
            .await
            .unwrap();
        (store, sessions)
    }

    #[tokio::test]
    async fn preflight_runs_once_before_the_first_call_and_its_failure_is_shared() {
        let (store, sessions) = many_sessions(2).await;
        let briefs = Arc::new(InMemoryBriefStore::new());
        // Were preflight run per session, the second session would pass it on
        // the first GOOD and generate a brief with the second.
        let s = summarizer(
            vec![
                status_error(401),
                MockResponse::text(GOOD),
                MockResponse::text(GOOD),
            ],
            SummarizeOptions {
                preflight: true,
                parallel: 2,
                ..fast()
            },
        );
        let out = s.summarize_many(store, briefs, sessions).await;
        let errors: Vec<&str> = out
            .iter()
            .map(|o| match &o.outcome {
                Outcome::Failed { error } => error.as_str(),
                other => panic!("expected Failed, got {other:?}"),
            })
            .collect();
        let shared = s.preflight_error().expect("preflight failed");
        assert!(shared.contains("preflight") && shared.contains("401"), "{shared}");
        assert!(errors.iter().all(|e| *e == shared), "{errors:?}");
    }

    #[tokio::test]
    async fn preflight_is_skipped_when_no_session_needs_a_call() {
        let (store, briefs, session) = store_with_session().await;
        let first = summarizer(vec![MockResponse::text(GOOD)], fast());
        assert_eq!(
            first
                .summarize_one(store.as_ref(), briefs.as_ref(), &session)
                .await
                .outcome
                .status(),
            "generated"
        );

        // Unchanged: a rejecting gateway is never reached.
        let rerun = summarizer(
            vec![status_error(401)],
            SummarizeOptions {
                preflight: true,
                ..fast()
            },
        );
        assert_eq!(
            rerun
                .summarize_one(store.as_ref(), briefs.as_ref(), &session)
                .await
                .outcome
                .status(),
            "unchanged"
        );
        assert!(rerun.preflight_error().is_none());

        // Forced: preflight answers, then the brief is generated.
        let forced = summarizer(
            vec![MockResponse::text("OK"), MockResponse::text(GOOD)],
            SummarizeOptions {
                preflight: true,
                force: true,
                ..fast()
            },
        );
        assert_eq!(
            forced
                .summarize_one(store.as_ref(), briefs.as_ref(), &session)
                .await
                .outcome
                .status(),
            "generated"
        );
    }

    struct PanickingPrompts;
    impl PromptSource for PanickingPrompts {
        fn user_prompts(&self, _tool: ToolKind, id: &str) -> Result<Option<Vec<String>>, String> {
            if id == "session-1" {
                panic!("prompt source exploded");
            }
            Ok(None)
        }
    }

    #[tokio::test]
    async fn a_panicking_session_fails_alone_and_is_reported() {
        let (store, sessions) = many_sessions(3).await;
        let briefs = Arc::new(InMemoryBriefStore::new());
        let s = Arc::new(
            Summarizer::new(
                Arc::new(MockGateway::new(vec![MockResponse::text(GOOD); 3])),
                ModelConfig::new("m", "http://mock", "k"),
                fast(),
            )
            .with_prompt_source(Arc::new(PanickingPrompts)),
        );
        let seen: Seen = Default::default();
        let sink = Arc::clone(&seen);
        let progress: Arc<ProgressFn> = Arc::new(
            move |done: usize, total: usize, o: &SessionOutcome, _took: Duration| {
                sink.lock().unwrap().push((done, total, o.outcome.status()));
            },
        );
        let out = s
            .summarize_many_with_progress(store, briefs, sessions, Some(progress))
            .await;
        assert_eq!(out.len(), 3);
        for o in &out {
            match (&o.session_id[..], &o.outcome) {
                ("session-1", Outcome::Failed { error }) => {
                    assert!(error.contains("panicked"), "{error}");
                    assert!(error.contains("prompt source exploded"), "{error}");
                }
                ("session-1", other) => panic!("expected Failed, got {other:?}"),
                (_, other) => assert_eq!(other.status(), "generated"),
            }
        }
        assert_eq!(seen.lock().unwrap().len(), 3, "every session is reported");
    }

    #[tokio::test]
    async fn missing_prompts_are_warned_about_once() {
        let (store, sessions) = many_sessions(1).await;
        let briefs = InMemoryBriefStore::new();
        let s = Summarizer::new(
            Arc::new(MockGateway::new(vec![])),
            ModelConfig::new("m", "http://mock", "k"),
            SummarizeOptions {
                dry_run: true,
                ..fast()
            },
        )
        .with_prompt_source(Arc::new(BrokenPrompts));
        match s
            .summarize_one(store.as_ref(), &briefs, &sessions[0])
            .await
            .outcome
        {
            Outcome::DryRun { warnings, .. } => assert_eq!(
                warnings,
                vec!["prompts: the history reader found no such session".to_string()]
            ),
            other => panic!("expected DryRun, got {other:?}"),
        }

        // Without a failing source, the plain warning still appears.
        let plain = Summarizer::new(
            Arc::new(MockGateway::new(vec![])),
            ModelConfig::new("m", "http://mock", "k"),
            SummarizeOptions {
                dry_run: true,
                ..fast()
            },
        );
        match plain
            .summarize_one(store.as_ref(), &briefs, &sessions[0])
            .await
            .outcome
        {
            Outcome::DryRun { warnings, .. } => {
                assert_eq!(warnings.len(), 1);
                assert!(warnings[0].starts_with("prompts: none were available"));
            }
            other => panic!("expected DryRun, got {other:?}"),
        }
    }
}
