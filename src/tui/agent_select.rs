//! Interactive agent selection UI
//!
//! This module provides the interactive TUI for selecting an AI agent
//! when --ai flag is not provided.

use crate::core::agent::get_agent_configs;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use std::io;

/// Interactive agent selection result
pub enum SelectionResult {
    /// Agent key was selected
    Selected(String),
    /// Selection was cancelled
    Cancelled,
}

/// Show interactive agent selection UI
pub fn select_agent_interactive() -> Result<SelectionResult> {
    let agents = get_agent_configs();
    let default_index = agents.iter().position(|a| a.key == "copilot").unwrap_or(0);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = ListState::default();
    state.select(Some(default_index));

    let result = loop {
        terminal.draw(|f| {
            let size = f.size();
            let items: Vec<ListItem> = agents
                .iter()
                .enumerate()
                .map(|(i, agent)| {
                    let is_selected = state.selected() == Some(i);
                    let style = if is_selected {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    };

                    let name = agent.name.clone();
                    let description = if is_selected {
                        format!("  {}", agent.key)
                    } else {
                        String::new()
                    };

                    ListItem::new(format!("{}{}", name, description)).style(style)
                })
                .collect();

            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Select AI Agent (↑/↓ to navigate, Enter to select, Esc to cancel)"),
                )
                .highlight_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                );

            f.render_stateful_widget(list, size, &mut state);
        })?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Esc => {
                    break SelectionResult::Cancelled;
                }
                KeyCode::Enter => {
                    if let Some(selected) = state.selected() {
                        let agent_key = agents[selected].key.clone();
                        break SelectionResult::Selected(agent_key);
                    }
                }
                KeyCode::Up => {
                    if let Some(selected) = state.selected() {
                        let new_selected = if selected == 0 {
                            agents.len() - 1
                        } else {
                            selected - 1
                        };
                        state.select(Some(new_selected));
                    }
                }
                KeyCode::Down => {
                    if let Some(selected) = state.selected() {
                        let new_selected = (selected + 1) % agents.len();
                        state.select(Some(new_selected));
                    }
                }
                KeyCode::Char('q') => {
                    break SelectionResult::Cancelled;
                }
                _ => {}
            }
        }
    };

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(result)
}
