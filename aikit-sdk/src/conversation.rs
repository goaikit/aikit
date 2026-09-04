//! Message-list pipeline: a conversation → one gateway completion → JSON
//! Schema validation → caller-authored corrective turns, every attempt kept.
//!
//! [`Pipeline`](crate::pipeline::Pipeline) renders a single prompt and drives a
//! subprocess agent. A judge (`aikit-evals`, spec `eval-judge.md`) needs the
//! same validate-and-retry loop over a *message list* against the native
//! [`LlmGateway`], with nothing on the wire the caller did not write:
//!
//! - the messages are sent exactly as given; the pipeline adds no persona and
//!   no "answer in JSON" suffix,
//! - a corrective turn after a rejected reply is a closure the caller supplies
//!   (and `None` means a rejected reply simply ends the conversation),
//! - every request that left the process is in the result, accepted or not,
//!   so an artifact can show what was asked and what came back.
//!
//! Two failure classes are counted separately: a **validation** failure (not
//! JSON, schema mismatch, truncated reply) spends a corrective turn; a
//! **transport** failure (HTTP 429, any 5xx, a timeout or connection error) is
//! retried with exponential backoff under its own budget, while 401, 403, 404
//! and any other client error fail at once.
//!
//! Blocking, like [`Pipeline::run`](crate::pipeline::Pipeline::run): the
//! gateway builds its own runtime, so callers on a tokio worker MUST use
//! `spawn_blocking`.

use crate::pipeline::PipelineError;
use crate::validation::ResponseValidator;
use aikit_agent::llm::{LlmError, LlmGateway, LlmMessage, LlmRequest, LlmUsage};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

/// What an attempt was: a reply that went through validation, or a request
/// that never produced one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttemptKind {
    /// A reply came back and was validated — accepted or rejected.
    Validation,
    /// The request produced no reply: an HTTP error status, a timeout, a
    /// connection failure. Retried with backoff when transient.
    Transport,
}

/// One request that left the process, and what came back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationAttempt {
    pub kind: AttemptKind,
    /// The messages exactly as sent for this attempt.
    pub messages: Vec<LlmMessage>,
    /// The raw reply text, when a reply arrived.
    #[serde(default)]
    pub response_text: Option<String>,
    #[serde(default)]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub usage: Option<LlmUsage>,
    /// The model the provider reported answering with, when it said.
    #[serde(default)]
    pub model_reported: Option<String>,
    /// Why this attempt did not yield an accepted reply, if it did not.
    #[serde(default)]
    pub error: Option<String>,
}

/// The accepted reply and the full attempt history that led to it.
#[derive(Debug, Clone)]
pub struct ConversationResult {
    /// The validated JSON object.
    pub data: serde_json::Value,
    /// Every attempt in order; the last one is the accepted reply.
    pub attempts: Vec<ConversationAttempt>,
}

/// Why a conversation produced no accepted reply.
#[derive(Debug)]
pub enum ConversationError {
    /// The schema string is not a usable JSON Schema; nothing was sent.
    Schema(String),
    /// A transport failure that is not retried, or one that exhausted the
    /// transport budget.
    Transport {
        attempts: Vec<ConversationAttempt>,
        source: LlmError,
    },
    /// The reply was rejected and no corrective turn was left.
    ValidationExhausted {
        attempts: Vec<ConversationAttempt>,
        errors: Vec<String>,
    },
}

impl ConversationError {
    /// The attempts made before the failure, so a caller can record them.
    pub fn attempts(&self) -> &[ConversationAttempt] {
        match self {
            ConversationError::Schema(_) => &[],
            ConversationError::Transport { attempts, .. }
            | ConversationError::ValidationExhausted { attempts, .. } => attempts,
        }
    }
}

impl fmt::Display for ConversationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConversationError::Schema(detail) => write!(f, "invalid reply schema: {}", detail),
            ConversationError::Transport { attempts, source } => write!(
                f,
                "transport failure after {} attempt(s): {}",
                attempts.len(),
                source
            ),
            ConversationError::ValidationExhausted { attempts, errors } => write!(
                f,
                "reply rejected after {} attempt(s): {}",
                attempts.len(),
                errors.join("; ")
            ),
        }
    }
}

