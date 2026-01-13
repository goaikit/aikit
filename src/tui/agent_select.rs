//! Interactive agent selection UI

use crate::core::agent::get_agent_configs;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::io;

/// Result of agent selection
pub enum SelectionResult {
    Selected(String),
    Cancelled,
}

/// Select agent interactively using TUI
pub fn select_agent_interactive() -> Result<SelectionResult, Box<dyn std::error::Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the selection app
    let result = run_selection_app(&mut terminal);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_selection_app<B: Backend>(
    terminal: &mut Terminal<B>,
) -> Result<SelectionResult, Box<dyn std::error::Error>>
where
    <B as Backend>::Error: 'static,
{
    let mut app = SelectionApp::new();

    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        return Ok(SelectionResult::Cancelled);
                    }
                    KeyCode::Up => {
                        app.previous();
                    }
                    KeyCode::Down => {
                        app.next();
                    }
                    KeyCode::Enter => {
                        if let Some(selected) = app.selected_agent() {
                            return Ok(SelectionResult::Selected(selected));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

struct SelectionApp {
    state: ListState,
    agents: Vec<(String, String)>, // (key, name)
}

impl SelectionApp {
    fn new() -> Self {
        let agents: Vec<(String, String)> = get_agent_configs()
            .into_iter()
            .map(|config| (config.key, config.name))
            .collect();

        let mut state = ListState::default();
        state.select(Some(0)); // Default to first item

        Self { state, agents }
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.agents.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.agents.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn selected_agent(&self) -> Option<String> {
        self.state.selected().map(|i| self.agents[i].0.clone())
    }
}

fn ui(f: &mut Frame, app: &mut SelectionApp) {
    // Create the main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Min(10),   // List
            Constraint::Length(4), // Instructions
        ])
        .split(f.area());

    // Title
    let title = Paragraph::new("Select AI Agent")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // Create list items
    let items: Vec<ListItem> = app
        .agents
        .iter()
        .enumerate()
        .map(|(i, (key, name))| {
            let style = if Some(i) == app.state.selected() {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(format!("{} ({})", key, name)).style(style)
        })
        .collect();

    // Create the list widget
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Available Agents"),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, chunks[1], &mut app.state);

    // Instructions
    let instructions = vec![Line::from(vec![
        Span::styled("↑/↓", Style::default().fg(Color::Cyan)),
        Span::raw(" Navigate • "),
        Span::styled("Enter", Style::default().fg(Color::Green)),
        Span::raw(" Select • "),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::raw(" Cancel"),
    ])];

    let instructions_widget = Paragraph::new(instructions)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(instructions_widget, chunks[2]);
}
