// src/testing/dashboard/views.rs
// View rendering for dashboard

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table, Wrap},
    Frame,
};

use super::state::{DashboardState, View};

/// Render the live event stream view
pub fn render_live_stream(f: &mut Frame, area: Rect, state: &DashboardState) {
    let events = state.filtered_events();
    let items: Vec<ListItem> = events
        .iter()
        .enumerate()
        .map(|(i, event)| {
            let style = if i == state.selected_index {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            let type_color = match event.event_type.as_str() {
                t if t.starts_with("error") => Color::Red,
                t if t.starts_with("operation.completed") => Color::Green,
                t if t.starts_with("operation.failed") => Color::Red,
                t if t.starts_with("operation.tool") => Color::Yellow,
                t if t.starts_with("operation") => Color::Cyan,
                t if t.starts_with("chat") => Color::Blue,
                _ => Color::Gray,
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("{:>5} ", event.sequence),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<25} ", event.event_type),
                    Style::default().fg(type_color),
                ),
                Span::raw(event.summary()),
            ]);

            ListItem::new(line).style(style)
        })
        .collect();

    let title = if state.event_filter.is_empty() {
        format!("Live Events ({} total)", state.events.len())
    } else {
        format!(
            "Live Events ({} filtered, {} total)",
            items.len(),
            state.events.len()
        )
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(list, area);
}

