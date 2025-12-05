// backend/src/cli/ws_client.rs
// WebSocket client for connecting to Mira backend

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::api::ws::message::{MessageMetadata, WsClientMessage};
use crate::api::ws::session::ChatSession;

/// Events received from the backend
#[derive(Debug, Clone)]
pub enum BackendEvent {
    /// Connection established
    Connected,
    /// Streaming token
    StreamToken(String),
    /// Chat completed
    ChatComplete {
        content: String,
        artifacts: Vec<serde_json::Value>,
        thinking: Option<String>,
    },
    /// Operation event (tool execution, agent, etc.)
    OperationEvent(OperationEvent),
    /// Status update
    Status { message: String, detail: Option<String> },
    /// Error from backend
    Error { message: String, code: String },
    /// Connection closed
    Disconnected,
    /// Session data response
    SessionData {
        response_type: String,
        data: serde_json::Value,
    },
}

/// Operation events from backend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum OperationEvent {
    Started {
        operation_id: String,
    },
    Streaming {
        operation_id: String,
        content: String,
    },
    PlanGenerated {
        operation_id: String,
        plan_text: String,
        reasoning_tokens: Option<u32>,
    },
    ToolExecuted {
        operation_id: String,
        tool_name: String,
        tool_type: String,
        summary: String,
        success: bool,
        duration_ms: u64,
    },
    ArtifactPreview {
        operation_id: String,
        artifact_id: String,
        path: Option<String>,
        preview: String,
    },
    ArtifactCompleted {
        operation_id: String,
        artifact: serde_json::Value,
    },
    TaskCreated {
        operation_id: String,
        task_id: String,
        title: String,
    },
    TaskStarted {
        operation_id: String,
        task_id: String,
    },
    TaskCompleted {
        operation_id: String,
        task_id: String,
    },
    AgentSpawned {
        operation_id: String,
        agent_id: String,
        agent_name: String,
        task: String,
    },
    AgentProgress {
        operation_id: String,
        agent_id: String,
        agent_name: String,
        iteration: u32,
        max_iterations: u32,
        current_activity: String,
    },
    AgentStreaming {
        operation_id: String,
        agent_id: String,
        content: String,
    },
    AgentCompleted {
        operation_id: String,
        agent_id: String,
        agent_name: String,
        result: String,
    },
    Completed {
        operation_id: String,
        result: Option<String>,
    },
    Failed {
        operation_id: String,
        error: String,
    },
    SudoApprovalRequired {
        operation_id: String,
        approval_request_id: String,
        command: String,
        reason: Option<String>,
    },
    Thinking {
        operation_id: String,
        status: String,
        message: String,
        tokens_in: i64,
        tokens_out: i64,
        active_tool: Option<String>,
    },
}

/// Mira WebSocket client
pub struct MiraClient {
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
    /// Connection status
    connected: bool,
}

impl MiraClient {
    /// Connect to the Mira backend
    pub async fn connect(url: &str) -> Result<Self> {
        let (ws_stream, _) = connect_async(url)
            .await
            .with_context(|| format!("Failed to connect to backend at {}", url))?;

        let (sender, mut receiver) = ws_stream.split();
        let sender = Arc::new(Mutex::new(sender));

        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(100);

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

        Ok(Self {
            sender,
            event_rx,
            project_id: None,
            connected: true,
        })
    }

