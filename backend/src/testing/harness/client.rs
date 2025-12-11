// src/testing/harness/client.rs
// Extended WebSocket client for testing with event capture

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, info, warn};

use crate::api::ws::message::{SystemAccessMode, WsClientMessage};
use crate::cli::ws_client::{BackendEvent, OperationEvent};
use serde_json::json;

/// A captured event with metadata
#[derive(Debug, Clone)]
pub struct CapturedEvent {
    pub event: BackendEvent,
    pub timestamp: Instant,
    pub sequence: usize,
}

/// Collection of captured events with query methods
#[derive(Debug, Clone)]
pub struct CapturedEvents {
    events: Vec<CapturedEvent>,
    start_time: Instant,
}

impl CapturedEvents {
    pub fn new(events: Vec<CapturedEvent>) -> Self {
        let start_time = events.first()
            .map(|e| e.timestamp)
            .unwrap_or_else(Instant::now);
        Self { events, start_time }
    }

    /// Get all captured events
    pub fn all(&self) -> &[CapturedEvent] {
        &self.events
    }

    /// Get total count of events
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Get total duration from first to last event
    pub fn duration(&self) -> Duration {
        if let (Some(first), Some(last)) = (self.events.first(), self.events.last()) {
            last.timestamp.duration_since(first.timestamp)
        } else {
            Duration::ZERO
        }
    }

    /// Get all events of a specific type
    pub fn of_type(&self, event_type: &str) -> Vec<&CapturedEvent> {
        self.events.iter()
            .filter(|e| self.event_type_matches(&e.event, event_type))
            .collect()
    }

    /// Get all tool execution events
    pub fn tool_executions(&self) -> Vec<&OperationEvent> {
        self.events.iter()
            .filter_map(|e| {
                if let BackendEvent::OperationEvent(op) = &e.event {
                    if matches!(op, OperationEvent::ToolExecuted { .. }) {
                        return Some(op);
                    }
                }
                None
            })
            .collect()
    }

    /// Get tool executions by name
    pub fn tool_executions_by_name(&self, name: &str) -> Vec<&OperationEvent> {
        self.tool_executions()
            .into_iter()
            .filter(|op| {
                if let OperationEvent::ToolExecuted { tool_name, .. } = op {
                    tool_name == name
                } else {
                    false
                }
            })
            .collect()
    }

    /// Check if any event matches predicate
    pub fn any<F>(&self, predicate: F) -> bool
    where
        F: Fn(&BackendEvent) -> bool,
    {
        self.events.iter().any(|e| predicate(&e.event))
    }

    /// Get the final response content (from ChatComplete or Completed)
    pub fn final_response(&self) -> Option<String> {
        for event in self.events.iter().rev() {
            match &event.event {
                BackendEvent::ChatComplete { content, .. } => {
                    return Some(content.clone());
                }
                BackendEvent::OperationEvent(OperationEvent::Completed { result, .. }) => {
                    return result.clone();
                }
                _ => {}
            }
        }
        None
    }

    /// Check if operation completed successfully
    pub fn completed_successfully(&self) -> bool {
        self.events.iter().any(|e| {
            matches!(
                &e.event,
                BackendEvent::ChatComplete { .. }
                    | BackendEvent::OperationEvent(OperationEvent::Completed { .. })
            )
        }) && !self.events.iter().any(|e| {
            matches!(
                &e.event,
                BackendEvent::Error { .. }
                    | BackendEvent::OperationEvent(OperationEvent::Failed { .. })
            )
        })
    }

    /// Get error message if operation failed
    pub fn error_message(&self) -> Option<String> {
        for event in self.events.iter().rev() {
            match &event.event {
                BackendEvent::Error { message, .. } => {
                    return Some(message.clone());
                }
                BackendEvent::OperationEvent(OperationEvent::Failed { error, .. }) => {
                    return Some(error.clone());
                }
                _ => {}
            }
        }
        None
    }

    /// Get accumulated streaming content
    pub fn streaming_content(&self) -> String {
        let mut content = String::new();
        for event in &self.events {
            match &event.event {
                BackendEvent::StreamToken(token) => content.push_str(token),
                BackendEvent::OperationEvent(OperationEvent::Streaming { content: c, .. }) => {
                    content.push_str(c);
                }
                _ => {}
            }
        }
        content
    }

