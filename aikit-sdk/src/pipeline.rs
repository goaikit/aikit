//! Structured agent pipeline: template rendering → agent invocation → JSON validation
//! → optional report generation.

use crate::agent_runner::AgentRunner;
use crate::report::ReportRenderer;
use crate::runner::RunError;
use crate::template::TemplateRenderer;
use crate::validation::ResponseValidator;
use std::fmt;

// ---------------------------------------------------------------------------
// PipelineError
// ---------------------------------------------------------------------------

/// All errors that can occur during pipeline execution.
#[derive(Debug)]
pub enum PipelineError {
    /// A `{{slot}}` referenced in the template was not found in the provided slots map.
    TemplateSlotMissing { slot: String },
    /// The agent invocation failed.
    AgentInvocation { source: RunError },
    /// The agent response did not pass JSON schema validation.
    ValidationFailed {
        raw_output: String,
        errors: Vec<String>,
    },
    /// Validation still failed after `max_retries` retry attempts.
    MaxRetriesExceeded { last_error: Box<PipelineError> },
    /// A `{{slot}}` referenced in the report template was not found.
    ReportRender { slot: String },
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipelineError::TemplateSlotMissing { slot } => {
                write!(f, "template slot missing: '{}'", slot)
            }
            PipelineError::AgentInvocation { source } => {
                write!(f, "agent invocation failed: {}", source)
            }
            PipelineError::ValidationFailed { errors, .. } => {
                write!(f, "validation failed: {}", errors.join("; "))
            }
            PipelineError::MaxRetriesExceeded { last_error } => {
                write!(f, "max retries exceeded; last error: {}", last_error)
            }
            PipelineError::ReportRender { slot } => {
                write!(f, "report template slot missing: '{}'", slot)
            }
        }
    }
}

impl std::error::Error for PipelineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PipelineError::AgentInvocation { source } => Some(source),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// OutputFormat
// ---------------------------------------------------------------------------

/// How the pipeline formats its final output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Pretty-printed JSON (default).
    #[default]
    Json,
    /// Markdown rendered from a report template.
    Markdown,
}

// ---------------------------------------------------------------------------
// PipelineResult
// ---------------------------------------------------------------------------

/// The successful result of a `Pipeline::run` call.
#[derive(Debug)]
pub struct PipelineResult {
    /// The rendered report (JSON or Markdown depending on `OutputFormat`).
    pub report: String,
    /// The parsed JSON data from the validated agent response.
    pub data: serde_json::Value,
    /// Number of attempts used (1 = no retries needed).
    pub attempts: u32,
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// A structured agent pipeline that:
/// 1. Renders a prompt template with slots.
/// 2. Calls an agent runner.
/// 3. Validates the JSON response against a schema.
/// 4. Retries on failure up to `max_retries` times.
/// 5. Renders the output according to `output_format`.
pub struct Pipeline {
    /// Prompt template with `{{slot}}` placeholders.
    pub template: String,
    /// JSON Schema (as a JSON string) for validating the agent response.
    pub schema: String,
    /// Optional report template for Markdown output.
    pub report_template: Option<String>,
    /// Maximum number of retries after initial failure (0 = no retries).
    pub max_retries: u32,
    /// How to format the final output.
    pub output_format: OutputFormat,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self {
            template: String::new(),
            schema: String::new(),
            report_template: None,
            max_retries: 0,
            output_format: OutputFormat::Json,
        }
    }
}

impl Pipeline {
    /// Create a new `Pipeline` with the given template and schema.
    pub fn new(template: impl Into<String>, schema: impl Into<String>) -> Self {
        Self {
            template: template.into(),
            schema: schema.into(),
            ..Default::default()
        }
    }

    /// Set the optional report template for Markdown output.
    pub fn report_template(mut self, t: impl Into<String>) -> Self {
        self.report_template = Some(t.into());
        self
    }

    /// Set the maximum number of retry attempts.
    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    /// Set the output format.
    pub fn output_format(mut self, fmt: OutputFormat) -> Self {
        self.output_format = fmt;
        self
    }