impl std::error::Error for ConversationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConversationError::Transport { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Is this gateway error worth another try?
///
/// 429 and every 5xx are transient by definition; a request that never got a
/// status (timeout, connection reset, unreadable body) may succeed next time.
/// A missing key, a 401/403/404 or any other 4xx will not improve on retry.
pub fn is_retryable_transport(err: &LlmError) -> bool {
    match err {
        LlmError::ErrorResponse { status, .. } => *status == 429 || (500..=599).contains(status),
        LlmError::RequestFailed { .. } => true,
        LlmError::NoApiKey { .. } | LlmError::StreamProtocol { .. } => false,
    }
}

/// The caller's corrective turn: the validation errors that rejected a reply
/// in, the user message that asks for a better one out (spec eval-judge R7).
pub type Corrective<'a> = &'a dyn Fn(&[String]) -> String;

/// A message-list pipeline: send, validate, correct, record.
#[derive(Debug, Clone)]
pub struct ConversationPipeline {
    /// JSON Schema (as a JSON string) the reply must satisfy.
    pub schema: String,
    /// Corrective turns allowed after a rejected reply (0 = none).
    pub max_retries: u32,
    /// Retries allowed for transient transport failures (0 = none).
    pub transport_retries: u32,
    /// First backoff delay; doubles per transport retry.
    pub backoff_base: Duration,
}

impl ConversationPipeline {
    /// A pipeline with no corrective turns, three transport retries and a
    /// one-second base backoff.
    pub fn new(schema: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
            max_retries: 0,
            transport_retries: 3,
            backoff_base: Duration::from_secs(1),
        }
    }

    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    pub fn transport_retries(mut self, n: u32) -> Self {
        self.transport_retries = n;
        self
    }

    pub fn backoff_base(mut self, d: Duration) -> Self {
        self.backoff_base = d;
        self
    }