    fn event_type_matches(&self, event: &BackendEvent, type_name: &str) -> bool {
        match (event, type_name) {
            (BackendEvent::Connected, "connected") => true,
            (BackendEvent::StreamToken(_), "stream_token") => true,
            (BackendEvent::ChatComplete { .. }, "chat_complete") => true,
            (BackendEvent::Status { .. }, "status") => true,
            (BackendEvent::Error { .. }, "error") => true,
            (BackendEvent::Disconnected, "disconnected") => true,
            (BackendEvent::OperationEvent(op), type_name) => {
                self.operation_event_type_matches(op, type_name)
            }
            _ => false,
        }
    }

    fn operation_event_type_matches(&self, event: &OperationEvent, type_name: &str) -> bool {
        match (event, type_name) {
            (OperationEvent::Started { .. }, "operation.started") => true,
            (OperationEvent::Streaming { .. }, "operation.streaming") => true,
            (OperationEvent::ToolExecuted { .. }, "operation.tool_executed") => true,
            (OperationEvent::Completed { .. }, "operation.completed") => true,
            (OperationEvent::Failed { .. }, "operation.failed") => true,
            (OperationEvent::PlanGenerated { .. }, "operation.plan_generated") => true,
            (OperationEvent::AgentSpawned { .. }, "operation.agent_spawned") => true,
            (OperationEvent::AgentProgress { .. }, "operation.agent_progress") => true,
            (OperationEvent::AgentCompleted { .. }, "operation.agent_completed") => true,
            (OperationEvent::TaskCreated { .. }, "operation.task_created") => true,
            (OperationEvent::TaskStarted { .. }, "operation.task_started") => true,
            (OperationEvent::TaskCompleted { .. }, "operation.task_completed") => true,
            (OperationEvent::SudoApprovalRequired { .. }, "operation.sudo_approval_required") => true,
            (OperationEvent::Thinking { .. }, "operation.thinking") => true,
            _ => false,
        }
    }
}

/// Extended WebSocket client for testing
pub struct TestClient {
    /// Sender for outgoing messages
    sender: Arc<Mutex<futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>
        >,
        Message
    >>>,
    /// Channel for receiving events
    event_rx: mpsc::Receiver<BackendEvent>,
    /// Current project ID
    project_id: Option<String>,
    /// Current session ID (for isolated testing)
    session_id: Option<String>,
    /// Connection status
    connected: bool,
    /// Default timeout for operations
    timeout: Duration,
    /// Event log for the current operation
    event_log: Vec<CapturedEvent>,
    /// System access mode for file operations
    access_mode: SystemAccessMode,
}

impl TestClient {
    /// Connect to the Mira backend with default settings
    pub async fn connect(url: &str) -> Result<Self> {
        Self::connect_with_timeout(url, Duration::from_secs(60)).await
    }

