// src/testing/dashboard/state.rs
// Dashboard state management

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::cli::ws_client::{BackendEvent, OperationEvent};

/// Maximum number of events to keep in history
const MAX_EVENTS: usize = 1000;

/// Current view in the dashboard
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    LiveStream,
    Operations,
    ToolInspector,
    Replay,
    Help,
}

/// An event with metadata for display
#[derive(Debug, Clone)]
pub struct DisplayEvent {
    pub event: BackendEvent,
    pub timestamp: Instant,
    pub sequence: usize,
    pub event_type: String,
}

impl DisplayEvent {
    pub fn new(event: BackendEvent, sequence: usize) -> Self {
        let event_type = Self::get_event_type(&event);
        Self {
            event,
            timestamp: Instant::now(),
            sequence,
            event_type,
        }
    }

    fn get_event_type(event: &BackendEvent) -> String {
        match event {
            BackendEvent::Connected => "connected".to_string(),
            BackendEvent::Disconnected => "disconnected".to_string(),
            BackendEvent::StreamToken(_) => "stream.token".to_string(),
            BackendEvent::ChatComplete { .. } => "chat.complete".to_string(),
            BackendEvent::Status { .. } => "status".to_string(),
            BackendEvent::Error { .. } => "error".to_string(),
            BackendEvent::SessionData { .. } => "session.data".to_string(),
            BackendEvent::OperationEvent(op) => match op {
                OperationEvent::Started { .. } => "operation.started".to_string(),
                OperationEvent::Streaming { .. } => "operation.streaming".to_string(),
                OperationEvent::PlanGenerated { .. } => "operation.plan".to_string(),
                OperationEvent::ToolExecuted { .. } => "operation.tool_executed".to_string(),
                OperationEvent::ArtifactPreview { .. } => "operation.artifact_preview".to_string(),
                OperationEvent::ArtifactCompleted { .. } => "operation.artifact".to_string(),
                OperationEvent::TaskCreated { .. } => "operation.task_created".to_string(),
                OperationEvent::TaskStarted { .. } => "operation.task_started".to_string(),
                OperationEvent::TaskCompleted { .. } => "operation.task_completed".to_string(),
                OperationEvent::AgentSpawned { .. } => "operation.agent_spawned".to_string(),
                OperationEvent::AgentProgress { .. } => "operation.agent_progress".to_string(),
                OperationEvent::AgentStreaming { .. } => "operation.agent_streaming".to_string(),
                OperationEvent::AgentCompleted { .. } => "operation.agent_completed".to_string(),
                OperationEvent::Completed { .. } => "operation.completed".to_string(),
                OperationEvent::Failed { .. } => "operation.failed".to_string(),
                OperationEvent::SudoApprovalRequired { .. } => "operation.sudo_request".to_string(),
                OperationEvent::Thinking { .. } => "operation.thinking".to_string(),
            },
        }
    }

    /// Get a summary string for the event
    pub fn summary(&self) -> String {
        match &self.event {
            BackendEvent::Connected => "Connected to backend".to_string(),
            BackendEvent::Disconnected => "Disconnected from backend".to_string(),
            BackendEvent::StreamToken(token) => {
                let preview: String = token.chars().take(30).collect();
                format!("Token: {}", preview.replace('\n', "\\n"))
            }
            BackendEvent::ChatComplete { content, .. } => {
                let preview: String = content.chars().take(50).collect();
                format!("Complete: {}...", preview.replace('\n', " "))
            }
            BackendEvent::Status { message, .. } => format!("Status: {}", message),
            BackendEvent::Error { message, .. } => format!("Error: {}", message),
            BackendEvent::SessionData { response_type, .. } => format!("Session: {}", response_type),
            BackendEvent::OperationEvent(op) => Self::operation_summary(op),
        }
    }

