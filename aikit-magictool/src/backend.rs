#[cfg(feature = "agent")]
pub use agent_impl::*;

#[cfg(feature = "agent")]
mod agent_impl {
    use crate::core::{
        executor::{ChatEvent, ExecError, ToolChat, ToolExecutor},
        tool::{compose_prompt, ToolDef},
    };
    use aikit_sdk::{
        agent_runner::AgentRunner,
        pipeline::{Pipeline, PipelineError},
        runner::{AgentEvent, AgentEventPayload, RunOptions},
    };

    // ── PipelineExecutor ───────────────────────────────────────────────────────

    pub struct PipelineExecutor;

    impl ToolExecutor for PipelineExecutor {
        fn execute(
            &self,
            tool: &ToolDef,
            input: &serde_json::Value,
        ) -> Result<serde_json::Value, ExecError> {
            let prompt = compose_prompt(tool, input);
            let schema_str = serde_json::to_string(&tool.output_schema)
                .map_err(|e| ExecError::Internal(format!("schema serialization failed: {e}")))?;

            let result = Pipeline::new(prompt, schema_str)
                .max_retries(2)
                .run(&[], AgentRunner::new().agent(&tool.agent_key))
                .map_err(map_pipeline_error)?;

            Ok(result.data)
        }
    }

    // ── SessionChat ────────────────────────────────────────────────────────────

    pub struct SessionChat;

    impl ToolChat for SessionChat {
        fn run_turn(
            &self,
            tool: &ToolDef,
            session_id: Option<&str>,
            msg: &str,
            sink: &mut dyn FnMut(ChatEvent),
        ) -> Result<(), ExecError> {
            let options = {
                let mut opts = RunOptions::default();
                if let Some(sid) = session_id {
                    opts = opts.with_session_id(sid.to_owned());
                }
                opts
            };

            // Collect events into a buffer first (the callback must be Send,
            // so we cannot capture `sink` directly).
            let collected = std::sync::Arc::new(std::sync::Mutex::new(Vec::<ChatEvent>::new()));
            let collected_clone = collected.clone();

            aikit_sdk::runner::run_agent_events(
                &tool.agent_key,
                msg,
                options,
                move |event: AgentEvent| {
                    if let Some(chat_event) = map_agent_event(&event) {
                        collected_clone.lock().unwrap().push(chat_event);
                    }
                },
            )
            .map_err(|e| ExecError::AgentFailed(e.to_string()))?;

            for event in collected.lock().unwrap().drain(..) {
                sink(event);
            }

            Ok(())
        }

        fn finalize(
            &self,
            tool: &ToolDef,
            session_id: &str,
        ) -> Result<serde_json::Value, ExecError> {
            let schema_pretty = serde_json::to_string_pretty(&tool.output_schema)
                .map_err(|e| ExecError::Internal(format!("schema serialization failed: {e}")))?;
            let extraction_prompt = format!(
                "## Output\nEmit the final result as a single ```json block matching the schema:\n{schema_pretty}"
            );

            let mut collected_events: Vec<AgentEvent> = Vec::new();
            let options = RunOptions::default().with_session_id(session_id.to_owned());

            aikit_sdk::runner::run_agent_events(
                &tool.agent_key,
                &extraction_prompt,
                options,
                |event: AgentEvent| {
                    collected_events.push(event);
                },
            )
            .map_err(|e| ExecError::AgentFailed(e.to_string()))?;

            // Extract the final text and parse JSON from it
            let final_text = collected_events
                .iter()
                .rev()
                .find_map(|e| match &e.payload {
                    AgentEventPayload::AikitTextFinal { content, .. } => Some(content.as_str()),
                    _ => None,
                })
                .unwrap_or("");

            let json_text = extract_json_block(final_text);
            let value: serde_json::Value = serde_json::from_str(&json_text)
                .map_err(|e| ExecError::OutputInvalid(format!("JSON parse error: {e}")))?;

            if let Err(errors) = tool.validate_output(&value) {
                return Err(ExecError::OutputInvalid(errors.join("; ")));
            }

            Ok(value)
        }
    }

    fn extract_json_block(text: &str) -> String {
        if let Some(start) = text.find("```json") {
            let after = &text[start + 7..];
            if let Some(end) = after.find("```") {
                return after[..end].trim().to_owned();
            }
        }
        text.trim().to_owned()
    }

    // ── Pure mapping functions (unit-testable) ─────────────────────────────────