    /// Connect with custom timeout
    pub async fn connect_with_timeout(url: &str, timeout: Duration) -> Result<Self> {
        info!("[TestClient] Connecting to {}", url);

        let (ws_stream, _) = connect_async(url)
            .await
            .with_context(|| format!("Failed to connect to backend at {}", url))?;

        let (sender, mut receiver) = ws_stream.split();
        let sender = Arc::new(Mutex::new(sender));

        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(1000);

        // Spawn receiver task
        let event_tx_clone = event_tx.clone();
        tokio::spawn(async move {
            while let Some(msg_result) = receiver.next().await {
                match msg_result {
                    Ok(Message::Text(text)) => {
                        if let Some(event) = Self::parse_message(&text) {
                            if event_tx_clone.send(event).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(Message::Close(_)) => {
                        let _ = event_tx_clone.send(BackendEvent::Disconnected).await;
                        break;
                    }
                    Err(e) => {
                        let _ = event_tx_clone.send(BackendEvent::Error {
                            message: e.to_string(),
                            code: "websocket_error".to_string(),
                        }).await;
                        break;
                    }
                    _ => {}
                }
            }
        });

        // Send connected event
        let _ = event_tx.send(BackendEvent::Connected).await;

        info!("[TestClient] Connected successfully");

        Ok(Self {
            sender,
            event_rx,
            project_id: None,
            session_id: None,
            connected: true,
            timeout,
            event_log: Vec::new(),
            access_mode: SystemAccessMode::Project, // Default to Project mode
        })
    }

    /// Set the project ID for subsequent operations
    pub fn set_project(&mut self, project_id: Option<String>) {
        self.project_id = project_id.clone();
        info!("[TestClient] Set project_id: {:?}", project_id);
    }

    /// Set the timeout for operations
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// Create a new session with project path for isolated testing
    /// Returns the session ID on success
    pub async fn create_session(&mut self, name: Option<&str>, project_path: Option<&str>) -> Result<String> {
        info!("[TestClient] Creating session with project_path: {:?}", project_path);

        // Send session.create command
        let msg = WsClientMessage::SessionCommand {
            method: "session.create".to_string(),
            params: json!({
                "name": name,
                "project_path": project_path,
            }),
        };

        let json_str = serde_json::to_string(&msg)?;
        {
            let mut sender = self.sender.lock().await;
            sender.send(Message::Text(json_str.into())).await?;
        }

        // Wait for session_created response
        let start = Instant::now();
        let timeout = Duration::from_secs(10);

        loop {
            tokio::select! {
                event = self.event_rx.recv() => {
                    match event {
                        Some(BackendEvent::SessionData { response_type, data }) => {
                            if response_type == "session_created" {
                                // Extract session ID from response
                                if let Some(session) = data.get("session") {
                                    if let Some(id) = session.get("id").and_then(|v| v.as_str()) {
                                        info!("[TestClient] Session created: {}", id);
                                        self.session_id = Some(id.to_string());
                                        return Ok(id.to_string());
                                    }
                                }
                                return Err(anyhow::anyhow!("Session created but ID not found in response"));
                            }
                            // Not the response we're waiting for, continue
                            continue;
                        }
                        Some(BackendEvent::Error { message, .. }) => {
                            return Err(anyhow::anyhow!("Session creation failed: {}", message));
                        }
                        Some(_) => {
                            // Continue waiting for session response
                            continue;
                        }
                        None => {
                            return Err(anyhow::anyhow!("Event channel closed while creating session"));
                        }
                    }
                }
                _ = tokio::time::sleep(timeout.saturating_sub(start.elapsed())) => {
                    return Err(anyhow::anyhow!("Timeout creating session"));
                }
            }
        }
    }

    /// Set the session ID for subsequent operations
    pub fn set_session(&mut self, session_id: String) {
        info!("[TestClient] Using session: {}", session_id);
        self.session_id = Some(session_id);
    }

    /// Set the system access mode for file operations
    pub fn set_access_mode(&mut self, mode: SystemAccessMode) {
        info!("[TestClient] Setting access mode: {:?}", mode);
        self.access_mode = mode;
    }

    /// Register a directory as a project and get its ID
    /// This allows tools to access files within that directory
    pub async fn register_project(&mut self, path: &str, _name: Option<&str>) -> Result<String> {
        info!("[TestClient] Registering project at path: {}", path);

        // Send project.open_directory command (uses get-or-create pattern)
        let msg = WsClientMessage::ProjectCommand {
            method: "project.open_directory".to_string(),
            params: json!({
                "path": path,
            }),
        };

        let json_str = serde_json::to_string(&msg)?;
        {
            let mut sender = self.sender.lock().await;
            sender.send(Message::Text(json_str.into())).await?;
        }

        // Wait for response with project ID
        let start = Instant::now();
        let timeout = Duration::from_secs(10);

        loop {
            tokio::select! {
                event = self.event_rx.recv() => {
                    match event {
                        Some(BackendEvent::SessionData { response_type, data }) => {
                            // project.open_directory returns "directory_opened"
                            if response_type == "directory_opened" {
                                if let Some(project) = data.get("project") {
                                    if let Some(id) = project.get("id").and_then(|v| v.as_str()) {
                                        info!("[TestClient] Project registered with ID: {}", id);
                                        self.project_id = Some(id.to_string());
                                        // Use Project mode since we now have a registered project
                                        self.access_mode = SystemAccessMode::Project;
                                        return Ok(id.to_string());
                                    }
                                }
                                return Err(anyhow::anyhow!("Directory opened but project ID not found"));
                            }
                            continue;
                        }
                        Some(BackendEvent::Error { message, .. }) => {
                            return Err(anyhow::anyhow!("Project registration failed: {}", message));
                        }
                        Some(_) => continue,
                        None => {
                            return Err(anyhow::anyhow!("Event channel closed while registering project"));
                        }
                    }
                }
                _ = tokio::time::sleep(timeout.saturating_sub(start.elapsed())) => {
                    return Err(anyhow::anyhow!("Timeout registering project"));
                }
            }
        }
    }

    /// Send a chat message and capture all events until completion
    pub async fn send_and_capture(&mut self, prompt: &str, force_tool: Option<String>) -> Result<CapturedEvents> {
        self.send_and_capture_with_timeout(prompt, self.timeout, force_tool).await
    }

    /// Send a chat message with custom timeout
    pub async fn send_and_capture_with_timeout(
        &mut self,
        prompt: &str,
        timeout: Duration,
        force_tool: Option<String>,
    ) -> Result<CapturedEvents> {
        info!("[TestClient] Sending prompt: {}, force_tool: {:?}", &prompt[..prompt.len().min(100)], force_tool);

        // Clear event log
        self.event_log.clear();

        // Send the message with session_id for isolation
        let msg = WsClientMessage::Chat {
            content: prompt.to_string(),
            project_id: self.project_id.clone(),
            session_id: self.session_id.clone(),
            system_access_mode: self.access_mode.clone(),
            metadata: None,
            force_tool, // Pass through for deterministic tool testing
        };

        let json = serde_json::to_string(&msg)?;
        {
            let mut sender = self.sender.lock().await;
            sender.send(Message::Text(json.into())).await?;
        }

        // Capture events until completion or timeout
        let mut sequence = 0;
        let start = Instant::now();

        loop {
            tokio::select! {
                event = self.event_rx.recv() => {
                    match event {
                        Some(e) => {
                            debug!("[TestClient] Received event: {:?}", std::mem::discriminant(&e));

                            self.event_log.push(CapturedEvent {
                                event: e.clone(),
                                timestamp: Instant::now(),
                                sequence,
                            });
                            sequence += 1;

                            if self.is_terminal_event(&e) {
                                info!("[TestClient] Operation completed after {} events", sequence);
                                break;
                            }
                        }
                        None => {
                            warn!("[TestClient] Event channel closed");
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(timeout.saturating_sub(start.elapsed())) => {
                    return Err(anyhow::anyhow!(
                        "Timeout after {:?} waiting for completion ({} events received)",
                        timeout,
                        self.event_log.len()
                    ));
                }
            }
        }

        Ok(CapturedEvents::new(self.event_log.clone()))
    }

    /// Check if an event is terminal (operation complete)
    fn is_terminal_event(&self, event: &BackendEvent) -> bool {
        matches!(
            event,
            BackendEvent::ChatComplete { .. }
                | BackendEvent::Disconnected
                | BackendEvent::OperationEvent(OperationEvent::Completed { .. })
                | BackendEvent::OperationEvent(OperationEvent::Failed { .. })
        )
    }

    /// Close the connection
    pub async fn close(&mut self) -> Result<()> {
        let mut sender = self.sender.lock().await;
        sender.send(Message::Close(None)).await?;
        self.connected = false;
        Ok(())
    }

    /// Parse a WebSocket message into a BackendEvent
    /// (Reuses logic from MiraClient)
    fn parse_message(text: &str) -> Option<BackendEvent> {
        // Delegate to the existing parsing logic
        crate::cli::ws_client::MiraClient::parse_message_static(text)
    }
}

// Add a static parse method to MiraClient that we can call
impl crate::cli::ws_client::MiraClient {
    /// Static version of parse_message for use by TestClient
    pub fn parse_message_static(text: &str) -> Option<BackendEvent> {
        // Try parsing as JSON first
        let json: serde_json::Value = serde_json::from_str(text).ok()?;

        // Check the "type" field
        let msg_type = json.get("type")?.as_str()?;

        // Handle operation events (type starts with "operation.")
        if msg_type.starts_with("operation.") {
            return Self::parse_operation_event_static(msg_type, &json);
        }

        // Handle standard WsServerMessage types
        match msg_type {
            "stream" => {
                let delta = json.get("delta")?.as_str()?;
                Some(BackendEvent::StreamToken(delta.to_string()))
            }
            "chat_complete" => {
                let content = json.get("content")?.as_str()?.to_string();
                let artifacts = json
                    .get("artifacts")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.clone())
                    .unwrap_or_default();
                let thinking = json
                    .get("thinking")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                Some(BackendEvent::ChatComplete {
                    content,
                    artifacts,
                    thinking,
                })
            }
            "status" => {
                let message = json.get("message")?.as_str()?.to_string();
                let detail = json
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                Some(BackendEvent::Status { message, detail })
            }
            "error" => {
                let message = json.get("message")?.as_str()?.to_string();
                let code = json
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                Some(BackendEvent::Error { message, code })
            }
            "connection_ready" => Some(BackendEvent::Connected),
            "data" => {
                if let Some(inner_data) = json.get("data") {
                    if let Some(inner_type) = inner_data.get("type").and_then(|v| v.as_str()) {
                        if inner_type.starts_with("operation.") {
                            return Self::parse_operation_event_static(inner_type, inner_data);
                        }
                        // Handle session responses
                        if inner_type.starts_with("session") {
                            return Some(BackendEvent::SessionData {
                                response_type: inner_type.to_string(),
                                data: inner_data.clone(),
                            });
                        }
                        // Handle project responses (directory_opened, project_created, etc.)
                        if inner_type.starts_with("directory")
                            || inner_type.starts_with("project")
                            || inner_type.starts_with("artifact")
                            || inner_type.starts_with("guidelines")
                        {
                            return Some(BackendEvent::SessionData {
                                response_type: inner_type.to_string(),
                                data: inner_data.clone(),
                            });
                        }
                    }
                }
                None
            }
            "pong" => None,
            _ => None,
        }
    }

    fn parse_operation_event_static(msg_type: &str, json: &serde_json::Value) -> Option<BackendEvent> {
        let operation_id = json
            .get("operation_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let event = match msg_type {
            "operation.started" => OperationEvent::Started { operation_id },
            "operation.streaming" => {
                let content = json
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                OperationEvent::Streaming { operation_id, content }
            }
            "operation.tool_executed" => {
                let tool_name = json.get("tool_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let tool_type = json.get("tool_type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let summary = json.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let success = json.get("success").and_then(|v| v.as_bool()).unwrap_or(true);
                let duration_ms = json.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                OperationEvent::ToolExecuted {
                    operation_id, tool_name, tool_type, summary, success, duration_ms,
                }
            }
            "operation.completed" => {
                let result = json.get("result").and_then(|v| v.as_str()).map(|s| s.to_string());
                OperationEvent::Completed { operation_id, result }
            }
            "operation.failed" => {
                let error = json.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error").to_string();
                OperationEvent::Failed { operation_id, error }
            }
            "operation.thinking" => {
                let status = json.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let message = json.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let tokens_in = json.get("tokens_in").and_then(|v| v.as_i64()).unwrap_or(0);
                let tokens_out = json.get("tokens_out").and_then(|v| v.as_i64()).unwrap_or(0);
                let active_tool = json.get("active_tool").and_then(|v| v.as_str()).map(|s| s.to_string());
                OperationEvent::Thinking {
                    operation_id, status, message, tokens_in, tokens_out, active_tool,
                }
            }
            _ => return None,
        };

        Some(BackendEvent::OperationEvent(event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_captured_events_empty() {
        let events = CapturedEvents::new(vec![]);
        assert!(events.is_empty());
        assert_eq!(events.len(), 0);
        assert_eq!(events.duration(), Duration::ZERO);
    }

    #[test]
    fn test_captured_events_query() {
        let events = CapturedEvents::new(vec![
            CapturedEvent {
                event: BackendEvent::Connected,
                timestamp: Instant::now(),
                sequence: 0,
            },
            CapturedEvent {
                event: BackendEvent::OperationEvent(OperationEvent::Started {
                    operation_id: "op1".to_string(),
                }),
                timestamp: Instant::now(),
                sequence: 1,
            },
        ]);

        assert_eq!(events.len(), 2);
        assert_eq!(events.of_type("connected").len(), 1);
        assert_eq!(events.of_type("operation.started").len(), 1);
    }
}
