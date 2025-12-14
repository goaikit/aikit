//! Formatted output utilities
//!
//! This module provides formatted output functions matching Python Rich library
//! formatting (panels, tables, trees).

/// Format a panel with title and content
pub fn format_panel(title: &str, content: &str) -> String {
    format!(
        "┌─ {} ─┐\n{}\n└{}┘",
        title,
        content,
        "─".repeat(title.len() + 4)
    )
}

/// Format a table
pub fn format_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut output = Vec::new();

    // Header row
    let header_row = headers.join(" | ");
    output.push(format!("| {} |", header_row));

    // Separator
    let separator = headers
        .iter()
        .map(|_| "---")
        .collect::<Vec<_>>()
        .join(" | ");
    output.push(format!("| {} |", separator));

    // Data rows
    for row in rows {
        let row_str = row.join(" | ");
        output.push(format!("| {} |", row_str));
    }

    output.join("\n")
}

/// Tree item for formatting
#[derive(Debug, Clone)]
pub struct TreeItem {
    pub label: String,
    pub children: Vec<TreeItem>,
}

impl TreeItem {
    /// Create a new tree item
    pub fn new(label: String) -> Self {
        Self {
            label,
            children: Vec::new(),
        }
    }

    /// Create a tree item with children
    pub fn with_children(label: String, children: Vec<TreeItem>) -> Self {
        Self { label, children }
    }
}

/// Format a tree structure
pub fn format_tree(items: &[TreeItem]) -> String {
    let mut output = Vec::new();
    format_tree_recursive(items, &mut output, "", true);
    output.join("\n")
}

fn format_tree_recursive(
    items: &[TreeItem],
    output: &mut Vec<String>,
    prefix: &str,
    is_last: bool,
) {
    for (i, item) in items.iter().enumerate() {
        let is_last_item = i == items.len() - 1;
        let connector = if is_last_item {
            "└── "
        } else {
            "├── "
        };
        output.push(format!("{}{}{}", prefix, connector, item.label));

        let new_prefix = if is_last {
            format!("{}    ", prefix)
        } else {
            format!("{}│   ", prefix)
        };

        if !item.children.is_empty() {
            format_tree_recursive(&item.children, output, &new_prefix, is_last_item);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_panel() {
        let panel = format_panel("Title", "Content");
        assert!(panel.contains("Title"));
        assert!(panel.contains("Content"));
    }

    #[test]
    fn test_format_table() {
        let headers = vec!["Name", "Status"];
        let rows = vec![
            vec!["Item 1".to_string(), "OK".to_string()],
            vec!["Item 2".to_string(), "FAIL".to_string()],
        ];
        let table = format_table(&headers, &rows);
        assert!(table.contains("Name"));
        assert!(table.contains("Item 1"));
    }
}
