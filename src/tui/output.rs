//! Output formatting utilities

/// Tree item for displaying hierarchical data
pub struct TreeItem {
    pub label: String,
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

/// Format panel (stub implementation)
pub fn format_panel(_title: &str, _content: &str) -> String {
    // TODO: Implement panel formatting
    format!("=== {} ===\n{}", _title, _content)
}

/// Format table
pub fn format_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    if headers.is_empty() || rows.is_empty() {
        return String::new();
    }

    // Calculate column widths
    let mut col_widths = vec![0; headers.len()];
    for (i, header) in headers.iter().enumerate() {
        col_widths[i] = col_widths[i].max(header.len());
    }
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_widths.len() {
                col_widths[i] = col_widths[i].max(cell.len());
            }
        }
    }

    // Build the table
    let mut output = Vec::new();

    // Header row
    let header_line: Vec<String> = headers
        .iter()
        .enumerate()
        .map(|(i, header)| format!("{:<width$}", header, width = col_widths[i]))
        .collect();
    output.push(header_line.join("  "));

    // Separator
    let separator: String = col_widths
        .iter()
        .map(|&width| "-".repeat(width))
        .collect::<Vec<_>>()
        .join("  ");
    output.push(separator);

    // Data rows
    for row in rows {
        let row_line: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                if i < col_widths.len() {
                    format!("{:<width$}", cell, width = col_widths[i])
                } else {
                    cell.clone()
                }
            })
            .collect();
        output.push(row_line.join("  "));
    }

    output.join("\n")
}
