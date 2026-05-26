use aikit_magictool::ToolDef;
use serde_json::json;

pub fn draft_agent_definition_tool() -> ToolDef {
    ToolDef::new(
        "agents",
        "draft_definition",
        "Draft an aikit AgentDefinition from a plain-English description",
        "You are an aikit agent-definition author. Given a plain-English description, \
         draft a complete agent definition: a concise kebab-case `name`, a one-line \
         `description`, a clear system `prompt`, and a sensible `tools` allowlist. \
         Be concise and accurate.",
        json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "format": "textarea",
                    "description": "Plain-English description of the agent you want."
                }
            },
            "required": ["description"],
            "additionalProperties": false
        }),
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Kebab-case identifier for the agent"
                },
                "description": {
                    "type": "string",
                    "description": "One-line summary of what the agent does"
                },
                "prompt": {
                    "type": "string",
                    "format": "textarea",
                    "description": "System prompt for the agent"
                },
                "tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Allowlisted tools (optional)"
                },
                "disallowed_tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Blocked tools (optional)"
                },
                "model": {
                    "type": "string",
                    "description": "Model override (optional)"
                }
            },
            "required": ["name", "description", "prompt"],
            "additionalProperties": false
        }),
    )
}
