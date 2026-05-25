use crate::error::ToolsError;
use jsonschema::Validator;
use serde_json::Value;

pub fn compile(schema: &Value) -> Result<Validator, ToolsError> {
    jsonschema::validator_for(schema)
        .map_err(|e| ToolsError::Internal(format!("invalid JSON Schema: {e}")))
}

pub fn validate(validator: &Validator, instance: &Value) -> Result<(), String> {
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|e| e.to_string())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compile_valid_schema() {
        let schema =
            json!({"type": "object", "properties": {"x": {"type": "string"}}, "required": ["x"]});
        assert!(compile(&schema).is_ok());
    }

    #[test]
    fn compile_invalid_schema() {
        let schema = json!({"type": "not-a-real-type-that-errors"});
        let schema2 = json!({"required": "not-an-array"});
        let _ = compile(&schema);
        let _ = compile(&schema2);
    }

    #[test]
    fn validate_conforming_instance() {
        let schema =
            json!({"type": "object", "properties": {"x": {"type": "string"}}, "required": ["x"]});
        let validator = compile(&schema).unwrap();
        let instance = json!({"x": "hello"});
        assert!(validate(&validator, &instance).is_ok());
    }

    #[test]
    fn validate_non_conforming_instance() {
        let schema =
            json!({"type": "object", "properties": {"x": {"type": "string"}}, "required": ["x"]});
        let validator = compile(&schema).unwrap();
        let instance = json!({"y": "hello"});
        let result = validate(&validator, &instance);
        assert!(result.is_err());
        assert!(!result.unwrap_err().is_empty());
    }
}
