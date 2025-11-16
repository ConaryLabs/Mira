// backend/src/api/ws/terminal.rs

use crate::api::error::{ApiError, ApiResult};
use crate::api::ws::message::WsServerMessage;
use crate::state::AppState;
use crate::terminal::{TerminalConfig, TerminalMessage, TerminalSessionInfo};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Terminal session manager for WebSocket connections
pub struct TerminalSessionManager {
    /// Active terminal sessions
    /// Key: terminal_session_id, Value: (project_id, input_sender)
    active_sessions: Arc<RwLock<HashMap<String, ActiveTerminal>>>,
}

struct ActiveTerminal {
    project_id: String,
    input_tx: tokio::sync::mpsc::Sender<TerminalMessage>,
}

impl TerminalSessionManager {
    pub fn new() -> Self {
        Self {
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start a new terminal session for a project
    pub async fn start_session(
        &self,
        project_id: String,
        working_directory: Option<PathBuf>,
        cols: u16,
        rows: u16,
        app_state: Arc<AppState>,
    ) -> ApiResult<(String, tokio::sync::mpsc::Receiver<TerminalMessage>)> {
        info!("Starting terminal session for project: {}", project_id);

        // Verify project exists
        let _project = app_state
            .project_store
            .get_project(&project_id)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to get project: {}", e)))?
            .ok_or_else(|| ApiError::not_found(format!("Project not found: {}", project_id)))?;

        // Determine working directory
        let working_dir = if let Some(wd) = working_directory {
            wd
        } else {
            // Try to get project's base path
            match app_state.git_store.get_project_base_path(&project_id).await {
                Ok(path) => path,
                Err(e) => {
                    warn!("Failed to get project base path: {}, using current dir", e);
                    std::env::current_dir().map_err(|e| {
                        ApiError::internal(format!("Failed to get current directory: {}", e))
                    })?
                }
            }
        };

        // Create terminal config
        let config = TerminalConfig {
            project_id: project_id.clone(),
            working_directory: Some(working_dir.clone()),
            shell: None, // Use default shell
            environment: Vec::new(),
            cols,
            rows,
        };

        // Create terminal session
        let session = crate::terminal::TerminalSession::new(config);
        let session_id = session.session_id().to_string();

        // Start shell
        let (input_tx, output_rx) = session
            .start_shell()
            .await
            .map_err(|e| ApiError::internal(format!("Failed to start shell: {}", e)))?;

        // Save session to database
        let session_info = TerminalSessionInfo {
            id: session_id.clone(),
            project_id: project_id.clone(),
            conversation_session_id: None, // Can be set later if needed
            working_directory: working_dir.to_string_lossy().to_string(),
            shell: None,
            created_at: chrono::Utc::now(),
            closed_at: None,
            exit_code: None,
        };

        app_state
            .terminal_store
            .create_session(&session_info)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to save session: {}", e)))?;

        // Store active session
        self.active_sessions.write().await.insert(
            session_id.clone(),
            ActiveTerminal {
                project_id,
                input_tx,
            },
        );

        info!("Terminal session started: {}", session_id);

        Ok((session_id, output_rx))
    }

    /// Send input to a terminal session
    pub async fn send_input(&self, session_id: &str, data: Vec<u8>) -> ApiResult<()> {
        debug!("Sending input to terminal {}: {} bytes", session_id, data.len());

        let sessions = self.active_sessions.read().await;
        let terminal = sessions
            .get(session_id)
            .ok_or_else(|| ApiError::not_found(format!("Terminal session not found: {}", session_id)))?;

        terminal
            .input_tx
            .send(TerminalMessage::Input { data })
            .await
            .map_err(|e| ApiError::internal(format!("Failed to send input: {}", e)))?;

        Ok(())
    }

    /// Resize a terminal session
    pub async fn resize(&self, session_id: &str, cols: u16, rows: u16) -> ApiResult<()> {
        debug!("Resizing terminal {} to {}x{}", session_id, cols, rows);

        let sessions = self.active_sessions.read().await;
        let terminal = sessions
            .get(session_id)
            .ok_or_else(|| ApiError::not_found(format!("Terminal session not found: {}", session_id)))?;

        terminal
            .input_tx
            .send(TerminalMessage::Resize { cols, rows })
            .await
            .map_err(|e| ApiError::internal(format!("Failed to send resize: {}", e)))?;

        Ok(())
    }

    /// Close a terminal session
    pub async fn close_session(
        &self,
        session_id: &str,
        app_state: Arc<AppState>,
    ) -> ApiResult<()> {
        info!("Closing terminal session: {}", session_id);

        // Remove from active sessions
        let terminal = self.active_sessions.write().await.remove(session_id);

        if let Some(terminal) = terminal {
            // Send close message
            let _ = terminal
                .input_tx
                .send(TerminalMessage::Closed { exit_code: None })
                .await;

            // Update database
            app_state
                .terminal_store
                .close_session(session_id, None)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to close session: {}", e)))?;

            Ok(())
        } else {
            Err(ApiError::not_found(format!(
                "Terminal session not found: {}",
                session_id
            )))
        }
    }

    /// Check if a session is active
    pub async fn is_active(&self, session_id: &str) -> bool {
        self.active_sessions.read().await.contains_key(session_id)
    }

    /// Get project ID for a terminal session
    pub async fn get_project_id(&self, session_id: &str) -> Option<String> {
        self.active_sessions
            .read()
            .await
            .get(session_id)
            .map(|t| t.project_id.clone())
    }
}

impl Default for TerminalSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// WebSocket message handlers for terminal operations

#[derive(Debug, Deserialize)]
pub struct StartSessionParams {
    pub project_id: String,
    pub working_directory: Option<String>,
    #[serde(default = "default_cols")]
    pub cols: u16,
    #[serde(default = "default_rows")]
    pub rows: u16,
}

fn default_cols() -> u16 {
    80
}
fn default_rows() -> u16 {
    24
}

#[derive(Debug, Serialize)]
pub struct StartSessionResponse {
    pub session_id: String,
    pub project_id: String,
    pub working_directory: String,
}

#[derive(Debug, Deserialize)]
pub struct SendInputParams {
    pub session_id: String,
    pub data: String, // Base64 encoded
}

#[derive(Debug, Deserialize)]
pub struct ResizeParams {
    pub session_id: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Deserialize)]
pub struct CloseSessionParams {
    pub session_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ListSessionsParams {
    pub project_id: String,
    pub active_only: Option<bool>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TerminalSessionListItem {
    pub id: String,
    pub project_id: String,
    pub working_directory: String,
    pub created_at: String,
    pub closed_at: Option<String>,
    pub is_active: bool,
}

/// Handle terminal.start_session command
pub async fn handle_start_session(
    params: Value,
    manager: Arc<TerminalSessionManager>,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    let params: StartSessionParams = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid parameters: {}", e)))?;

    let working_dir = params
        .working_directory
        .as_ref()
        .map(|s| PathBuf::from(s));

    let (session_id, mut output_rx) = manager
        .start_session(
            params.project_id.clone(),
            working_dir.clone(),
            params.cols,
            params.rows,
            app_state.clone(),
        )
        .await?;

    let working_directory = working_dir
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .to_string_lossy()
        .to_string();

    // Spawn task to forward terminal output to WebSocket
    let session_id_clone = session_id.clone();
    tokio::spawn(async move {
        while let Some(msg) = output_rx.recv().await {
            match msg {
                TerminalMessage::Output { data } => {
                    // TODO: Send to WebSocket connection
                    // This will be handled by the WebSocket connection handler
                    debug!(
                        "Terminal {} output: {} bytes",
                        session_id_clone,
                        data.len()
                    );
                }
                TerminalMessage::Closed { exit_code } => {
                    info!("Terminal {} closed with exit code: {:?}", session_id_clone, exit_code);
                    break;
                }
                TerminalMessage::Error { message } => {
                    error!("Terminal {} error: {}", session_id_clone, message);
                    break;
                }
                _ => {}
            }
        }
    });

    Ok(WsServerMessage::Data {
        data: serde_json::to_value(StartSessionResponse {
            session_id,
            project_id: params.project_id,
            working_directory,
        })
        .unwrap(),
        request_id: None,
    })
}

/// Handle terminal.send_input command
pub async fn handle_send_input(
    params: Value,
    manager: Arc<TerminalSessionManager>,
) -> ApiResult<WsServerMessage> {
    let params: SendInputParams = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid parameters: {}", e)))?;

    // Decode base64 data
    let data = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        params.data.as_bytes(),
    )
    .map_err(|e| ApiError::bad_request(format!("Invalid base64 data: {}", e)))?;

    manager.send_input(&params.session_id, data).await?;

    Ok(WsServerMessage::Data {
        data: serde_json::json!({ "success": true }),
        request_id: None,
    })
}

/// Handle terminal.resize command
pub async fn handle_resize(
    params: Value,
    manager: Arc<TerminalSessionManager>,
) -> ApiResult<WsServerMessage> {
    let params: ResizeParams = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid parameters: {}", e)))?;

    manager
        .resize(&params.session_id, params.cols, params.rows)
        .await?;

    Ok(WsServerMessage::Data {
        data: serde_json::json!({ "success": true }),
        request_id: None,
    })
}

/// Handle terminal.close_session command
pub async fn handle_close_session(
    params: Value,
    manager: Arc<TerminalSessionManager>,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    let params: CloseSessionParams = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid parameters: {}", e)))?;

    manager.close_session(&params.session_id, app_state).await?;

    Ok(WsServerMessage::Data {
        data: serde_json::json!({ "success": true }),
        request_id: None,
    })
}

/// Handle terminal.list_sessions command
pub async fn handle_list_sessions(
    params: Value,
    manager: Arc<TerminalSessionManager>,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    let params: ListSessionsParams = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid parameters: {}", e)))?;

    let active_only = params.active_only.unwrap_or(false);

    let sessions = if active_only {
        app_state
            .terminal_store
            .list_active_sessions(&params.project_id)
            .await
    } else {
        app_state
            .terminal_store
            .list_project_sessions(&params.project_id, params.limit)
            .await
    }
    .map_err(|e| ApiError::internal(format!("Failed to list sessions: {}", e)))?;

    let mut session_list = Vec::new();

    for session in sessions {
        let is_active = manager.is_active(&session.id).await;

        session_list.push(TerminalSessionListItem {
            id: session.id,
            project_id: session.project_id,
            working_directory: session.working_directory,
            created_at: session.created_at.to_rfc3339(),
            closed_at: session.closed_at.map(|t| t.to_rfc3339()),
            is_active,
        });
    }

    Ok(WsServerMessage::Data {
        data: serde_json::json!({ "sessions": session_list }),
        request_id: None,
    })
}
