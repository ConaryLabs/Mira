// backend/src/cli/session/picker.rs
// Interactive session picker UI

use anyhow::Result;
use console::{style, Term};
use std::io::{self, Write};

use super::types::CliSession;

/// Interactive session picker
pub struct SessionPicker {
    sessions: Vec<CliSession>,
    selected: usize,
    term: Term,
}

impl SessionPicker {
    /// Create a new session picker
    pub fn new(sessions: Vec<CliSession>) -> Self {
        Self {
            sessions,
            selected: 0,
            term: Term::stdout(),
        }
    }

    /// Show the picker and return the selected session
    pub fn show(&mut self) -> Result<Option<CliSession>> {
        if self.sessions.is_empty() {
            println!("{}", style("No sessions found.").dim());
            return Ok(None);
        }

        // Enable raw mode for keyboard input
        crossterm::terminal::enable_raw_mode()?;

        let result = self.run_picker();

        // Disable raw mode
        crossterm::terminal::disable_raw_mode()?;

        // Clear the picker display
        self.clear_display()?;

        result
    }

    /// Run the interactive picker loop
    fn run_picker(&mut self) -> Result<Option<CliSession>> {
        use crossterm::event::{self, Event, KeyCode, KeyModifiers};

        loop {
            self.render()?;

            // Wait for keyboard input
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.selected > 0 {
                            self.selected -= 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if self.selected < self.sessions.len() - 1 {
                            self.selected += 1;
                        }
                    }
                    KeyCode::Enter => {
                        return Ok(Some(self.sessions[self.selected].clone()));
                    }
                    KeyCode::Esc | KeyCode::Char('q') => {
                        return Ok(None);
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(None);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Render the picker UI
    fn render(&self) -> Result<()> {
        // Move to start and clear
        self.term.move_cursor_up(self.sessions.len().min(10) + 3)?;
        self.term.clear_to_end_of_screen()?;

        // Header
        println!(
            "{}\n",
            style("Select a session (↑/↓ to navigate, Enter to select, Esc to cancel)").cyan()
        );

        // Sessions list (show max 10)
        let start = if self.selected >= 10 {
            self.selected - 9
        } else {
            0
        };
        let end = (start + 10).min(self.sessions.len());

        for (i, session) in self.sessions[start..end].iter().enumerate() {
            let idx = start + i;
            let is_selected = idx == self.selected;

            let prefix = if is_selected {
                style("> ").cyan().bold()
            } else {
                style("  ").dim()
            };

            let name = if is_selected {
                style(session.display_name()).bold()
            } else {
                style(session.display_name())
            };

            let time = style(session.last_active_display()).dim();
            let messages = style(format!("{} msgs", session.message_count)).dim();
            let preview = style(truncate(&session.preview(), 40)).dim();

            println!("{}{} {} | {} | {}", prefix, name, time, messages, preview);
        }

        // Footer
        if self.sessions.len() > 10 {
            println!(
                "\n{}",
                style(format!(
                    "Showing {}-{} of {} sessions",
                    start + 1,
                    end,
                    self.sessions.len()
                ))
                .dim()
            );
        }

        io::stdout().flush()?;
        Ok(())
    }

    /// Clear the picker display
    fn clear_display(&self) -> Result<()> {
        let lines = self.sessions.len().min(10) + 4;
        for _ in 0..lines {
            self.term.clear_line()?;
            self.term.move_cursor_up(1)?;
        }
        self.term.clear_line()?;
        Ok(())
    }
}

/// Simple session picker without interactivity (for non-TTY environments)
pub fn simple_session_list(sessions: &[CliSession]) {
    if sessions.is_empty() {
        println!("{}", style("No sessions found.").dim());
        return;
    }

    println!("{}\n", style("Recent sessions:").cyan().bold());

    for (i, session) in sessions.iter().enumerate().take(10) {
        let idx = style(format!("{:2}.", i + 1)).dim();
        let name = style(session.display_name()).bold();
        let time = style(session.last_active_display()).dim();
        let id = style(format!("[{}]", &session.id[..8])).dim();

        println!("{} {} {} {}", idx, name, time, id);

        if let Some(ref preview) = session.last_message {
            println!("    {}", style(truncate(preview, 60)).dim());
        }
    }

    println!(
        "\n{}",
        style("Use -r <session-id> to resume a specific session").dim()
    );
}

/// Truncate a string to a maximum length
fn truncate(s: &str, max_len: usize) -> String {
    // Take first line only
    let s = s.lines().next().unwrap_or(s);
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("this is a longer string", 10), "this is...");
        assert_eq!(truncate("line1\nline2", 20), "line1");
    }

    #[test]
    fn test_empty_picker() {
        let picker = SessionPicker::new(vec![]);
        assert!(picker.sessions.is_empty());
    }
}
