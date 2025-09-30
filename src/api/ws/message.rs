// src/api/ws/message.rs
// Defines the data structures for WebSocket client and server messages.

use serde::{Deserialize, Serialize};

/// Contains metadata about the user's context, such as the file being viewed.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageMetadata {
    // File and selection context
    pub file_path: Option<String>,
    pub file_content: Option<String>,  // ADDED: Critical field for actual file content
    pub repo_id: Option<String>,
    pub attachment_id: Option<String>,
    pub language: Option<String>,
    pub selection: Option<TextSelection>,
    
    // Project context fields sent by frontend
    pub project_name: Option<String>,
    pub has_repository: Option<bool>,
    pub repo_root: Option<String>,
    pub branch: Option<String>,
    pub request_repo_context: Option<bool>,
}

/// Represents a user's text selection in a file.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TextSelection {
    pub start_line: usize,
    pub end_line: usize,
    pub text: Option<String>,
}

/// Represents all possible messages sent from the client (frontend) to the server.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsClientMessage {
    Chat {
        content: String,
        project_id: Option<String>,
        metadata: Option<MessageMetadata>,
    },
    Command {
        command: String,
        args: Option<serde_json::Value>,
    },
    Status {
        message: String,
    },
    Typing {
        active: bool,
    },
    ProjectCommand {
        method: String,
        params: serde_json::Value,
    },
    MemoryCommand {
        method: String,
        params: serde_json::Value,
    },
    GitCommand {
        method: String,
        params: serde_json::Value,
    },
    FileSystemCommand {
        method: String,
        params: serde_json::Value,
    },
    FileTransfer {
        operation: String,
        data: serde_json::Value,
    },
    CodeIntelligenceCommand {
        method: String,
        params: serde_json::Value,
    },
    DocumentCommand {  // NEW: Document processing commands
        method: String,
        params: serde_json::Value,
    },
}

/// Represents all possible messages sent from the server to the client (frontend).
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WsServerMessage {
    /// A general response with data
    #[serde(rename = "response")]
    Response {
        data: serde_json::Value,
    },
    
    /// A general status update for the client UI
    #[serde(rename = "status")]
    Status { 
        message: String,
        detail: Option<String>,
    },
    
    /// An error message
    #[serde(rename = "error")]
    Error { 
        message: String, 
        code: String,
    },
    
    /// Signals that the server is connected and ready
    #[serde(rename = "connection_ready")]
    ConnectionReady,
    
    /// A pong response to a client's ping for heartbeats
    #[serde(rename = "pong")]
    Pong,
    
    /// A message containing the result of an image generation tool
    #[serde(rename = "image_generated")]
    ImageGenerated {
        urls: Vec<String>,
        revised_prompt: Option<String>,
    },
    
    /// A data response with optional request_id for matching
    #[serde(rename = "data")]
    Data {
        data: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
}
