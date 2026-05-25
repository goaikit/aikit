//! JSON response validation using jsonschema.
//!
//! Extracts the first ```json block from a response (falling back to bare JSON),
//! then validates it against a JSON Schema.

use crate::pipeline::PipelineError;

/// A successfully validated agent response.
#[derive(Debug, Clone)]
pub struct ValidatedResponse {
    /// The parsed JSON data.
    pub data: serde_json::Value,
    /// The raw string that was parsed (the extracted JSON text).
    pub raw: String,
}

/// Validates agent text responses against a JSON Schema.
pub struct ResponseValidator;

impl ResponseValidator {
    /// Extract and validate `text` against `schema_str`.
    ///
    /// Extraction: searches for a ` ```json ` fenced block (newline after fence
    /// is permitted). Falls back to treating the entire string as bare JSON.
    ///
    /// Validation: uses `jsonschema::validator_for` to build a validator from
    /// `schema_str`, then collects all validation errors.
    pub fn validate(text: &str, schema_str: &str) -> Result<ValidatedResponse, PipelineError> {
        // --- Extract JSON text ---
        let json_text = Self::extract_json(text);

        // --- Parse JSON ---
        let data: serde_json::Value =
            serde_json::from_str(&json_text).map_err(|e| PipelineError::ValidationFailed {
                raw_output: text.to_string(),
                errors: vec![format!("JSON parse error: {}", e)],
            })?;

        // --- Parse schema ---
        let schema_value: serde_json::Value =
            serde_json::from_str(schema_str).map_err(|e| PipelineError::ValidationFailed {
                raw_output: text.to_string(),
                errors: vec![format!("Schema parse error: {}", e)],
            })?;

        let validator = jsonschema::validator_for(&schema_value).map_err(|e| {
            PipelineError::ValidationFailed {
                raw_output: text.to_string(),
                errors: vec![format!("Schema compilation error: {}", e)],
            }
        })?;

        // --- Validate ---
        let error_messages: Vec<String> = validator
            .iter_errors(&data)
            .map(|e| e.to_string())
            .collect();

        if error_messages.is_empty() {
            Ok(ValidatedResponse {
                data,
                raw: json_text,
            })
        } else {
            Err(PipelineError::ValidationFailed {
                raw_output: text.to_string(),
                errors: error_messages,
            })
        }
    }

    /// Extract the first ```json block from `text`, or return `text` as-is.
    fn extract_json(text: &str) -> String {
        // Look for ```json (optionally followed by a newline)
        if let Some(start_idx) = find_json_fence_start(text) {
            // Find the closing ```
            let after_open = &text[start_idx..];
            if let Some(close_offset) = after_open.find("```") {
                let json_content = &after_open[..close_offset];
                return json_content.trim().to_string();
            }
        }
        // Fallback: bare JSON
        text.trim().to_string()
    }
}

/// Find the start of the content inside the first ```json fence.
///
/// Returns the byte index within `text` where the JSON content begins
/// (i.e., right after the opening fence line).
fn find_json_fence_start(text: &str) -> Option<usize> {
    // Search for "```json" optionally preceded by whitespace on a new segment
    let marker = "```json";
    if let Some(pos) = text.find(marker) {
        let after_marker = pos + marker.len();
        // Skip optional newline (LF or CRLF) after the fence
        let content_start = if text[after_marker..].starts_with("\r\n") {
            after_marker + 2
        } else if text[after_marker..].starts_with('\n') {
            after_marker + 1
        } else {
            after_marker
        };
        return Some(content_start);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA_NAME_REQUIRED: &str = r#"{
        "type": "object",
        "properties": { "name": { "type": "string" } },
        "required": ["name"]
    }"#;

    #[test]
    fn test_valid_bare_json() {
        let result = ResponseValidator::validate(r#"{"name":"Alice"}"#, SCHEMA_NAME_REQUIRED);
        assert!(result.is_ok());
        let v = result.unwrap();
        assert_eq!(v.data["name"], "Alice");
    }

    #[test]
    fn test_valid_json_in_fence() {
        let text = "Here is the result:\n```json\n{\"name\":\"Bob\"}\n```\n";
        let result = ResponseValidator::validate(text, SCHEMA_NAME_REQUIRED);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().data["name"], "Bob");
    }

    #[test]
    fn test_valid_json_fence_no_newline() {
        let text = "```json{\"name\":\"Carol\"}```";
        let result = ResponseValidator::validate(text, SCHEMA_NAME_REQUIRED);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().data["name"], "Carol");
    }

    #[test]
    fn test_missing_required_field_fails() {
        let raw = r#"{"age": 42}"#;
        let err = ResponseValidator::validate(raw, SCHEMA_NAME_REQUIRED).unwrap_err();
        match err {
            PipelineError::ValidationFailed { errors, .. } => {
                assert!(!errors.is_empty());
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_invalid_json_parse_error() {
        let raw = "not json at all";
        let err = ResponseValidator::validate(raw, SCHEMA_NAME_REQUIRED).unwrap_err();
        match err {
            PipelineError::ValidationFailed { errors, .. } => {
                assert!(errors[0].contains("JSON parse error") || !errors.is_empty());
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_fence_content_returned_in_raw() {
        let text = "```json\n{\"name\":\"Dave\"}\n```";
        let v = ResponseValidator::validate(text, SCHEMA_NAME_REQUIRED).unwrap();
        assert_eq!(v.raw.trim(), r#"{"name":"Dave"}"#);
    }

    #[test]
    fn test_wrong_type_fails_validation() {
        let schema =
            r#"{"type":"object","properties":{"count":{"type":"integer"}},"required":["count"]}"#;
        let raw = r#"{"count": "not-an-integer"}"#;
        let err = ResponseValidator::validate(raw, schema).unwrap_err();
        assert!(matches!(err, PipelineError::ValidationFailed { .. }));
    }

    #[test]
    fn test_first_fence_used_when_multiple() {
        let text = "```json\n{\"name\":\"First\"}\n```\n```json\n{\"name\":\"Second\"}\n```";
        let v = ResponseValidator::validate(text, SCHEMA_NAME_REQUIRED).unwrap();
        assert_eq!(v.data["name"], "First");
    }
}
