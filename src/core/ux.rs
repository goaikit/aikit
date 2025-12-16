//! User experience utilities for the AIKIT CLI
//!
//! This module provides utilities for progress indicators, interactive prompts,
//! and other UX improvements.

use crate::error::AikError;
use indicatif::{ProgressBar, ProgressStyle};
use std::fmt::Display;

/// Create a progress bar for long operations
pub fn create_progress_bar(total: u64, message: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb.set_message(message.to_string());
    pb
}

/// Create a spinner for indeterminate progress
pub fn create_spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    pb.set_message(message.to_string());
    pb
}

/// Show a confirmation prompt
pub fn confirm_action(prompt: &str) -> Result<bool, AikError> {
    if !atty::is(atty::Stream::Stdout) {
        // If not interactive, default to yes for non-destructive operations
        // For destructive operations, this should be handled differently
        return Ok(true);
    }

    dialoguer::Confirm::new()
        .with_prompt(prompt)
        .default(false)
        .interact()
        .map_err(|e| AikError::Generic(format!("Confirmation prompt failed: {}", e)))
}

/// Select from a list of options
pub fn select_from_list<T: Display>(items: &[T], prompt: &str) -> Result<usize, AikError> {
    if !atty::is(atty::Stream::Stdout) {
        return Err(AikError::Generic("Cannot show interactive selection in non-interactive mode".to_string()));
    }

    let selection = dialoguer::Select::new()
        .with_prompt(prompt)
        .items(items)
        .default(0)
        .interact()
        .map_err(|e| AikError::Generic(format!("Selection prompt failed: {}", e)))?;

    Ok(selection)
}

/// Show a success message
pub fn show_success(message: &str) {
    println!("✅ {}", message);
}

/// Show a warning message
pub fn show_warning(message: &str) {
    eprintln!("⚠️  {}", message);
}

/// Show an info message
pub fn show_info(message: &str) {
    println!("ℹ️  {}", message);
}