    /// Run the pipeline.
    ///
    /// # Arguments
    /// * `slots` - Values for template placeholders.
    /// * `runner` - An `AgentRunner` whose `run()` is called for each attempt.
    ///
    /// Blocking. Callers needing concurrency MUST use spawn_blocking.
    pub fn run(
        &self,
        slots: &[(&str, &str)],
        runner: AgentRunner,
    ) -> Result<PipelineResult, PipelineError> {
        // Step 1: render the template — fatal if a slot is missing
        let original_prompt = TemplateRenderer::render(&self.template, slots)?;

        let mut attempt = 1u32;
        let mut current_prompt = original_prompt.clone();

        loop {
            // Step 2: call the agent — fatal on RunError (no retry)
            let raw_text = runner.run(&current_prompt)?;

            // Step 3: validate
            match ResponseValidator::validate(&raw_text, &self.schema) {
                Ok(validated) => {
                    // Validation passed — render output
                    let report = match &self.output_format {
                        OutputFormat::Json => ReportRenderer::render_json(&validated.data)?,
                        OutputFormat::Markdown => {
                            let tmpl = self.report_template.as_deref().unwrap_or("");
                            ReportRenderer::render_markdown(tmpl, &validated.data)?
                        }
                    };
                    return Ok(PipelineResult {
                        report,
                        data: validated.data,
                        attempts: attempt,
                    });
                }
                Err(PipelineError::ValidationFailed { raw_output, errors }) => {
                    // Check retry budget
                    if attempt > self.max_retries {
                        if self.max_retries == 0 {
                            return Err(PipelineError::ValidationFailed { raw_output, errors });
                        } else {
                            return Err(PipelineError::MaxRetriesExceeded {
                                last_error: Box::new(PipelineError::ValidationFailed {
                                    raw_output,
                                    errors,
                                }),
                            });
                        }
                    }

                    // Build augmented retry prompt (§4.6)
                    let mut retry_prompt = original_prompt.clone();
                    retry_prompt.push_str("\n\n## Previous response\n");
                    retry_prompt.push_str(&raw_output);
                    retry_prompt.push_str("\n\n## Validation errors\n");
                    for err_msg in &errors {
                        retry_prompt.push_str("- ");
                        retry_prompt.push_str(err_msg);
                        retry_prompt.push('\n');
                    }
                    retry_prompt.push_str(
                        "\n\nPlease respond again. Your response MUST contain a single ```json block matching the schema.",
                    );

                    current_prompt = retry_prompt;
                    attempt += 1;
                }
                Err(other) => return Err(other),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runner::AgentRunner;

    // -----------------------------------------------------------------------
    // Error display / std::error::Error tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_pipeline_error_display_template_slot_missing() {
        let err = PipelineError::TemplateSlotMissing {
            slot: "foo".to_string(),
        };
        assert!(err.to_string().contains("foo"));
    }

    #[test]
    fn test_pipeline_error_display_validation_failed() {
        let err = PipelineError::ValidationFailed {
            raw_output: "bad".to_string(),
            errors: vec!["missing field".to_string()],
        };
        assert!(err.to_string().contains("missing field"));
    }

    #[test]
    fn test_pipeline_error_display_max_retries_exceeded() {
        let inner = PipelineError::ValidationFailed {
            raw_output: "x".to_string(),
            errors: vec!["err".to_string()],
        };
        let err = PipelineError::MaxRetriesExceeded {
            last_error: Box::new(inner),
        };
        assert!(err.to_string().contains("max retries exceeded"));
    }

    #[test]
    fn test_pipeline_error_display_report_render() {
        let err = PipelineError::ReportRender {
            slot: "body".to_string(),
        };
        assert!(err.to_string().contains("body"));
    }

    #[test]
    fn test_pipeline_error_source_agent_invocation() {
        use crate::runner::RunError;
        use std::error::Error;
        let run_err = RunError::AgentNotRunnable("fake".to_string());
        let err = PipelineError::AgentInvocation { source: run_err };
        assert!(err.source().is_some());
    }

    #[test]
    fn test_pipeline_error_source_others_none() {
        use std::error::Error;
        let err = PipelineError::TemplateSlotMissing {
            slot: "x".to_string(),
        };
        assert!(err.source().is_none());
    }

    // -----------------------------------------------------------------------
    // Template integration
    // -----------------------------------------------------------------------

    #[test]
    fn test_template_render_missing_slot_is_fatal() {
        let result = TemplateRenderer::render("Hello {{missing}}", &[]);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Validation integration
    // -----------------------------------------------------------------------

    #[test]
    fn test_response_validator_valid_json() {
        use crate::validation::ResponseValidator;

        let schema =
            r#"{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}"#;
        let raw = r#"{"name":"Alice"}"#;
        let validated = ResponseValidator::validate(raw, schema).unwrap();
        assert_eq!(validated.data["name"], "Alice");
    }

    #[test]
    fn test_response_validator_invalid_json_fails() {
        use crate::validation::ResponseValidator;

        let schema =
            r#"{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}"#;
        let raw = r#"{"age":42}"#;
        let err = ResponseValidator::validate(raw, schema).unwrap_err();
        match err {
            PipelineError::ValidationFailed { errors, .. } => {
                assert!(!errors.is_empty());
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // AC14: max_retries(0) → ValidationFailed returned directly (not MaxRetriesExceeded)
    // -----------------------------------------------------------------------

    #[test]
    fn test_pipeline_max_retries_0_returns_validation_failed_directly() {
        let schema =
            r#"{"type":"object","properties":{"score":{"type":"integer"}},"required":["score"]}"#;
        let (mock_runner, _) = AgentRunner::with_mock(vec![Ok("not valid json".to_string())]);

        let result = Pipeline::new("test prompt", schema)
            .max_retries(0)
            .run(&[], mock_runner);

        assert!(
            matches!(result, Err(PipelineError::ValidationFailed { .. })),
            "expected ValidationFailed, got {:?}",
            result
        );
    }

    // -----------------------------------------------------------------------
    // AC15: max_retries(1), both attempts fail → MaxRetriesExceeded boxing ValidationFailed
    // -----------------------------------------------------------------------

    #[test]
    fn test_pipeline_max_retries_1_both_fail_returns_max_retries_exceeded() {
        let schema =
            r#"{"type":"object","properties":{"score":{"type":"integer"}},"required":["score"]}"#;
        let (mock_runner, _) = AgentRunner::with_mock(vec![
            Ok("bad output 1".to_string()),
            Ok("bad output 2".to_string()),
        ]);

        let result = Pipeline::new("test prompt", schema)
            .max_retries(1)
            .run(&[], mock_runner);

        match result {
            Err(PipelineError::MaxRetriesExceeded { last_error }) => {
                assert!(
                    matches!(*last_error, PipelineError::ValidationFailed { .. }),
                    "last_error should be ValidationFailed, got {:?}",
                    last_error
                );
            }
            other => panic!("expected MaxRetriesExceeded, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // AC16: max_retries(1), second attempt succeeds → attempts == 2
    // -----------------------------------------------------------------------

    #[test]
    fn test_pipeline_max_retries_1_second_attempt_succeeds_attempts_equals_2() {
        let schema =
            r#"{"type":"object","properties":{"score":{"type":"integer"}},"required":["score"]}"#;
        let valid_response = "```json\n{\"score\": 7}\n```".to_string();
        let (mock_runner, _) = AgentRunner::with_mock(vec![
            Ok("not valid json at all".to_string()),
            Ok(valid_response),
        ]);

        let result = Pipeline::new("test prompt", schema)
            .max_retries(1)
            .run(&[], mock_runner)
            .expect("should succeed on second attempt");

        assert_eq!(result.attempts, 2, "attempts should be 2 after one retry");
    }

    // -----------------------------------------------------------------------
    // AC17: RunError from agent bypasses retry loop regardless of max_retries
    // -----------------------------------------------------------------------

    #[test]
    fn test_pipeline_agent_run_error_is_not_retried() {
        use crate::runner::RunError;

        let schema = r#"{"type":"object"}"#;
        let (mock_runner, _) = AgentRunner::with_mock(vec![Err(PipelineError::AgentInvocation {
            source: RunError::AgentNotRunnable("fake-agent".to_string()),
        })]);

        // Even with max_retries=5, a fatal RunError must surface immediately
        let result = Pipeline::new("test prompt", schema)
            .max_retries(5)
            .run(&[], mock_runner);

        assert!(
            matches!(result, Err(PipelineError::AgentInvocation { .. })),
            "expected AgentInvocation (fatal, no retry), got {:?}",
            result
        );
    }

    // -----------------------------------------------------------------------
    // AC18: Augmented retry prompt contains required content
    // -----------------------------------------------------------------------

    #[test]
    fn test_pipeline_retry_prompt_contains_required_content() {
        let schema =
            r#"{"type":"object","properties":{"score":{"type":"integer"}},"required":["score"]}"#;
        let first_bad_output = "this is definitely not json";
        let valid_response = "```json\n{\"score\": 1}\n```".to_string();

        let (mock_runner, captured) =
            AgentRunner::with_mock(vec![Ok(first_bad_output.to_string()), Ok(valid_response)]);

        let _ = Pipeline::new("Original prompt text", schema)
            .max_retries(1)
            .run(&[], mock_runner)
            .expect("should succeed on second attempt");

        let prompts = captured.lock().unwrap();
        assert_eq!(prompts.len(), 2, "expected 2 prompts (initial + retry)");

        let retry_prompt = &prompts[1];
        assert!(
            retry_prompt.contains("Original prompt text"),
            "retry prompt must contain the original prompt"
        );
        assert!(
            retry_prompt.contains("## Previous response"),
            "retry prompt must contain '## Previous response' header"
        );
        assert!(
            retry_prompt.contains(first_bad_output),
            "retry prompt must contain the previous raw output"
        );
        assert!(
            retry_prompt.contains("## Validation errors"),
            "retry prompt must contain '## Validation errors' header"
        );
        assert!(
            retry_prompt.contains("Please respond again"),
            "retry prompt must contain the instruction line"
        );
        assert!(
            retry_prompt.contains("```json block"),
            "retry prompt must mention the required json block format"
        );
    }

    // -----------------------------------------------------------------------
    // AC24–AC26: Pipeline::run() happy path
    // -----------------------------------------------------------------------

    #[test]
    fn test_pipeline_run_success_json_format() {
        let schema =
            r#"{"type":"object","properties":{"score":{"type":"integer"}},"required":["score"]}"#;
        let valid_response = "```json\n{\"score\": 42}\n```".to_string();
        let (mock_runner, _) = AgentRunner::with_mock(vec![Ok(valid_response)]);

        let result = Pipeline::new("test prompt", schema)
            .max_retries(0)
            .output_format(OutputFormat::Json)
            .run(&[], mock_runner)
            .expect("pipeline should succeed");

        assert_eq!(result.attempts, 1);
        assert!(result.report.contains("42"));
        assert_eq!(result.data["score"], 42);
    }

    #[test]
    fn test_pipeline_run_success_markdown_format() {
        let schema =
            r#"{"type":"object","properties":{"score":{"type":"integer"}},"required":["score"]}"#;
        let valid_response = "```json\n{\"score\": 5}\n```".to_string();
        let (mock_runner, _) = AgentRunner::with_mock(vec![Ok(valid_response)]);

        let result = Pipeline::new("test prompt", schema)
            .output_format(OutputFormat::Markdown)
            .report_template("Score: {{score}}")
            .run(&[], mock_runner)
            .expect("pipeline should succeed");

        assert_eq!(result.report, "Score: 5");
    }

    /// Integration test stub: full end-to-end pipeline with a real agent.
    /// Requires a Claude agent binary installed. Run with: cargo test --ignored
    #[test]
    #[ignore]
    fn integration_test_pipeline_run_end_to_end() {
        let schema = r#"{
            "type": "object",
            "properties": { "greeting": { "type": "string" } },
            "required": ["greeting"]
        }"#;
        let result = Pipeline::new(
            "Reply with a JSON object containing a single key 'greeting' with value 'hello'.",
            schema,
        )
        .max_retries(1)
        .output_format(OutputFormat::Json)
        .run(&[], AgentRunner::new().agent("claude"));

        assert!(
            result.is_ok(),
            "integration pipeline should succeed: {:?}",
            result.err()
        );
    }
}