/// Render the operations list view
pub fn render_operations(f: &mut Frame, area: Rect, state: &DashboardState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Operations list
    let rows: Vec<Row> = state
        .operations
        .iter()
        .enumerate()
        .map(|(i, op)| {
            let style = if i == state.selected_index {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            let _status_color = match op.status.as_str() {
                "completed" => Color::Green,
                "failed" => Color::Red,
                "started" => Color::Yellow,
                _ => Color::Cyan,
            };

            let duration = op
                .duration
                .map(|d| format!("{:.2}s", d.as_secs_f64()))
                .unwrap_or_else(|| format!("{:.2}s", op.started_at.elapsed().as_secs_f64()));

            Row::new(vec![
                format!("{}...", &op.id[..8]),
                op.status.clone(),
                duration,
                op.tool_count.to_string(),
                op.error.as_deref().unwrap_or("-").to_string(),
            ])
            .style(style)
            .height(1)
        })
        .collect();

    let header = Row::new(vec!["ID", "Status", "Duration", "Tools", "Error"])
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Operations ({})", state.operations.len()))
            .border_style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(table, chunks[0]);

    // Operation details (events for selected operation)
    let detail_content = if let Some(op) = state.operations.get(state.selected_index) {
        let events: Vec<Line> = op
            .events
            .iter()
            .map(|e| {
                Line::from(vec![
                    Span::styled(
                        format!("{:<20} ", e.event_type),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw(e.summary()),
                ])
            })
            .collect();
        events
    } else {
        vec![Line::from("Select an operation to see details")]
    };

    let detail = Paragraph::new(detail_content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Operation Events")
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(detail, chunks[1]);
}

/// Render the tool inspector view
pub fn render_tool_inspector(f: &mut Frame, area: Rect, state: &DashboardState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Tool executions list
    let rows: Vec<Row> = state
        .tool_executions
        .iter()
        .enumerate()
        .map(|(i, exec)| {
            let style = if i == state.selected_index {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            let _status_style = if exec.success {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            };

            Row::new(vec![
                exec.tool_name.clone(),
                if exec.success { "OK" } else { "FAIL" }.to_string(),
                format!("{}ms", exec.duration_ms),
                exec.tool_type.clone(),
                format!("{}...", &exec.operation_id[..8.min(exec.operation_id.len())]),
            ])
            .style(style)
        })
        .collect();

    let header = Row::new(vec!["Tool", "Status", "Duration", "Type", "Operation"])
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

    let table = Table::new(
        rows,
        [
            Constraint::Length(25),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Min(12),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Tool Executions ({})", state.tool_executions.len()))
            .border_style(Style::default().fg(Color::Yellow)),
    );

    f.render_widget(table, chunks[0]);

    // Tool details - show summary
    let detail_content = if let Some(exec) = state.tool_executions.get(state.selected_index) {
        vec![
            Line::from(vec![
                Span::styled("Tool: ", Style::default().fg(Color::Yellow)),
                Span::raw(&exec.tool_name),
            ]),
            Line::from(vec![
                Span::styled("Type: ", Style::default().fg(Color::Yellow)),
                Span::raw(&exec.tool_type),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    if exec.success { "Success" } else { "Failed" },
                    if exec.success { Style::default().fg(Color::Green) } else { Style::default().fg(Color::Red) }
                ),
            ]),
            Line::from(vec![
                Span::styled("Duration: ", Style::default().fg(Color::Yellow)),
                Span::raw(format!("{}ms", exec.duration_ms)),
            ]),
            Line::from(""),
            Line::from(Span::styled("Summary:", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
            Line::from(exec.summary.clone()),
        ]
    } else {
        vec![Line::from("Select a tool execution to see details")]
    };

    let detail = Paragraph::new(detail_content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Tool Details")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(detail, chunks[1]);
}

/// Render the replay view (placeholder)
pub fn render_replay(f: &mut Frame, area: Rect, _state: &DashboardState) {
    let text = vec![
        Line::from("Replay Mode"),
        Line::from(""),
        Line::from("Load operations from the database to replay."),
        Line::from(""),
        Line::from("Commands:"),
        Line::from("  l - Load recent operations"),
        Line::from("  Enter - Select operation to replay"),
        Line::from("  Space - Play/Pause"),
        Line::from("  <- -> - Step through events"),
    ];

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Replay Mode (Coming Soon)")
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, area);
}

/// Render the help view
pub fn render_help(f: &mut Frame, area: Rect, _state: &DashboardState) {
    let text = vec![
        Line::from(Span::styled(
            "Mira Test Dashboard",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled("Navigation", Style::default().add_modifier(Modifier::BOLD))),
        Line::from("  Tab / Shift+Tab  - Switch views"),
        Line::from("  j / Down         - Move down"),
        Line::from("  k / Up           - Move up"),
        Line::from("  g                - Go to top"),
        Line::from("  G                - Go to bottom"),
        Line::from(""),
        Line::from(Span::styled("Views", Style::default().add_modifier(Modifier::BOLD))),
        Line::from("  l - Live Event Stream"),
        Line::from("  o - Operations List"),
        Line::from("  t - Tool Inspector"),
        Line::from("  r - Replay Mode"),
        Line::from("  ? - This help"),
        Line::from(""),
        Line::from(Span::styled("Actions", Style::default().add_modifier(Modifier::BOLD))),
        Line::from("  Space   - Pause/Resume live updates"),
        Line::from("  c       - Clear all events"),
        Line::from("  /       - Filter events (type to filter)"),
        Line::from("  Esc     - Clear filter"),
        Line::from("  q       - Quit"),
        Line::from(""),
        Line::from(Span::styled("Event Types", Style::default().add_modifier(Modifier::BOLD))),
        Line::from(vec![
            Span::styled("  operation.* ", Style::default().fg(Color::Cyan)),
            Span::raw("- Operation lifecycle events"),
        ]),
        Line::from(vec![
            Span::styled("  operation.tool_* ", Style::default().fg(Color::Yellow)),
            Span::raw("- Tool execution events"),
        ]),
        Line::from(vec![
            Span::styled("  chat.* ", Style::default().fg(Color::Blue)),
            Span::raw("- Chat/streaming events"),
        ]),
        Line::from(vec![
            Span::styled("  error ", Style::default().fg(Color::Red)),
            Span::raw("- Error events"),
        ]),
    ];

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Help")
                .border_style(Style::default().fg(Color::Green)),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, area);
}

/// Render the status bar
pub fn render_status_bar(f: &mut Frame, area: Rect, state: &DashboardState) {
    let connection_status = if state.connected {
        Span::styled(" CONNECTED ", Style::default().bg(Color::Green).fg(Color::Black))
    } else {
        Span::styled(" DISCONNECTED ", Style::default().bg(Color::Red).fg(Color::White))
    };

    let pause_status = if state.paused {
        Span::styled(" PAUSED ", Style::default().bg(Color::Yellow).fg(Color::Black))
    } else {
        Span::raw("")
    };

    let view_name = match state.view {
        View::LiveStream => "Live Stream",
        View::Operations => "Operations",
        View::ToolInspector => "Tool Inspector",
        View::Replay => "Replay",
        View::Help => "Help",
    };

    let uptime = state.uptime();
    let uptime_str = format!(
        "{:02}:{:02}:{:02}",
        uptime.as_secs() / 3600,
        (uptime.as_secs() % 3600) / 60,
        uptime.as_secs() % 60
    );

    let filter_info = if !state.event_filter.is_empty() {
        format!(" Filter: {} ", state.event_filter)
    } else {
        String::new()
    };

    let line = Line::from(vec![
        connection_status,
        pause_status,
        Span::raw(" "),
        Span::styled(view_name, Style::default().fg(Color::Cyan)),
        Span::raw(" | "),
        Span::raw(format!("Events: {} ", state.events.len())),
        Span::raw(format!("Ops: {} ", state.operations.len())),
        Span::raw(format!("Tools: {} ", state.tool_executions.len())),
        Span::styled(filter_info, Style::default().fg(Color::Yellow)),
        Span::raw(" | "),
        Span::raw(format!("Uptime: {} ", uptime_str)),
        Span::styled(
            " Tab:Views  ?:Help  q:Quit ",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(line);
    f.render_widget(paragraph, area);
}
