// backend/src/terminal/types.rs

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Terminal I/O message for WebSocket streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalMessage {
    /// Output from terminal (stdout/stderr)
    Output { data: Vec<u8> },
    /// Input to terminal (user keystrokes)
    Input { data: Vec<u8> },
    /// Terminal resize event
    Resize { cols: u16, rows: u16 },
    /// Terminal session closed
    Closed { exit_code: Option<i32> },
    /// Error occurred
    Error { message: String },
}

/// File metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub is_directory: bool,
    pub size: u64,
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
    pub permissions: Option<String>,
    pub is_hidden: bool,
}

/// Command execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

/// Terminal session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalConfig {
    /// Project this terminal belongs to
    pub project_id: String,
    /// Initial working directory (defaults to project path)
    pub working_directory: Option<PathBuf>,
    /// Shell to use (defaults to system shell)
    pub shell: Option<String>,
    /// Environment variables to set
    pub environment: Vec<(String, String)>,
    /// Terminal size
    pub cols: u16,
    pub rows: u16,
}

/// Terminal session info for database storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalSessionInfo {
    pub id: String,
    pub project_id: String,
    pub conversation_session_id: Option<String>,
    pub working_directory: String,
    pub shell: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub closed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub exit_code: Option<i32>,
}

/// Terminal operation errors
#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    #[error("File operation failed: {0}")]
    FileOperationFailed(String),

    #[error("Command execution failed: {0}")]
    CommandFailed(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Terminal error: {0}")]
    TerminalError(String),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

pub type TerminalResult<T> = Result<T, TerminalError>;