    pub(crate) fn map_pipeline_error(e: PipelineError) -> ExecError {
        match e {
            PipelineError::AgentInvocation { source } => ExecError::AgentFailed(source.to_string()),
            PipelineError::ValidationFailed { errors, .. } => {
                ExecError::OutputInvalid(errors.join("; "))
            }
            PipelineError::MaxRetriesExceeded { last_error } => {
                ExecError::OutputInvalid(format!("max retries exceeded: {last_error}"))
            }
            PipelineError::TemplateSlotMissing { slot } => {
                ExecError::Internal(format!("template slot missing: {slot}"))
            }
            PipelineError::ReportRender { slot } => {
                ExecError::Internal(format!("report render slot missing: {slot}"))
            }
        }
    }

    pub(crate) fn map_agent_event(event: &AgentEvent) -> Option<ChatEvent> {
        match &event.payload {
            AgentEventPayload::AikitTextDelta { content, .. } => {
                Some(ChatEvent::Delta(content.clone()))
            }
            AgentEventPayload::AikitTextFinal { content, .. } => {
                Some(ChatEvent::Final(content.clone()))
            }
            AgentEventPayload::SessionStarted { session_id } => Some(ChatEvent::Started {
                session_id: session_id.clone(),
            }),
            _ => None,
        }
    }

    pub fn extract_session_id(events: &[AgentEvent]) -> Option<String> {
        events.iter().find_map(|e| {
            if let AgentEventPayload::SessionStarted { session_id } = &e.payload {
                Some(session_id.clone())
            } else {
                None
            }
        })
    }

    // ── Unit tests for pure mapping functions ──────────────────────────────────

    #[cfg(test)]
    mod tests {
        use super::*;
        use aikit_sdk::runner::{AgentEventStream, RunError};

        fn make_event(payload: AgentEventPayload) -> AgentEvent {
            AgentEvent {
                agent_key: "aikit".to_string(),
                seq: 0,
                stream: AgentEventStream::Stdout,
                payload,
            }
        }

        #[test]
        fn test_map_pipeline_error_agent_invocation() {
            let err = PipelineError::AgentInvocation {
                source: RunError::AgentNotRunnable("test".to_string()),
            };
            assert!(matches!(map_pipeline_error(err), ExecError::AgentFailed(_)));
        }

        #[test]
        fn test_map_pipeline_error_validation_failed() {
            let err = PipelineError::ValidationFailed {
                raw_output: String::new(),
                errors: vec!["e".to_string()],
            };
            assert!(matches!(
                map_pipeline_error(err),
                ExecError::OutputInvalid(_)
            ));
        }

        #[test]
        fn test_map_pipeline_error_max_retries() {
            let err = PipelineError::MaxRetriesExceeded {
                last_error: Box::new(PipelineError::ValidationFailed {
                    raw_output: String::new(),
                    errors: vec!["e".to_string()],
                }),
            };
            assert!(matches!(
                map_pipeline_error(err),
                ExecError::OutputInvalid(_)
            ));
        }

        #[test]
        fn test_map_pipeline_error_template_slot_missing() {
            let err = PipelineError::TemplateSlotMissing {
                slot: "x".to_string(),
            };
            assert!(matches!(map_pipeline_error(err), ExecError::Internal(_)));
        }

        #[test]
        fn test_map_pipeline_error_report_render() {
            let err = PipelineError::ReportRender {
                slot: "x".to_string(),
            };
            assert!(matches!(map_pipeline_error(err), ExecError::Internal(_)));
        }

        #[test]
        fn test_map_agent_event_text_delta() {
            let event = make_event(AgentEventPayload::AikitTextDelta {
                content: "hello".to_string(),
                turn_id: None,
            });
            assert!(matches!(map_agent_event(&event), Some(ChatEvent::Delta(_))));
        }

        #[test]
        fn test_map_agent_event_text_final() {
            let event = make_event(AgentEventPayload::AikitTextFinal {
                content: "world".to_string(),
                turn_id: None,
            });
            assert!(matches!(map_agent_event(&event), Some(ChatEvent::Final(_))));
        }

        #[test]
        fn test_map_agent_event_session_started() {
            let event = make_event(AgentEventPayload::SessionStarted {
                session_id: "abc-123".to_string(),
            });
            assert!(matches!(
                map_agent_event(&event),
                Some(ChatEvent::Started { .. })
            ));
        }

        #[test]
        fn test_map_agent_event_other_returns_none() {
            let event = make_event(AgentEventPayload::RawLine("raw".to_string()));
            assert!(map_agent_event(&event).is_none());
        }

        #[test]
        fn test_extract_session_id() {
            let events = vec![make_event(AgentEventPayload::SessionStarted {
                session_id: "abc-123".to_string(),
            })];
            assert_eq!(extract_session_id(&events), Some("abc-123".to_string()));
        }

        #[test]
        fn test_extract_session_id_none() {
            let events = vec![make_event(AgentEventPayload::RawLine("x".to_string()))];
            assert_eq!(extract_session_id(&events), None);
        }
    }
}
