//! Interactive agent selection UI

/// Result of agent selection
pub enum SelectionResult {
    Selected(String),
    Cancelled,
}

/// Select agent interactively (stub implementation)
pub fn select_agent_interactive() -> Result<SelectionResult, Box<dyn std::error::Error>> {
    // For now, return Claude as default
    // TODO: Implement actual interactive selection
    Ok(SelectionResult::Selected("claude".to_string()))
}
