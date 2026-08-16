//! Output formatting utilities

/// Tree item for displaying hierarchical data
pub struct TreeItem {
    pub label: String,
    #[allow(dead_code)]
    pub children: Vec<TreeItem>,
}

impl TreeItem {
    pub fn new(label: String) -> Self {
        Self {
            label,
            children: Vec::new(),
        }
    }
}

/// Format tree structure
pub fn format_tree(items: &[TreeItem]) -> String {
    items
        .iter()
        .map(|item| item.label.clone())
        .collect::<Vec<_>>()
        .join("\n")
}
