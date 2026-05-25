pub fn validate_value(
    validator: &jsonschema::Validator,
    value: &serde_json::Value,
) -> Result<(), Vec<String>> {
    let errors: Vec<String> = validator
        .iter_errors(value)
        .map(|e| e.to_string())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
