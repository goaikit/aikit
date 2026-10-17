//! Shared strict JSON-object parsing for deployment formats.
use serde_json::Value;

pub(crate) enum JsonObjectError {
    Json(serde_json::Error),
    NotObject,
}

pub(crate) fn parse_object(bytes: &[u8]) -> Result<Value, JsonObjectError> {
    let value: Value = serde_json::from_slice(bytes).map_err(JsonObjectError::Json)?;
    if !value.is_object() {
        return Err(JsonObjectError::NotObject);
    }
    Ok(value)
}
