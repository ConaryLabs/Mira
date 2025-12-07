// src/testing/dashboard/ui.rs
// Main UI rendering

use ratatui::{
    layout::{Constraint, Direction, Layout},
    Frame,
};

use super::state::{DashboardState, View};
use super::views;

/// Render the full dashboard UI
pub fn render(f: &mut Frame, state: &DashboardState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),     // Main content
            Constraint::Length(1), // Status bar
        ])
        .split(f.area());

    // Render the active view
    match state.view {
        View::LiveStream => views::render_live_stream(f, chunks[0], state),
        View::Operations => views::render_operations(f, chunks[0], state),
        View::ToolInspector => views::render_tool_inspector(f, chunks[0], state),
        View::Replay => views::render_replay(f, chunks[0], state),
        View::Help => views::render_help(f, chunks[0], state),
    }

    // Render status bar
    views::render_status_bar(f, chunks[1], state);
}
