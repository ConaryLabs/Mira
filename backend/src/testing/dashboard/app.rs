// src/testing/dashboard/app.rs
// Main dashboard application

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::cli::ws_client::BackendEvent;
use super::state::{DashboardState, View};
use super::ui;

/// Dashboard application
pub struct DashboardApp {
    state: DashboardState,
    event_rx: mpsc::UnboundedReceiver<BackendEvent>,
}

impl DashboardApp {
    /// Create a new dashboard app with an event receiver
    pub fn new(event_rx: mpsc::UnboundedReceiver<BackendEvent>) -> Self {
        Self {
            state: DashboardState::new(),
            event_rx,
        }
    }

    /// Create a dashboard with no event source (for testing)
    pub fn standalone() -> (Self, mpsc::UnboundedSender<BackendEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self::new(rx), tx)
    }

    /// Run the dashboard
    pub async fn run(mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.event_loop(&mut terminal).await;

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

    async fn event_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        loop {
            // Draw UI
            terminal.draw(|f| ui::render(f, &self.state))?;

            // Poll for events with timeout
            let timeout = Duration::from_millis(100);

            // Check for keyboard input
            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    // Handle quit
                    if key.code == KeyCode::Char('q')
                        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        return Ok(());
                    }

                    // Handle input
                    self.handle_key(key.code, key.modifiers);
                }
            }

            // Check for backend events
            while let Ok(event) = self.event_rx.try_recv() {
                self.state.add_event(event);
            }
        }
    }

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match code {
            // Navigation between views
            KeyCode::Tab => {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    self.state.prev_view();
                } else {
                    self.state.next_view();
                }
            }

            // Direct view access
            KeyCode::Char('l') => self.state.view = View::LiveStream,
            KeyCode::Char('o') => self.state.view = View::Operations,
            KeyCode::Char('t') => self.state.view = View::ToolInspector,
            KeyCode::Char('r') => self.state.view = View::Replay,
            KeyCode::Char('?') => self.state.view = View::Help,

            // List navigation
            KeyCode::Down | KeyCode::Char('j') => self.state.select_next(),
            KeyCode::Up | KeyCode::Char('k') => self.state.select_prev(),
            KeyCode::Char('g') => self.state.selected_index = 0,
            KeyCode::Char('G') => {
                let max = match self.state.view {
                    View::LiveStream => self.state.filtered_events().len(),
                    View::Operations => self.state.operations.len(),
                    View::ToolInspector => self.state.tool_executions.len(),
                    _ => 0,
                };
                self.state.selected_index = max.saturating_sub(1);
            }

            // Actions
            KeyCode::Char(' ') => self.state.toggle_pause(),
            KeyCode::Char('c') if !modifiers.contains(KeyModifiers::CONTROL) => self.state.clear(),
            KeyCode::Esc => self.state.event_filter.clear(),

            // Filter input (simplified - just toggle between some common filters)
            KeyCode::Char('/') => {
                // Cycle through common filters
                self.state.event_filter = match self.state.event_filter.as_str() {
                    "" => "operation".to_string(),
                    "operation" => "tool".to_string(),
                    "tool" => "error".to_string(),
                    "error" => "chat".to_string(),
                    _ => String::new(),
                };
            }

            _ => {}
        }
    }
}

/// Run the dashboard connected to a WebSocket
pub async fn run_dashboard(ws_url: &str) -> Result<()> {
    use crate::cli::ws_client::MiraClient;

    // Create event channel
    let (tx, rx) = mpsc::unbounded_channel();

    // Create dashboard
    let app = DashboardApp::new(rx);

    // Connect to WebSocket in background
    let url = ws_url.to_string();
    let event_tx = tx.clone();
    tokio::spawn(async move {
        loop {
            match MiraClient::connect(&url).await {
                Ok(mut client) => {
                    let _ = event_tx.send(BackendEvent::Connected);

                    // Forward events to dashboard using recv()
                    while let Some(event) = client.recv().await {
                        if event_tx.send(event).is_err() {
                            break;
                        }
                    }

                    let _ = event_tx.send(BackendEvent::Disconnected);
                }
                Err(e) => {
                    tracing::error!("Failed to connect to WebSocket: {}", e);
                    let _ = event_tx.send(BackendEvent::Error {
                        message: format!("Connection failed: {}", e),
                        code: "connection_error".to_string(),
                    });
                }
            }

            // Wait before reconnecting
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });

    // Run the dashboard
    app.run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dashboard_state() {
        let (_tx, rx) = mpsc::unbounded_channel::<BackendEvent>();
        let app = DashboardApp::new(rx);

        assert_eq!(app.state.view, View::LiveStream);
        assert!(app.state.events.is_empty());
        assert!(!app.state.connected);
    }

    #[test]
    fn test_event_processing() {
        let mut state = DashboardState::new();

        state.add_event(BackendEvent::Connected);
        assert!(state.connected);
        assert_eq!(state.events.len(), 1);

        state.add_event(BackendEvent::Disconnected);
        assert!(!state.connected);
        assert_eq!(state.events.len(), 2);
    }

    #[test]
    fn test_view_navigation() {
        let mut state = DashboardState::new();

        assert_eq!(state.view, View::LiveStream);
        state.next_view();
        assert_eq!(state.view, View::Operations);
        state.next_view();
        assert_eq!(state.view, View::ToolInspector);
        state.prev_view();
        assert_eq!(state.view, View::Operations);
    }

    #[test]
    fn test_pause() {
        let mut state = DashboardState::new();

        assert!(!state.paused);
        state.add_event(BackendEvent::Connected);
        assert_eq!(state.events.len(), 1);

        state.toggle_pause();
        assert!(state.paused);
        state.add_event(BackendEvent::Disconnected);
        assert_eq!(state.events.len(), 1); // Should not add when paused

        state.toggle_pause();
        state.add_event(BackendEvent::Disconnected);
        assert_eq!(state.events.len(), 2);
    }
}
