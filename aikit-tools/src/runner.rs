use std::env;
use std::sync::Arc;

use aikit_agent::{
    AgentConfig, AgentInternalEvent, AgentPersona, HostToolDefinition, HostToolProvider,
};
use serde_json::Value;

use crate::error::ToolsError;

pub trait AgentRunner: Send + Sync {
    fn run(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        output_schema: &Value,
    ) -> Result<Vec<AgentInternalEvent>, ToolsError>;
}

struct EmitOutputProvider {
    output_schema: Value,
}

impl HostToolProvider for EmitOutputProvider {
    fn list_tools(&self) -> Vec<HostToolDefinition> {
        vec![HostToolDefinition {
            name: "emit_output".to_string(),
            description: Some(
                "Submit the final structured Draft. Call exactly once, then stop.".to_string(),
            ),
            parameters: self.output_schema.clone(),
        }]
    }

    fn call_tool(&self, _name: &str, _arguments: Value) -> Result<String, String> {
        Ok("OUTPUT_RECORDED. Draft captured. Stop now.".to_string())
    }
}

pub struct ProductionAgentRunner;

impl AgentRunner for ProductionAgentRunner {
    fn run(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        output_schema: &Value,
    ) -> Result<Vec<AgentInternalEvent>, ToolsError> {
        let workdir = env::current_dir()
            .map_err(|e| ToolsError::Internal(format!("cannot determine workdir: {e}")))?;

        let mut config = AgentConfig::from_env(workdir, false, None)
            .map_err(|e| ToolsError::Internal(format!("agent config error: {e}")))?;

        config.session_persona = Some(AgentPersona {
            name: "aikit-tools-agent".to_string(),
            description: "Tool invocation agent".to_string(),
            prompt: system_prompt.to_string(),
            tools: None,
            disallowed_tools: None,
            model: None,
        });

        config.host_tool_provider = Some(Arc::new(EmitOutputProvider {
            output_schema: output_schema.clone(),
        }));

        let gateway = aikit_agent::llm::openai_compat::OpenAiCompatProvider::new(
            config.timeout_secs,
            config.connect_timeout_secs,
        )
        .map_err(|e| ToolsError::Internal(format!("gateway error: {e}")))?;

        aikit_agent::run(config, user_prompt, Box::new(gateway))
            .map_err(|e| ToolsError::AgentFailed(format!("{e}")))
    }
}

pub struct MockRunner {
    pub canned: Vec<AgentInternalEvent>,
}

impl AgentRunner for MockRunner {
    fn run(
        &self,
        _system_prompt: &str,
        _user_prompt: &str,
        _output_schema: &Value,
    ) -> Result<Vec<AgentInternalEvent>, ToolsError> {
        Ok(self.canned.clone())
    }
}

pub fn capture_output(events: &[AgentInternalEvent]) -> Result<Value, ToolsError> {
    // Step 1: Error event takes precedence
    for event in events {
        if let AgentInternalEvent::Error { message, .. } = event {
            return Err(ToolsError::AgentFailed(message.clone()));
        }
    }

    // Step 2: Last emit_output ToolUse
    let mut last_emit: Option<&Value> = None;
    for event in events {
        if let AgentInternalEvent::ToolUse {
            tool_name,
            tool_input,
            ..
        } = event
        {
            if tool_name == "emit_output" {
                last_emit = Some(tool_input);
            }
        }
    }
    if let Some(value) = last_emit {
        return Ok(value.clone());
    }

    // Step 3: Last TextFinal parseable as JSON
    let mut last_text_final: Option<&str> = None;
    for event in events {
        if let AgentInternalEvent::TextFinal { content, .. } = event {
            last_text_final = Some(content.as_str());
        }
    }
    if let Some(text) = last_text_final {
        let stripped = strip_json_fences(text);
        if let Ok(value) = serde_json::from_str::<Value>(stripped) {
            return Ok(value);
        }
    }

    // Step 4: No output
    Err(ToolsError::AgentNoOutput)
}

fn strip_json_fences(s: &str) -> &str {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix("```json") {
        if let Some(inner2) = inner.strip_suffix("```") {
            return inner2.trim();
        }
    }
    if let Some(inner) = s.strip_prefix("```") {
        if let Some(inner2) = inner.strip_suffix("```") {
            return inner2.trim();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_error_event() -> AgentInternalEvent {
        AgentInternalEvent::Error {
            code: "x".to_string(),
            message: "y".to_string(),
        }
    }

    fn make_emit_output(value: Value) -> AgentInternalEvent {
        AgentInternalEvent::ToolUse {
            tool_name: "emit_output".to_string(),
            tool_input: value,
            call_id: "c1".to_string(),
        }
    }

    fn make_text_final(content: &str) -> AgentInternalEvent {
        AgentInternalEvent::TextFinal {
            content: content.to_string(),
            turn_id: None,
        }
    }

    #[test]
    fn empty_events_returns_no_output() {
        let result = capture_output(&[]);
        assert!(matches!(result, Err(ToolsError::AgentNoOutput)));
    }

    #[test]
    fn error_event_returns_agent_failed() {
        let events = vec![make_error_event()];
        let result = capture_output(&events);
        assert!(matches!(result, Err(ToolsError::AgentFailed(_))));
    }

    #[test]
    fn emit_output_tool_use_returns_value() {
        let events = vec![make_emit_output(json!({"a": 1}))];
        let result = capture_output(&events).unwrap();
        assert_eq!(result, json!({"a": 1}));
    }

    #[test]
    fn text_final_json_returns_value() {
        let events = vec![make_text_final(r#"{"a":1}"#)];
        let result = capture_output(&events).unwrap();
        assert_eq!(result, json!({"a": 1}));
    }

    #[test]
    fn text_final_with_json_fences_returns_value() {
        let events = vec![make_text_final("```json\n{\"a\":1}\n```")];
        let result = capture_output(&events).unwrap();
        assert_eq!(result, json!({"a": 1}));
    }

    #[test]
    fn multiple_emit_outputs_last_wins() {
        let events = vec![
            make_emit_output(json!({"a": 1})),
            make_emit_output(json!({"a": 2})),
        ];
        let result = capture_output(&events).unwrap();
        assert_eq!(result, json!({"a": 2}));
    }

    #[test]
    fn error_takes_precedence_over_emit_output() {
        let events = vec![make_emit_output(json!({"a": 1})), make_error_event()];
        let result = capture_output(&events);
        assert!(matches!(result, Err(ToolsError::AgentFailed(_))));
    }
}
