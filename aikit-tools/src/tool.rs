use crate::error::ToolsError;
use crate::schema;
use jsonschema::Validator;
use serde_json::{json, Value};

pub struct AgentTool {
    pub namespace: String,
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub system_prompt: String,
    pub(crate) input_validator: Validator,
    pub(crate) output_validator: Validator,
}

impl AgentTool {
    pub fn new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        output_schema: Value,
        system_prompt: impl Into<String>,
    ) -> Result<Self, ToolsError> {
        let input_validator = schema::compile(&input_schema)?;
        let output_validator = schema::compile(&output_schema)?;
        Ok(Self {
            namespace: namespace.into(),
            name: name.into(),
            description: description.into(),
            input_schema,
            output_schema,
            system_prompt: system_prompt.into(),
            input_validator,
            output_validator,
        })
    }

    pub fn key(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }

    pub fn schema_doc(&self) -> Value {
        json!({
            "namespace": self.namespace,
            "name": self.name,
            "description": self.description,
            "inputSchema": self.input_schema,
            "outputSchema": self.output_schema,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_tool() -> AgentTool {
        AgentTool::new(
            "ns",
            "name",
            "desc",
            json!({"type": "object", "properties": {"x": {"type": "string"}}, "required": ["x"]}),
            json!({"type": "object", "properties": {"y": {"type": "string"}}, "required": ["y"]}),
            "system prompt",
        )
        .unwrap()
    }

    #[test]
    fn new_bad_schema_returns_err() {
        let result = AgentTool::new(
            "ns",
            "name",
            "desc",
            json!({"required": "not-an-array"}),
            json!({"type": "object"}),
            "prompt",
        );
        assert!(result.is_err(), "expected Err for invalid input schema");
    }

    #[test]
    fn key_format() {
        let tool = make_tool();
        assert_eq!(tool.key(), "ns/name");
    }

    #[test]
    fn schema_doc_contains_expected_keys() {
        let tool = make_tool();
        let doc = tool.schema_doc();
        assert!(doc.get("inputSchema").is_some());
        assert!(doc.get("outputSchema").is_some());
        assert!(doc.get("description").is_some());
        assert!(doc.get("namespace").is_some());
        assert!(doc.get("name").is_some());
    }
}
