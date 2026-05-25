//! Structured agent pipeline: template rendering → agent invocation → JSON validation
//! → optional report generation.

use crate::agent_runner::AgentRunner;
use crate::report::ReportRenderer;
use crate::runner::RunError;
use crate::template::TemplateRenderer;
use crate::validation::ResponseValidator;
use std::collections::HashMap;
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
    /// The rendered output (JSON or Markdown depending on `OutputFormat`).
    pub output: String,
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
    pub fn with_report_template(mut self, tmpl: impl Into<String>) -> Self {
        self.report_template = Some(tmpl.into());
        self
    }

    /// Set the maximum number of retry attempts.
    pub fn with_max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    /// Set the output format.
    pub fn with_output_format(mut self, fmt: OutputFormat) -> Self {
        self.output_format = fmt;
        self
    }

    /// Run the pipeline.
    ///
    /// # Arguments
    /// * `slots` - Values for template placeholders.
    /// * `runner` - An `AgentRunner` (consumed; takes `self`). Its configuration
    ///   is extracted before the retry loop so the agent key/model/dir can be
    ///   reused across attempts.
    pub fn run(
        &self,
        slots: &HashMap<String, String>,
        runner: AgentRunner,
    ) -> Result<PipelineResult, PipelineError> {
        // Step 1: render the template — fatal if a slot is missing
        let original_prompt = TemplateRenderer::render(&self.template, slots)?;

        // Decompose runner into config so we can rebuild per attempt
        let (agent_key, model, working_dir) = runner.into_parts();

        let mut attempt = 1u32;
        let mut current_prompt = original_prompt.clone();

        loop {
            // Rebuild runner for this attempt
            let mut attempt_runner = AgentRunner::new(agent_key.clone());
            if let Some(ref m) = model {
                attempt_runner = attempt_runner.with_model(m.clone());
            }
            if let Some(ref d) = working_dir {
                attempt_runner = attempt_runner.with_working_dir(d.clone());
            }

            // Step 2: call the agent
            let raw_text = attempt_runner
                .run_prompt(current_prompt.clone())
                .map_err(|source| PipelineError::AgentInvocation { source })?;

            // Step 3: validate
            match ResponseValidator::validate(&raw_text, &self.schema) {
                Ok(validated) => {
                    // Validation passed — render output
                    let output = match &self.output_format {
                        OutputFormat::Json => ReportRenderer::render_json(&validated.data)?,
                        OutputFormat::Markdown => {
                            let tmpl = self.report_template.as_deref().unwrap_or("");
                            ReportRenderer::render_markdown(tmpl, &validated.data)?
                        }
                    };
                    return Ok(PipelineResult {
                        output,
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

                    // Build augmented retry prompt
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
    fn test_template_render_missing_slot_is_fatal() {
        // TemplateRenderer errors flow through Pipeline's slot rendering step
        let result = TemplateRenderer::render("Hello {{missing}}", &HashMap::new());
        assert!(result.is_err());
    }

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

    #[test]
    fn test_pipeline_error_source_agent_invocation() {
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
}
