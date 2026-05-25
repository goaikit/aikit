use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;

use crate::tool::AgentTool;

pub struct ToolRegistry {
    tools: HashMap<String, Arc<AgentTool>>,
}

#[derive(Serialize)]
pub struct ToolListEntry {
    pub namespace: String,
    pub name: String,
    pub description: String,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: AgentTool) {
        let key = tool.key();
        if self.tools.contains_key(&key) {
            panic!("aikit-tools: duplicate tool registration for key '{key}'");
        }
        self.tools.insert(key, Arc::new(tool));
    }

    pub fn resolve(&self, ns: &str, name: &str) -> Option<Arc<AgentTool>> {
        let key = format!("{ns}/{name}");
        self.tools.get(&key).cloned()
    }

    pub fn list(&self) -> Vec<ToolListEntry> {
        let mut entries: Vec<ToolListEntry> = self
            .tools
            .values()
            .map(|t| ToolListEntry {
                namespace: t.namespace.clone(),
                name: t.name.clone(),
                description: t.description.clone(),
            })
            .collect();
        entries.sort_by(|a, b| a.namespace.cmp(&b.namespace).then(a.name.cmp(&b.name)));
        entries
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_tool(ns: &str, name: &str) -> AgentTool {
        AgentTool::new(
            ns,
            name,
            "desc",
            json!({"type": "object", "properties": {"x": {"type": "string"}}, "required": ["x"]}),
            json!({"type": "object", "properties": {"y": {"type": "string"}}, "required": ["y"]}),
            "system prompt",
        )
        .unwrap()
    }

    #[test]
    fn register_and_resolve() {
        let mut registry = ToolRegistry::new();
        registry.register(make_tool("crm", "draft_contact"));
        let tool = registry.resolve("crm", "draft_contact");
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().key(), "crm/draft_contact");
    }

    #[test]
    fn resolve_missing_returns_none() {
        let registry = ToolRegistry::new();
        assert!(registry.resolve("crm", "unknown").is_none());
    }

    #[test]
    fn list_returns_entries() {
        let mut registry = ToolRegistry::new();
        registry.register(make_tool("crm", "draft_contact"));
        let list = registry.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].namespace, "crm");
        assert_eq!(list[0].name, "draft_contact");
    }

    #[test]
    #[should_panic(expected = "duplicate tool registration")]
    fn register_duplicate_panics() {
        let mut registry = ToolRegistry::new();
        registry.register(make_tool("crm", "draft_contact"));
        registry.register(make_tool("crm", "draft_contact"));
    }
}