    fn operation_summary(op: &OperationEvent) -> String {
        match op {
            OperationEvent::Started { operation_id } => {
                format!("Started: {}", &operation_id[..8.min(operation_id.len())])
            }
            OperationEvent::Streaming { content, .. } => {
                let preview: String = content.chars().take(30).collect();
                format!("Stream: {}", preview.replace('\n', "\\n"))
            }
            OperationEvent::PlanGenerated { plan_text, .. } => {
                let preview: String = plan_text.chars().take(40).collect();
                format!("Plan: {}", preview.replace('\n', " "))
            }
            OperationEvent::ToolExecuted { tool_name, success, duration_ms, .. } => {
                let status = if *success { "OK" } else { "FAIL" };
                format!("{} {} ({}ms)", tool_name, status, duration_ms)
            }
            OperationEvent::ArtifactPreview { path, preview, .. } => {
                let path_str = path.as_deref().unwrap_or("inline");
                let preview_short: String = preview.chars().take(30).collect();
                format!("Preview: {} - {}", path_str, preview_short)
            }
            OperationEvent::ArtifactCompleted { artifact, .. } => {
                let path = artifact
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("inline");
                format!("Artifact completed: {}", path)
            }
            OperationEvent::TaskCreated { title, .. } => {
                format!("Task: {}", title)
            }
            OperationEvent::TaskStarted { task_id, .. } => {
                format!("Task started: {}", &task_id[..8.min(task_id.len())])
            }
            OperationEvent::TaskCompleted { task_id, .. } => {
                format!("Task done: {}", &task_id[..8.min(task_id.len())])
            }
            OperationEvent::AgentSpawned { agent_name, task, .. } => {
                let task_preview: String = task.chars().take(30).collect();
                format!("Agent {}: {}", agent_name, task_preview)
            }
            OperationEvent::AgentProgress { agent_name, iteration, max_iterations, current_activity, .. } => {
                format!("{} [{}/{}]: {}", agent_name, iteration, max_iterations, current_activity)
            }
            OperationEvent::AgentStreaming { content, .. } => {
                let preview: String = content.chars().take(30).collect();
                format!("Agent: {}", preview.replace('\n', "\\n"))
            }
            OperationEvent::AgentCompleted { agent_name, result, .. } => {
                let preview: String = result.chars().take(30).collect();
                format!("Agent {} done: {}", agent_name, preview)
            }
            OperationEvent::Completed { result, .. } => {
                let preview: String = result.as_deref().unwrap_or("done").chars().take(30).collect();
                format!("Completed: {}", preview)
            }
            OperationEvent::Failed { error, .. } => {
                format!("Failed: {}", error)
            }
            OperationEvent::SudoApprovalRequired { command, .. } => {
                format!("Sudo: {}", command)
            }
            OperationEvent::Thinking { status, message, .. } => {
                format!("Thinking [{}]: {}", status, message)
            }
        }
    }
}

/// Operation summary for the operations list view
#[derive(Debug, Clone)]
pub struct OperationSummary {
    pub id: String,
    pub status: String,
    pub started_at: Instant,
    pub duration: Option<Duration>,
    pub tool_count: usize,
    pub error: Option<String>,
    pub events: Vec<DisplayEvent>,
}

impl OperationSummary {
    pub fn new(id: String) -> Self {
        Self {
            id,
            status: "started".to_string(),
            started_at: Instant::now(),
            duration: None,
            tool_count: 0,
            error: None,
            events: Vec::new(),
        }
    }
}

/// Tool execution details for the tool inspector
#[derive(Debug, Clone)]
pub struct ToolExecution {
    pub tool_name: String,
    pub operation_id: String,
    pub summary: String,
    pub tool_type: String,
    pub success: bool,
    pub duration_ms: u64,
    pub timestamp: Instant,
}

/// Dashboard state
#[derive(Debug)]
pub struct DashboardState {
    /// Current view
    pub view: View,
    /// All events received
    pub events: VecDeque<DisplayEvent>,
    /// Event sequence counter
    pub event_counter: usize,
    /// Operations indexed by ID
    pub operations: Vec<OperationSummary>,
    /// Tool executions for inspector
    pub tool_executions: Vec<ToolExecution>,
    /// Selected index in list views
    pub selected_index: usize,
    /// Scroll offset for event stream
    pub scroll_offset: usize,
    /// Filter for event types (empty = show all)
    pub event_filter: String,
    /// Whether the dashboard is paused (not receiving new events)
    pub paused: bool,
    /// Connection status
    pub connected: bool,
    /// Start time for uptime display
    pub start_time: Instant,
}

impl Default for DashboardState {
    fn default() -> Self {
        Self::new()
    }
}

