//! The summarizer: one native completion per session over a scrubbed
//! digest, validated tags, a stored brief, and bounded concurrency over a
//! batch (ADR 0022).

use std::sync::Arc;

use aikit_agent::llm::{LlmGateway, LlmMessage, LlmRequest, LlmResponse};
use aikit_session_capture::{EventStore, SecretScrubber, SessionSummary, ToolKind};
use sha2::{Digest as _, Sha256};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::areas::{group_areas, AreaMapping};
use crate::brief::{BriefStore, SessionBrief, TagAssignment};
use crate::digest::{build_digest, DigestOptions, PromptsInput};
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
            max_tokens: 1024,
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
    /// Concurrent model calls.
    pub parallel: usize,
    /// Mirror the primary tag through the [`TagMirror`], when one is set.
    pub mirror: bool,
}

impl Default for SummarizeOptions {
    fn default() -> Self {
        Self {
            tags: TagList::builtin(),
            areas: AreaMapping::default(),
            digest: DigestOptions::default(),
            force: false,
            dry_run: false,
            parallel: 4,
            mirror: true,
        }
    }
}

/// Supplies user prompts from outside the event store (the history reader).
/// `None` means the source cannot answer for this session; the digest then
/// falls back to prompt events. Text returned here is scrubbed by the engine.
pub trait PromptSource: Send + Sync {
    fn user_prompts(&self, tool: ToolKind, session_id: &str) -> Option<Vec<String>>;
}

/// Writes the primary tag into the tool's own tag slot (a `HistoryMutator`).
pub trait TagMirror: Send + Sync {
    /// `Ok(false)` when this tool has no tag slot; `Err` is reported as a
    /// warning on the outcome and never fails the session.
    fn mirror(&self, tool: ToolKind, session_id: &str, tag: &str) -> Result<bool, String>;
}

/// The result for one session.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// A brief was generated and stored. `mirrored` is `Some(Err)` when the
    /// mirror was attempted and failed, `Some(Ok(true))` when it wrote.
    Generated {
        brief: SessionBrief,
        mirrored: Option<Result<bool, String>>,
    },
    /// The stored brief already has this digest hash; nothing was asked.
    Unchanged {
        brief: SessionBrief,
    },
    /// `dry_run`: the exact user message that would be sent, and its hash.
    DryRun {
        user_message: String,
        mechanical: Vec<TagAssignment>,
        digest_hash: String,
    },
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone)]
pub struct SessionOutcome {
    pub tool: ToolKind,
    pub session_id: String,
    pub outcome: Outcome,
}

pub struct Summarizer {
    gateway: Arc<dyn LlmGateway>,
    model: ModelConfig,
    options: SummarizeOptions,
    prompts: Option<Arc<dyn PromptSource>>,
    mirror: Option<Arc<dyn TagMirror>>,
    scrubber: SecretScrubber,
}

