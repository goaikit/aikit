use aikit_magictool::{MagicToolState, ToolDef, ToolRegistry};
use serde_json::json;

pub fn fixture_tool() -> ToolDef {
    ToolDef::new(
        "test",
        "form_fill",
        "Test fixture: exercises all widget types",
        "Fill in all fields based on the input. Be accurate.",
        json!({
            "type": "object",
            "properties": {
                "raw_text": { "type": "string", "description": "Unstructured input" }
            },
            "required": ["raw_text"],
            "additionalProperties": false
        }),
        json!({
            "type": "object",
            "properties": {
                "title":      { "type": "string" },
                "notes":      { "type": "string", "format": "textarea" },
                "active":     { "type": "boolean" },
                "priority":   { "type": "integer", "minimum": 1, "maximum": 5 },
                "status": {
                    "oneOf": [
                        { "type": "string", "const": "open",   "title": "Open" },
                        { "type": "string", "const": "closed", "title": "Closed" }
                    ]
                },
                "tags":       { "type": "array", "items": { "type": "string" } }
            },
            "required": ["title", "active", "priority", "status", "tags"],
            "additionalProperties": false
        }),
    )
}

#[cfg(feature = "agent")]
pub fn fixture_state() -> MagicToolState {
    let mut registry = ToolRegistry::new();
    registry.register(fixture_tool());
    aikit_magictool::state_with_registry(registry)
}
