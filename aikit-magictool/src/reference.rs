use crate::core::tool::ToolDef;
use serde_json::json;

pub fn draft_lead_tool() -> ToolDef {
    ToolDef::new(
        "crm",
        "draft_lead",
        "Draft a CRM lead record from unstructured input",
        "You are a CRM assistant. Given the input data, draft a lead record with all available fields populated. Be concise and accurate.",
        json!({
            "type": "object",
            "properties": {
                "raw_text": {
                    "type": "string",
                    "description": "Unstructured text describing the lead"
                }
            },
            "required": ["raw_text"],
            "additionalProperties": false
        }),
        json!({
            "type": "object",
            "properties": {
                "first_name": {
                    "type": "string",
                    "description": "First name of the lead"
                },
                "last_name": {
                    "type": "string",
                    "description": "Last name of the lead"
                },
                "notes": {
                    "type": "string",
                    "format": "textarea",
                    "description": "Free-form notes about this lead"
                },
                "is_qualified": {
                    "type": "boolean",
                    "description": "Whether this lead is qualified"
                },
                "score": {
                    "type": "integer",
                    "description": "Lead score from 0 to 100",
                    "minimum": 0,
                    "maximum": 100
                },
                "status": {
                    "oneOf": [
                        {"type": "string", "const": "new", "title": "New"},
                        {"type": "string", "const": "contacted", "title": "Contacted"},
                        {"type": "string", "const": "qualified", "title": "Qualified"},
                        {"type": "string", "const": "lost", "title": "Lost"}
                    ],
                    "description": "Current lead status"
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tags associated with this lead"
                }
            },
            "required": ["first_name", "last_name", "is_qualified", "score", "status", "tags"],
            "additionalProperties": false
        }),
    )
}
