use super::tool::ToolDef;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct ToolListEntry {
    pub namespace: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Default)]
pub struct ToolRegistry {
    tools: HashMap<(String, String), ToolDef>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, mut def: ToolDef) {
        let key = (def.namespace.clone(), def.name.clone());
        assert!(
            !self.tools.contains_key(&key),
            "duplicate tool: {}/{}",
            def.namespace,
            def.name
        );
        def.compile_validators();
        self.tools.insert(key, def);
    }

    pub fn get(&self, ns: &str, name: &str) -> Option<&ToolDef> {
        self.tools.get(&(ns.to_owned(), name.to_owned()))
    }

    pub fn list(&self) -> Vec<ToolListEntry> {
        let mut entries: Vec<ToolListEntry> = self
            .tools
            .values()
            .map(|def| ToolListEntry {
                namespace: def.namespace.clone(),
                name: def.name.clone(),
                description: def.description.clone(),
            })
            .collect();
        entries.sort_by(|a, b| a.namespace.cmp(&b.namespace).then(a.name.cmp(&b.name)));
        entries
    }
}
