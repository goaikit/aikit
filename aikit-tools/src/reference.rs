use serde_json::json;

use crate::tool::AgentTool;

pub fn draft_contact_tool() -> AgentTool {
    AgentTool::new(
        "crm",
        "draft_contact",
        "CRM contact data-entry assistant",
        json!({
            "type": "object",
            "properties": {
                "note": { "type": "string" },
                "partial": { "type": "object" }
            },
            "required": ["note"]
        }),
        json!({
            "type": "object",
            "properties": {
                "first_name": { "type": "string" },
                "last_name": { "type": "string" },
                "email": { "type": "string" },
                "company": { "type": "string" },
                "title": { "type": "string" },
                "notes": { "type": "string" }
            },
            "required": ["first_name", "last_name"],
            "additionalProperties": false
        }),
        "You are a CRM contact data-entry assistant. Extract contact information from the provided note and any partial data, then call emit_output exactly once with the structured contact draft.",
    )
    .expect("draft_contact_tool schema must be valid")
}
