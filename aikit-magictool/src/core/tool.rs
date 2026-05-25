use serde::{Deserialize, Serialize};
use std::sync::Arc;

fn default_agent_key() -> String {
    "aikit".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub namespace: String,
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    #[serde(default = "default_agent_key")]
    pub agent_key: String,
    #[serde(skip)]
    pub(crate) input_validator: Option<Arc<jsonschema::Validator>>,
    #[serde(skip)]
    pub(crate) output_validator: Option<Arc<jsonschema::Validator>>,
}

impl ToolDef {
    pub fn new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        instructions: impl Into<String>,
        input_schema: serde_json::Value,
        output_schema: serde_json::Value,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
            description: description.into(),
            instructions: instructions.into(),
            input_schema,
            output_schema,
            agent_key: default_agent_key(),
            input_validator: None,
            output_validator: None,
        }
    }

    pub fn with_agent_key(mut self, key: impl Into<String>) -> Self {
        self.agent_key = key.into();
        self
    }

    pub(crate) fn compile_validators(&mut self) {
        let iv = jsonschema::validator_for(&self.input_schema).unwrap_or_else(|e| {
            panic!(
                "invalid input_schema for {}/{}: {}",
                self.namespace, self.name, e
            )
        });
        let ov = jsonschema::validator_for(&self.output_schema).unwrap_or_else(|e| {
            panic!(
                "invalid output_schema for {}/{}: {}",
                self.namespace, self.name, e
            )
        });
        self.input_validator = Some(Arc::new(iv));
        self.output_validator = Some(Arc::new(ov));
    }

    pub fn validate_input(&self, value: &serde_json::Value) -> Result<(), Vec<String>> {
        if let Some(v) = &self.input_validator {
            let errors: Vec<String> = v.iter_errors(value).map(|e| e.to_string()).collect();
            if errors.is_empty() {
                Ok(())
            } else {
                Err(errors)
            }
        } else {
            Ok(())
        }
    }

    pub fn validate_output(&self, value: &serde_json::Value) -> Result<(), Vec<String>> {
        if let Some(v) = &self.output_validator {
            let errors: Vec<String> = v.iter_errors(value).map(|e| e.to_string()).collect();
            if errors.is_empty() {
                Ok(())
            } else {
                Err(errors)
            }
        } else {
            Ok(())
        }
    }
}

pub fn compose_prompt(tool: &ToolDef, input: &serde_json::Value) -> String {
    let mut s = tool.instructions.clone();
    s.push_str("\n\n## Input\n");
    s.push_str(&serde_json::to_string_pretty(input).unwrap_or_default());
    s.push_str("\n\n## Output\nRespond with a single ```json block conforming to this schema:\n");
    s.push_str(&serde_json::to_string_pretty(&tool.output_schema).unwrap_or_default());
    s
}