/// SHA-256 hex of the exact user message: the identity of the request.
pub fn digest_hash(user_message: &str) -> String {
    hex::encode(Sha256::digest(user_message.as_bytes()))
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
            mirror: None,
            scrubber: SecretScrubber::default(),
        }
    }

    pub fn with_prompt_source(mut self, source: Arc<dyn PromptSource>) -> Self {
        self.prompts = Some(source);
        self
    }

    pub fn with_tag_mirror(mut self, mirror: Arc<dyn TagMirror>) -> Self {
        self.mirror = Some(mirror);
        self
    }

    pub fn with_scrubber(mut self, scrubber: SecretScrubber) -> Self {
        self.scrubber = scrubber;
        self
    }

    pub fn options(&self) -> &SummarizeOptions {
        &self.options
    }

    fn request(&self, messages: Vec<LlmMessage>) -> LlmRequest {
        LlmRequest {
            model: self.model.model.clone(),
            base_url: self.model.base_url.clone(),
            api_key: self.model.api_key.clone(),
            messages,
            tools: vec![],
            tool_choice: None,
            temperature: Some(self.model.temperature),
            top_p: None,
            max_tokens: Some(self.model.max_tokens),
            stream: false,
        }
    }

    /// The gateway blocks on its own runtime, so it runs under
    /// `spawn_blocking` (the judge does the same).
    async fn complete(&self, messages: Vec<LlmMessage>) -> Result<LlmResponse, String> {
        let gateway = Arc::clone(&self.gateway);
        let req = self.request(messages);
        tokio::task::spawn_blocking(move || gateway.complete(req))
            .await
            .map_err(|e| format!("model task failed: {e}"))?
            .map_err(|e| e.to_string())
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

        let prompts = match self
            .prompts
            .as_ref()
            .and_then(|p| p.user_prompts(session.tool, &session.session_id))
        {
            Some(list) => PromptsInput::History(
                list.iter()
                    .map(|p| self.scrubber.scrub(p))
                    .filter(|p| !p.trim().is_empty())
                    .collect(),
            ),
            None => PromptsInput::Events,
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
        let prompt_source = digest.prompt_source.map(str::to_string);
        // Scrub the rendered digest once more: everything the model sees has
        // passed the scrubber, whichever path it arrived by.
        let digest_text = self.scrubber.scrub(&digest.render(&self.options.digest));
        let user = user_message(&digest_text, &self.options.tags, &mechanical);
        let hash = digest_hash(&user);

        if self.options.dry_run {
            return Ok(Outcome::DryRun {
                user_message: user,
                mechanical,
                digest_hash: hash,
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

        let mut messages = vec![
            message("system", SYSTEM_PROMPT.to_string()),
            message("user", user),
        ];
        let first = self.complete(messages.clone()).await?;
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
                messages.push(message("assistant", first_text.clone()));
                messages.push(message(
                    "user",
                    corrective_message(&rejected, parse_error.as_deref(), &self.options.tags),
                ));
                let second = self.complete(messages).await?;
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
        };
        briefs
            .put_brief(&brief)
            .await
            .map_err(|e| format!("storing brief: {e}"))?;

        let mirrored = match (&self.mirror, brief.primary_tag()) {
            (Some(m), Some(tag)) if self.options.mirror => {
                Some(m.mirror(session.tool, &session.session_id, tag))
            }
            _ => None,
        };
        Ok(Outcome::Generated { brief, mirrored })
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
        let parallel = self.options.parallel.max(1);
        let sem = Arc::new(Semaphore::new(parallel));
        let mut set: JoinSet<(usize, SessionOutcome)> = JoinSet::new();
        for (i, session) in sessions.into_iter().enumerate() {
            let me = Arc::clone(self);
            let events = Arc::clone(&events);
            let briefs = Arc::clone(&briefs);
            let sem = Arc::clone(&sem);
            set.spawn(async move {
                let _permit = sem.acquire_owned().await.expect("semaphore open");
                (
                    i,
                    me.summarize_one(events.as_ref(), briefs.as_ref(), &session)
                        .await,
                )
            });
        }
        let mut out: Vec<(usize, SessionOutcome)> = Vec::new();
        while let Some(res) = set.join_next().await {
            match res {
                Ok(pair) => out.push(pair),
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

    fn summarizer(responses: Vec<MockResponse>, options: SummarizeOptions) -> Arc<Summarizer> {
        Arc::new(Summarizer::new(
            Arc::new(MockGateway::new(responses)),
            ModelConfig::new("mock-model", "http://mock", "k"),
            options,
        ))
    }

    #[tokio::test]
    async fn generates_stores_and_then_skips_unchanged() {
        let (store, briefs, session) = store_with_session().await;
        let reply = r#"{"summary": "Added a --force flag to the CLI.", "tags": [{"name": "feature", "why": "a new flag was added"}]}"#;
        let s = summarizer(vec![MockResponse::text(reply)], SummarizeOptions::default());

        let out = s
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        let brief = match &out.outcome {
            Outcome::Generated { brief, mirrored } => {
                assert!(mirrored.is_none(), "no mirror configured");
                brief.clone()
            }
            other => panic!("expected Generated, got {other:?}"),
        };
        assert_eq!(brief.summary, "Added a --force flag to the CLI.");
        assert_eq!(brief.tags.len(), 1);
        assert_eq!(brief.tags[0].name, "feature");
        assert_eq!(brief.tags[0].source, TagSource::Model);
        assert_eq!(brief.tags[0].justification, "a new flag was added");
        assert_eq!(brief.areas[0].area, "src");
        assert_eq!(brief.model, "mock-model");
        assert_eq!(brief.prompt_source.as_deref(), Some("events"));
        assert_eq!(brief.digest_hash.len(), 64);
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
                ..Default::default()
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
                ..Default::default()
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
            } => {
                assert!(user_message.contains("## Allowed tags"));
                assert!(user_message.contains("1. add a --force flag"));
                assert!(mechanical.is_empty());
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
            SummarizeOptions::default(),
        );
        let out = s
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        match out.outcome {
            Outcome::Generated { brief, .. } => {
                assert_eq!(brief.summary, "s2", "the retry's reply is used");
                let names: Vec<&str> = brief.tags.iter().map(|t| t.name.as_str()).collect();
                assert_eq!(names, vec!["feature"]);
                assert_eq!(brief.rejected_tags, vec!["enhancement"]);
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
                MockResponse::text(r#"{"summary": "ok", "tags": []}"#),
            ],
            SummarizeOptions::default(),
        );
        let out = s
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        assert!(
            matches!(out.outcome, Outcome::Generated { ref brief, .. } if brief.summary == "ok")
        );

        let twice_bad = summarizer(
            vec![MockResponse::text("no"), MockResponse::text("still no")],
            SummarizeOptions {
                force: true,
                ..Default::default()
            },
        );
        let out = twice_bad
            .summarize_one(store.as_ref(), briefs.as_ref(), &session)
            .await;
        assert!(matches!(out.outcome, Outcome::Failed { .. }));
    }

    struct RecordingMirror(std::sync::Mutex<Vec<(ToolKind, String, String)>>);
    impl TagMirror for RecordingMirror {
        fn mirror(&self, tool: ToolKind, id: &str, tag: &str) -> Result<bool, String> {
            self.0.lock().unwrap().push((tool, id.into(), tag.into()));
            Ok(true)
        }
    }

    struct FixedPrompts;
    impl PromptSource for FixedPrompts {
        fn user_prompts(&self, _tool: ToolKind, _id: &str) -> Option<Vec<String>> {
            Some(vec![
                "from history sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789".into(),
            ])
        }
    }

    #[tokio::test]
    async fn mechanical_tag_is_primary_mirrored_and_history_prompts_are_scrubbed() {
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
        let mirror = Arc::new(RecordingMirror(Default::default()));
        let s = Arc::new(
            Summarizer::new(
                Arc::new(MockGateway::new(vec![MockResponse::text(
                    r#"{"summary": "Fixed a test.", "tags": [{"name": "bugfix", "why": "..."}]}"#,
                )])),
                ModelConfig::new("m", "http://mock", "k"),
                SummarizeOptions {
                    dry_run: false,
                    ..Default::default()
                },
            )
            .with_prompt_source(Arc::new(FixedPrompts))
            .with_tag_mirror(mirror.clone()),
        );
        // Dry-run view of the same session to inspect the message.
        let dry = Arc::new(
            Summarizer::new(
                Arc::new(MockGateway::new(vec![])),
                ModelConfig::new("m", "http://mock", "k"),
                SummarizeOptions {
                    dry_run: true,
                    ..Default::default()
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
            Outcome::Generated { brief, mirrored } => {
                assert_eq!(brief.primary_tag(), Some("test"));
                assert_eq!(brief.tags[0].source, TagSource::Mechanical);
                assert_eq!(brief.tags[1].name, "bugfix");
                assert_eq!(brief.prompt_source.as_deref(), Some("history"));
                assert_eq!(mirrored, Some(Ok(true)));
            }
            other => panic!("expected Generated, got {other:?}"),
        }
        let calls = mirror.0.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].2, "test");
    }

    #[tokio::test]
    async fn summarize_many_keeps_input_order_under_concurrency() {
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
        let responses = (0..6)
            .map(|_| MockResponse::text(r#"{"summary": "s", "tags": []}"#))
            .collect();
        let s = summarizer(
            responses,
            SummarizeOptions {
                parallel: 3,
                ..Default::default()
            },
        );
        let out = s
            .summarize_many(store.clone(), briefs.clone(), sessions.clone())
            .await;
        let ids: Vec<&str> = out.iter().map(|o| o.session_id.as_str()).collect();
        let expected: Vec<&str> = sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(ids, expected);
        assert!(out
            .iter()
            .all(|o| matches!(o.outcome, Outcome::Generated { .. })));
    }
}