impl DashboardState {
    pub fn new() -> Self {
        Self {
            view: View::LiveStream,
            events: VecDeque::with_capacity(MAX_EVENTS),
            event_counter: 0,
            operations: Vec::new(),
            tool_executions: Vec::new(),
            selected_index: 0,
            scroll_offset: 0,
            event_filter: String::new(),
            paused: false,
            connected: false,
            start_time: Instant::now(),
        }
    }

    /// Add a new event
    pub fn add_event(&mut self, event: BackendEvent) {
        if self.paused {
            return;
        }

        let display_event = DisplayEvent::new(event.clone(), self.event_counter);
        self.event_counter += 1;

        // Update connection status
        match &event {
            BackendEvent::Connected => self.connected = true,
            BackendEvent::Disconnected => self.connected = false,
            _ => {}
        }

        // Update operations
        if let BackendEvent::OperationEvent(op) = &event {
            self.handle_operation_event(op, &display_event);
        }

        // Add to events queue
        self.events.push_back(display_event);
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
    }

    fn handle_operation_event(&mut self, op: &OperationEvent, display_event: &DisplayEvent) {
        match op {
            OperationEvent::Started { operation_id } => {
                let mut summary = OperationSummary::new(operation_id.clone());
                summary.events.push(display_event.clone());
                self.operations.push(summary);
            }
            OperationEvent::ToolExecuted { operation_id, tool_name, tool_type, summary, success, duration_ms } => {
                if let Some(op_summary) = self.operations.iter_mut().find(|s| &s.id == operation_id) {
                    op_summary.tool_count += 1;
                    op_summary.events.push(display_event.clone());
                }
                // Add tool execution
                self.tool_executions.push(ToolExecution {
                    tool_name: tool_name.clone(),
                    operation_id: operation_id.clone(),
                    summary: summary.clone(),
                    tool_type: tool_type.clone(),
                    success: *success,
                    duration_ms: *duration_ms,
                    timestamp: Instant::now(),
                });
            }
            OperationEvent::Completed { operation_id, .. } => {
                if let Some(summary) = self.operations.iter_mut().find(|s| &s.id == operation_id) {
                    summary.status = "completed".to_string();
                    summary.duration = Some(summary.started_at.elapsed());
                    summary.events.push(display_event.clone());
                }
            }
            OperationEvent::Failed { operation_id, error } => {
                if let Some(summary) = self.operations.iter_mut().find(|s| &s.id == operation_id) {
                    summary.status = "failed".to_string();
                    summary.error = Some(error.clone());
                    summary.duration = Some(summary.started_at.elapsed());
                    summary.events.push(display_event.clone());
                }
            }
            _ => {
                // Add to current operation if there is one
                if let Some(summary) = self.operations.last_mut() {
                    summary.events.push(display_event.clone());
                }
            }
        }
    }

    /// Get filtered events for display
    pub fn filtered_events(&self) -> Vec<&DisplayEvent> {
        if self.event_filter.is_empty() {
            self.events.iter().collect()
        } else {
            self.events.iter()
                .filter(|e| e.event_type.contains(&self.event_filter))
                .collect()
        }
    }

    /// Navigate to next view
    pub fn next_view(&mut self) {
        self.view = match self.view {
            View::LiveStream => View::Operations,
            View::Operations => View::ToolInspector,
            View::ToolInspector => View::Replay,
            View::Replay => View::Help,
            View::Help => View::LiveStream,
        };
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Navigate to previous view
    pub fn prev_view(&mut self) {
        self.view = match self.view {
            View::LiveStream => View::Help,
            View::Operations => View::LiveStream,
            View::ToolInspector => View::Operations,
            View::Replay => View::ToolInspector,
            View::Help => View::Replay,
        };
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Select next item in list
    pub fn select_next(&mut self) {
        let max = match self.view {
            View::LiveStream => self.filtered_events().len().saturating_sub(1),
            View::Operations => self.operations.len().saturating_sub(1),
            View::ToolInspector => self.tool_executions.len().saturating_sub(1),
            _ => 0,
        };
        if self.selected_index < max {
            self.selected_index += 1;
        }
    }

    /// Select previous item in list
    pub fn select_prev(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Toggle pause
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    /// Clear all events
    pub fn clear(&mut self) {
        self.events.clear();
        self.operations.clear();
        self.tool_executions.clear();
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Get uptime
    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }
}