    /// Parse a WebSocket message into a BackendEvent
    fn parse_message(text: &str) -> Option<BackendEvent> {
        // Try parsing as JSON first
        let json: serde_json::Value = serde_json::from_str(text).ok()?;

        // Check the "type" field
        let msg_type = json.get("type")?.as_str()?;

        // Handle operation events (type starts with "operation.")
        if msg_type.starts_with("operation.") {
            return Self::parse_operation_event(msg_type, &json);
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
            "sudo_approval_required" => {
                // Top-level sudo approval required message
                let approval_request_id = json
                    .get("approval_request_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let operation_id = json
                    .get("operation_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let command = json
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let reason = json
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                Some(BackendEvent::OperationEvent(OperationEvent::SudoApprovalRequired {
                    operation_id,
                    approval_request_id,
                    command,
                    reason,
                }))
            }
            "sudo_approval_response" => {
                // Sudo approval response - we can ignore this in CLI as we initiated it
                None
            }
            "data" => {
                // Data messages contain nested operation events or session data
                if let Some(inner_data) = json.get("data") {
                    if let Some(inner_type) = inner_data.get("type").and_then(|v| v.as_str()) {
                        if inner_type.starts_with("operation.") {
                            return Self::parse_operation_event(inner_type, inner_data);
                        }
                        // Handle session responses
                        if inner_type.starts_with("session") {
                            return Some(BackendEvent::SessionData {
                                response_type: inner_type.to_string(),
                                data: inner_data.clone(),
                            });
                        }
                    }
                }
                // Fallback for other data messages
                None
            }
            "pong" => {
                // Heartbeat response - ignore silently
                None
            }
            _ => {
                // Unknown message type - log in verbose mode but don't fail
                tracing::debug!("Unknown message type: {}", msg_type);
                None
            }
        }
    }

    /// Parse an operation event
    fn parse_operation_event(msg_type: &str, json: &serde_json::Value) -> Option<BackendEvent> {
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
                OperationEvent::Streaming {
                    operation_id,
                    content,
                }
            }
            "operation.plan_generated" => {
                let plan_text = json
                    .get("plan_text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let reasoning_tokens = json
                    .get("reasoning_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                OperationEvent::PlanGenerated {
                    operation_id,
                    plan_text,
                    reasoning_tokens,
                }
            }
            "operation.tool_executed" => {
                let tool_name = json
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_type = json
                    .get("tool_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let summary = json
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let success = json
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let duration_ms = json
                    .get("duration_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                OperationEvent::ToolExecuted {
                    operation_id,
                    tool_name,
                    tool_type,
                    summary,
                    success,
                    duration_ms,
                }
            }
            "operation.completed" => {
                let result = json
                    .get("result")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                OperationEvent::Completed {
                    operation_id,
                    result,
                }
            }
            "operation.failed" => {
                let error = json
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown error")
                    .to_string();
                OperationEvent::Failed {
                    operation_id,
                    error,
                }
            }
            "operation.agent_spawned" => {
                let agent_id = json
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let agent_name = json
                    .get("agent_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let task = json
                    .get("task")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                OperationEvent::AgentSpawned {
                    operation_id,
                    agent_id,
                    agent_name,
                    task,
                }
            }
            "operation.agent_progress" => {
                let agent_id = json
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let agent_name = json
                    .get("agent_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let iteration = json
                    .get("iteration")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let max_iterations = json
                    .get("max_iterations")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let current_activity = json
                    .get("current_activity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                OperationEvent::AgentProgress {
                    operation_id,
                    agent_id,
                    agent_name,
                    iteration,
                    max_iterations,
                    current_activity,
                }
            }
            "operation.agent_completed" => {
                let agent_id = json
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let agent_name = json
                    .get("agent_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let result = json
                    .get("result")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                OperationEvent::AgentCompleted {
                    operation_id,
                    agent_id,
                    agent_name,
                    result,
                }
            }
            "operation.sudo_approval_required" => {
                let approval_request_id = json
                    .get("approval_request_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let command = json
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let reason = json
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                OperationEvent::SudoApprovalRequired {
                    operation_id,
                    approval_request_id,
                    command,
                    reason,
                }
            }
            "operation.thinking" => {
                let status = json
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let message = json
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tokens_in = json
                    .get("tokens_in")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let tokens_out = json
                    .get("tokens_out")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let active_tool = json
                    .get("active_tool")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                OperationEvent::Thinking {
                    operation_id,
                    status,
                    message,
                    tokens_in,
                    tokens_out,
                    active_tool,
                }
            }
            _ => {
                tracing::debug!("Unknown operation event type: {}", msg_type);
                return None;
            }
        };

        Some(BackendEvent::OperationEvent(event))
    }


    /// Send a chat message to the backend
    pub async fn send_chat(&mut self, content: &str, metadata: Option<MessageMetadata>) -> Result<()> {
        let msg = WsClientMessage::Chat {
            content: content.to_string(),
            project_id: self.project_id.clone(),
            metadata,
        };

        self.send_message(&msg).await
    }

    /// Send a command to the backend
    pub async fn send_command(&mut self, command: &str, args: Option<serde_json::Value>) -> Result<()> {
        let msg = WsClientMessage::Command {
            command: command.to_string(),
            args,
        };

        self.send_message(&msg).await
    }

    /// Send a raw message to the backend
    async fn send_message(&self, msg: &WsClientMessage) -> Result<()> {
        let json = serde_json::to_string(msg)
            .context("Failed to serialize message")?;

        let mut sender = self.sender.lock().await;
        sender.send(Message::Text(json.into())).await
            .context("Failed to send message to backend")?;

        Ok(())
    }

    /// Receive the next event from the backend
    pub async fn recv(&mut self) -> Option<BackendEvent> {
        self.event_rx.recv().await
    }

    /// Set the current project ID
    pub fn set_project_id(&mut self, project_id: Option<String>) {
        self.project_id = project_id;
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Close the connection
    pub async fn close(&mut self) -> Result<()> {
        let mut sender = self.sender.lock().await;
        sender.send(Message::Close(None)).await
            .context("Failed to send close message")?;
        self.connected = false;
        Ok(())
    }

    // ========================================================================
    // SESSION MANAGEMENT METHODS
    // ========================================================================

    /// Send a session command and return the raw JSON response
    async fn send_session_command(&self, method: &str, params: serde_json::Value) -> Result<()> {
        let msg = WsClientMessage::SessionCommand {
            method: method.to_string(),
            params,
        };

        self.send_message(&msg).await
    }

    /// Create a new session
    pub async fn create_session(
        &mut self,
        name: Option<&str>,
        project_path: Option<&str>,
    ) -> Result<ChatSession> {
        self.send_session_command("session.create", serde_json::json!({
            "name": name,
            "project_path": project_path,
        })).await?;

        // Wait for response
        self.wait_for_session_response("session_created").await
    }

    /// List sessions with optional filters
    pub async fn list_sessions(
        &mut self,
        project_path: Option<&str>,
        search: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<ChatSession>> {
        self.send_session_command("session.list", serde_json::json!({
            "project_path": project_path,
            "search": search,
            "limit": limit,
        })).await?;

        // Wait for response
        let data = self.wait_for_session_data("session_list").await?;
        let sessions: Vec<ChatSession> = serde_json::from_value(
            data.get("sessions").cloned().unwrap_or(serde_json::json!([]))
        ).context("Failed to parse sessions")?;
        Ok(sessions)
    }

    /// Get a session by ID
    pub async fn get_session(&mut self, id: &str) -> Result<ChatSession> {
        self.send_session_command("session.get", serde_json::json!({
            "id": id,
        })).await?;

        self.wait_for_session_response("session").await
    }

    /// Update a session's name
    pub async fn update_session(&mut self, id: &str, name: Option<&str>) -> Result<ChatSession> {
        self.send_session_command("session.update", serde_json::json!({
            "id": id,
            "name": name,
        })).await?;

        self.wait_for_session_response("session_updated").await
    }

    /// Delete a session
    pub async fn delete_session(&mut self, id: &str) -> Result<()> {
        self.send_session_command("session.delete", serde_json::json!({
            "id": id,
        })).await?;

        // Wait for status response
        loop {
            match self.recv().await {
                Some(BackendEvent::Status { message, .. }) => {
                    if message.contains("deleted") {
                        return Ok(());
                    }
                }
                Some(BackendEvent::Error { message, .. }) => {
                    return Err(anyhow::anyhow!(message));
                }
                None => return Err(anyhow::anyhow!("Connection closed")),
                _ => continue,
            }
        }
    }

    /// Fork a session
    pub async fn fork_session(&mut self, source_id: &str, name: Option<&str>) -> Result<ChatSession> {
        self.send_session_command("session.fork", serde_json::json!({
            "source_id": source_id,
            "name": name,
        })).await?;

        self.wait_for_session_response("session_forked").await
    }

    /// Wait for a session response and extract the session object
    async fn wait_for_session_response(&mut self, expected_type: &str) -> Result<ChatSession> {
        let data = self.wait_for_session_data(expected_type).await?;
        let session: ChatSession = serde_json::from_value(
            data.get("session").cloned().unwrap_or(serde_json::json!({}))
        ).context("Failed to parse session")?;
        Ok(session)
    }

    /// Wait for a session data response
    async fn wait_for_session_data(&mut self, expected_type: &str) -> Result<serde_json::Value> {
        loop {
            match self.recv().await {
                Some(BackendEvent::SessionData { response_type, data }) => {
                    if response_type == expected_type {
                        return Ok(data);
                    }
                }
                Some(BackendEvent::Error { message, .. }) => {
                    return Err(anyhow::anyhow!(message));
                }
                None => return Err(anyhow::anyhow!("Connection closed")),
                _ => continue,
            }
        }
    }

    // ========================================================================
    // SUDO APPROVAL METHODS
    // ========================================================================

    /// Approve a sudo command request
    pub async fn approve_sudo_request(&self, approval_request_id: &str) -> Result<()> {
        let msg = WsClientMessage::SudoCommand {
            method: "sudo.approve".to_string(),
            params: serde_json::json!({
                "approval_request_id": approval_request_id,
            }),
        };

        self.send_message(&msg).await
    }

    /// Deny a sudo command request
    pub async fn deny_sudo_request(&self, approval_request_id: &str, reason: Option<&str>) -> Result<()> {
        let msg = WsClientMessage::SudoCommand {
            method: "sudo.deny".to_string(),
            params: serde_json::json!({
                "approval_request_id": approval_request_id,
                "reason": reason,
            }),
        };

        self.send_message(&msg).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_stream_message() {
        let json = r#"{"type":"stream","delta":"Hello"}"#;
        let event = MiraClient::parse_message(json);
        assert!(matches!(event, Some(BackendEvent::StreamToken(s)) if s == "Hello"));
    }

    #[test]
    fn test_parse_error_message() {
        let json = r#"{"type":"error","message":"Test error","code":"test_error"}"#;
        let event = MiraClient::parse_message(json);
        assert!(matches!(
            event,
            Some(BackendEvent::Error { message, code })
            if message == "Test error" && code == "test_error"
        ));
    }

    #[test]
    fn test_parse_status_message() {
        let json = r#"{"type":"status","message":"Processing","detail":"Step 1"}"#;
        let event = MiraClient::parse_message(json);
        assert!(matches!(
            event,
            Some(BackendEvent::Status { message, detail })
            if message == "Processing" && detail == Some("Step 1".to_string())
        ));
    }
}
