//! Terminal rendering helper for `aikit run --progress`.
//!
//! Supports two modes:
//! - **TTY mode**: redraws a viewport in-place using cursor manipulation.
//! - **Non-TTY mode**: appends new lines to stderr without cursor movement.

use aikit_sdk::RunProgress;
use crossterm::{
    cursor,
    terminal::{self, ClearType},
    ExecutableCommand,
};
use std::io::{self, Stderr, Write};

/// Renders [`RunProgress`] state to stderr.
pub struct ProgressRenderer {
    is_tty: bool,
    stderr: Stderr,
    /// Number of lines rendered in the last TTY redraw (used for cursor positioning).
    last_render_lines: usize,
}

impl ProgressRenderer {
    /// Create a new renderer. Detects TTY automatically.
    pub fn new() -> io::Result<Self> {
        let is_tty = atty::is(atty::Stream::Stderr);
        Ok(Self {
            is_tty,
            stderr: io::stderr(),
            last_render_lines: 0,
        })
    }

    /// Create a renderer that always uses non-TTY (append-only) mode.
    pub fn non_tty() -> Self {
        Self {
            is_tty: false,
            stderr: io::stderr(),
            last_render_lines: 0,
        }
    }

    /// Render the current progress state to stderr.
    pub fn render(&mut self, progress: &RunProgress) -> io::Result<()> {
        if self.is_tty {
            self.render_tty(progress)
        } else {
            self.render_append(progress)
        }
    }

    /// Final summary output after the agent exits.
    pub fn finalize(&mut self, exit_code: i32, final_tokens: Option<String>) -> io::Result<()> {
        if self.is_tty {
            // Clear the viewport before writing the final summary
            self.clear_viewport()?;
        }
        writeln!(self.stderr, "--- agent finished (exit={}) ---", exit_code)?;
        if let Some(tokens) = final_tokens {
            writeln!(self.stderr, "{}", tokens)?;
        }
        self.stderr.flush()
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    fn render_tty(&mut self, progress: &RunProgress) -> io::Result<()> {
        self.clear_viewport()?;

        let lines: Vec<&str> = progress.formatted_lines().collect();
        let (term_width, term_height) = terminal::size().unwrap_or((80, 24));

        // Reserve space for token footer if present
        let footer = progress.token_footer();
        let footer_lines = if footer.is_some() { 1 } else { 0 };
        let max_display = (term_height as usize)
            .saturating_sub(footer_lines + 1)
            .max(1);

        let display_start = lines.len().saturating_sub(max_display);
        let display = &lines[display_start..];

        let mut rendered = 0;
        for line in display {
            let truncated = if line.len() > term_width as usize {
                &line[..term_width as usize]
            } else {
                line
            };
            writeln!(self.stderr, "{}", truncated)?;
            rendered += 1;
        }

        if let Some(ref f) = footer {
            let truncated = if f.len() > term_width as usize {
                &f[..term_width as usize]
            } else {
                f.as_str()
            };
            writeln!(self.stderr, "{}", truncated)?;
            rendered += 1;
        }

        self.last_render_lines = rendered;
        self.stderr.flush()
    }

    fn render_append(&mut self, progress: &RunProgress) -> io::Result<()> {
        // In non-TTY mode emit only the newest line (last in the ring buffer)
        if let Some(last) = progress.formatted_lines().last() {
            writeln!(self.stderr, "{}", last)?;
        }
        self.stderr.flush()
    }

    fn clear_viewport(&mut self) -> io::Result<()> {
        if self.last_render_lines == 0 {
            return Ok(());
        }
        // Move cursor up by the number of lines we rendered, then clear downward
        self.stderr
            .execute(cursor::MoveUp(self.last_render_lines as u16))?;
        self.stderr
            .execute(terminal::Clear(ClearType::FromCursorDown))?;
        self.last_render_lines = 0;
        Ok(())
    }
}