    /// Run the conversation to an accepted reply or a recorded failure.
    ///
    /// `base` supplies the identity (model, endpoint, key, sampling) and the
    /// opening messages. Tools, tool choice and streaming are cleared on every
    /// attempt: this pipeline sends completions only.
    ///
    /// `corrective` renders the user message that follows a rejected reply,
    /// given the validation errors. `None` means a rejected reply ends the
    /// conversation whatever `max_retries` says.
    ///
    /// Blocking. Callers on a tokio worker MUST use `spawn_blocking`.
    pub fn run(
        &self,
        base: &LlmRequest,
        gateway: &dyn LlmGateway,
        corrective: Option<Corrective>,
    ) -> Result<ConversationResult, ConversationError> {
        let schema_value: serde_json::Value = serde_json::from_str(&self.schema)
            .map_err(|e| ConversationError::Schema(format!("schema is not JSON: {}", e)))?;
        jsonschema::validator_for(&schema_value)
            .map_err(|e| ConversationError::Schema(format!("schema does not compile: {}", e)))?;

        let mut attempts: Vec<ConversationAttempt> = Vec::new();
        let mut messages = base.messages.clone();
        let mut corrective_turns = 0u32;
        let mut transport_failures = 0u32;

        loop {
            let req = LlmRequest {
                messages: messages.clone(),
                tools: Vec::new(),
                tool_choice: None,
                stream: false,
                ..base.clone()
            };

            let resp = match gateway.complete(req) {
                Ok(resp) => resp,
                Err(err) => {
                    attempts.push(ConversationAttempt {
                        kind: AttemptKind::Transport,
                        messages: messages.clone(),
                        response_text: None,
                        finish_reason: None,
                        usage: None,
                        model_reported: None,
                        error: Some(err.to_string()),
                    });
                    if is_retryable_transport(&err) && transport_failures < self.transport_retries {
                        let delay = self
                            .backoff_base
                            .saturating_mul(1u32 << transport_failures.min(16));
                        transport_failures += 1;
                        if !delay.is_zero() {
                            std::thread::sleep(delay);
                        }
                        continue;
                    }
                    return Err(ConversationError::Transport {
                        attempts,
                        source: err,
                    });
                }
            };

            let text = resp.content.clone().unwrap_or_default();
            let mut errors: Vec<String> = Vec::new();
            if resp.finish_reason.as_deref() == Some("length") {
                errors.push("reply truncated: finish_reason is \"length\"".to_string());
            }
            if text.trim().is_empty() {
                errors.push("empty reply".to_string());
            }
            let data = if errors.is_empty() {
                match ResponseValidator::validate(&text, &self.schema) {
                    Ok(validated) => Some(validated.data),
                    Err(PipelineError::ValidationFailed { errors: e, .. }) => {
                        errors = e;
                        None
                    }
                    Err(other) => {
                        errors = vec![other.to_string()];
                        None
                    }
                }
            } else {
                None
            };

            attempts.push(ConversationAttempt {
                kind: AttemptKind::Validation,
                messages: messages.clone(),
                response_text: resp.content.clone(),
                finish_reason: resp.finish_reason.clone(),
                usage: resp.usage.clone(),
                model_reported: resp.model.clone(),
                error: if errors.is_empty() {
                    None
                } else {
                    Some(errors.join("; "))
                },
            });

            if let Some(data) = data {
                return Ok(ConversationResult { data, attempts });
            }

            let Some(corrective) = corrective else {
                return Err(ConversationError::ValidationExhausted { attempts, errors });
            };
            if corrective_turns >= self.max_retries {
                return Err(ConversationError::ValidationExhausted { attempts, errors });
            }
            corrective_turns += 1;

            // The rejected reply, then the caller's corrective turn: recorded
            // turns, not a hidden loop (spec eval-judge R7).
            messages.push(LlmMessage {
                role: "assistant".to_string(),
                content: Some(text),
                tool_calls: None,
                tool_call_id: None,
            });
            messages.push(LlmMessage {
                role: "user".to_string(),
                content: Some(corrective(&errors)),
                tool_calls: None,
                tool_call_id: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_agent::llm::mock::{MockGateway, MockResponse};
    use aikit_agent::llm::{FunctionDefinition, LlmResponse, ToolDefinition};
    use std::sync::Mutex;

    const SCHEMA: &str = r#"{"type":"object","properties":{"answer":{"type":"integer"}},"required":["answer"],"additionalProperties":false}"#;

    fn base(messages: Vec<LlmMessage>) -> LlmRequest {
        LlmRequest {
            model: "judge-model".to_string(),
            base_url: "http://127.0.0.1:9/v1".to_string(),
            api_key: "k".to_string(),
            messages,
            tools: vec![],
            tool_choice: None,
            temperature: Some(0.0),
            top_p: None,
            max_tokens: Some(64),
            stream: false,
        }
    }

    fn user(text: &str) -> LlmMessage {
        LlmMessage {
            role: "user".to_string(),
            content: Some(text.to_string()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn fast(schema: &str) -> ConversationPipeline {
        ConversationPipeline::new(schema).backoff_base(Duration::ZERO)
    }

    /// Records every request it receives and answers from a queue.
    struct RecordingGateway {
        inner: MockGateway,
        seen: Mutex<Vec<LlmRequest>>,
    }

    impl RecordingGateway {
        fn new(responses: Vec<MockResponse>) -> Self {
            Self {
                inner: MockGateway::new(responses),
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    impl LlmGateway for RecordingGateway {
        fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
            self.seen.lock().unwrap().push(req.clone());
            self.inner.complete(req)
        }
        fn stream(&self, req: LlmRequest) -> Result<aikit_agent::llm::LlmStreamHandle, LlmError> {
            self.inner.stream(req)
        }
    }

    #[test]
    fn accepted_first_reply_is_one_validation_attempt() {
        let gw = MockGateway::new(vec![MockResponse::text(r#"{"answer": 4}"#)]);
        let out = fast(SCHEMA)
            .run(&base(vec![user("q")]), &gw, None)
            .expect("valid reply");
        assert_eq!(out.data["answer"], 4);
        assert_eq!(out.attempts.len(), 1);
        assert_eq!(out.attempts[0].kind, AttemptKind::Validation);
        assert!(out.attempts[0].error.is_none());
        assert_eq!(
            out.attempts[0].response_text.as_deref(),
            Some(r#"{"answer": 4}"#)
        );
    }

    #[test]
    fn rejected_reply_becomes_assistant_turn_followed_by_corrective_user_turn() {
        let gw = RecordingGateway::new(vec![
            MockResponse::text(r#"{"wrong": true}"#),
            MockResponse::text(r#"{"answer": 2}"#),
        ]);
        let corrective = |errors: &[String]| format!("rejected: {}", errors.join("; "));
        let out = fast(SCHEMA)
            .max_retries(2)
            .run(&base(vec![user("q")]), &gw, Some(&corrective))
            .expect("second reply accepted");

        assert_eq!(out.attempts.len(), 2);
        assert!(out
            .attempts
            .iter()
            .all(|a| a.kind == AttemptKind::Validation));
        assert!(
            out.attempts[0].error.is_some(),
            "first attempt was rejected"
        );

        let seen = gw.seen.lock().unwrap();
        let second = &seen[1].messages;
        assert_eq!(second.len(), 3, "user, rejected assistant, corrective user");
        assert_eq!(second[1].role, "assistant");
        assert_eq!(second[1].content.as_deref(), Some(r#"{"wrong": true}"#));
        assert_eq!(second[2].role, "user");
        assert!(second[2]
            .content
            .as_deref()
            .unwrap()
            .starts_with("rejected: "));
        assert_eq!(out.attempts[1].messages.len(), 3);
    }

    #[test]
    fn no_corrective_means_no_second_attempt_whatever_max_retries_says() {
        let gw = RecordingGateway::new(vec![
            MockResponse::text("not json"),
            MockResponse::text(r#"{"answer": 2}"#),
        ]);
        let err = fast(SCHEMA)
            .max_retries(5)
            .run(&base(vec![user("q")]), &gw, None)
            .expect_err("no corrective turn available");
        assert!(matches!(err, ConversationError::ValidationExhausted { .. }));
        assert_eq!(err.attempts().len(), 1);
        assert_eq!(gw.seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn corrective_budget_exhausted_is_a_recorded_failure() {
        let gw = MockGateway::new(vec![
            MockResponse::text("nope"),
            MockResponse::text("still nope"),
        ]);
        let corrective = |_: &[String]| "again".to_string();
        let err = fast(SCHEMA)
            .max_retries(1)
            .run(&base(vec![user("q")]), &gw, Some(&corrective))
            .expect_err("budget of one corrective turn");
        assert_eq!(err.attempts().len(), 2);
        assert!(matches!(err, ConversationError::ValidationExhausted { .. }));
    }

    #[test]
    fn transient_status_is_retried_and_recorded_as_transport() {
        let gw = MockGateway::new(vec![
            MockResponse::error(LlmError::ErrorResponse {
                status: 503,
                url: "u".to_string(),
                body: "busy".to_string(),
            }),
            MockResponse::text(r#"{"answer": 1}"#),
        ]);
        let out = fast(SCHEMA)
            .run(&base(vec![user("q")]), &gw, None)
            .expect("second attempt succeeds");
        assert_eq!(out.attempts.len(), 2);
        assert_eq!(out.attempts[0].kind, AttemptKind::Transport);
        assert!(out.attempts[0].error.as_deref().unwrap().contains("503"));
        assert_eq!(out.attempts[1].kind, AttemptKind::Validation);
    }

    #[test]
    fn unauthorized_fails_after_exactly_one_attempt() {
        let gw = RecordingGateway::new(vec![
            MockResponse::error(LlmError::ErrorResponse {
                status: 401,
                url: "u".to_string(),
                body: "no".to_string(),
            }),
            MockResponse::text(r#"{"answer": 1}"#),
        ]);
        let err = fast(SCHEMA)
            .transport_retries(3)
            .run(&base(vec![user("q")]), &gw, None)
            .expect_err("401 is final");
        assert!(matches!(err, ConversationError::Transport { .. }));
        assert_eq!(err.attempts().len(), 1);
        assert_eq!(gw.seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn transport_budget_exhausted_returns_transport_error() {
        let busy = || {
            MockResponse::error(LlmError::ErrorResponse {
                status: 500,
                url: "u".to_string(),
                body: "x".to_string(),
            })
        };
        let gw = MockGateway::new(vec![busy(), busy(), busy()]);
        let err = fast(SCHEMA)
            .transport_retries(1)
            .run(&base(vec![user("q")]), &gw, None)
            .expect_err("two attempts then give up");
        assert_eq!(err.attempts().len(), 2);
        assert!(matches!(err, ConversationError::Transport { .. }));
    }

    #[test]
    fn truncated_reply_is_a_validation_failure() {
        let mut truncated = MockResponse::text(r#"{"answer": 1"#);
        truncated.finish_reason = "length".to_string();
        let gw = MockGateway::new(vec![truncated]);
        let err = fast(SCHEMA)
            .run(&base(vec![user("q")]), &gw, None)
            .expect_err("length is a rejection");
        match err {
            ConversationError::ValidationExhausted { attempts, errors } => {
                assert_eq!(attempts[0].kind, AttemptKind::Validation);
                assert!(errors.iter().any(|e| e.contains("length")), "{errors:?}");
            }
            other => panic!("expected validation failure, got {other}"),
        }
    }

    #[test]
    fn sent_request_carries_no_tools_and_no_streaming() {
        let gw = RecordingGateway::new(vec![MockResponse::text(r#"{"answer": 1}"#)]);
        let mut req = base(vec![user("q")]);
        req.tools = vec![ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "leak".to_string(),
                description: None,
                parameters: serde_json::json!({}),
            },
        }];
        req.stream = true;
        fast(SCHEMA).run(&req, &gw, None).expect("ok");
        let seen = gw.seen.lock().unwrap();
        assert!(seen[0].tools.is_empty());
        assert!(seen[0].tool_choice.is_none());
        assert!(!seen[0].stream);
        assert_eq!(
            seen[0].messages.len(),
            1,
            "nothing injected around the caller's message"
        );
    }

    #[test]
    fn unusable_schema_sends_nothing() {
        let gw = RecordingGateway::new(vec![MockResponse::text(r#"{"answer": 1}"#)]);
        let err = fast("{not json")
            .run(&base(vec![user("q")]), &gw, None)
            .expect_err("schema rejected");
        assert!(matches!(err, ConversationError::Schema(_)));
        assert!(gw.seen.lock().unwrap().is_empty());
    }
}
