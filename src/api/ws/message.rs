// src/api/ws/message.rs
// Enhanced WebSocket message types with metadata support for file context
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Enhanced client message with metadata support
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EnhancedClientMessage {
    pub content: String,
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MessageMetadata>,
}

/// Metadata about the current context (file being viewed, etc.)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageMetadata {
    pub file_path: Option<String>,
    pub repo_id: Option<String>,
    pub attachment_id: Option<String>,
    pub language: Option<String>,
    pub selection: Option<TextSelection>,
}

/// Text selection information
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TextSelection {
    pub start_line: usize,
    pub end_line: usize,
    pub text: Option<String>,
}

/// Client messages from frontend
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WsClientMessage {
    // Enhanced chat message with metadata
    #[serde(rename = "chat")]
    Chat {
        content: String,
        project_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<MessageMetadata>,
    },
    
    // Command variant for control messages
    #[serde(rename = "command")]
    Command {
        command: String,
        args: Option<Value>,
    },
    
    // Status variant for heartbeat/status
    #[serde(rename = "status")]
    Status {
        message: String,
    },
    
    // Typing indicator
    #[serde(rename = "typing")]
    Typing {
        active: bool,
    },
}

/// Server messages to frontend
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WsServerMessage {
    // Streaming chunk of response
    #[serde(rename = "chunk")]
    Chunk {
        content: String,
        mood: Option<String>,
    },
    
    // Completion message with metadata
    #[serde(rename = "complete")]
    Complete {
        mood: Option<String>,
        salience: Option<f32>,
        tags: Option<Vec<String>>,
    },
    
    // Status messages for commands
    #[serde(rename = "status")]
    Status {
        message: String,
        detail: Option<String>,
    },
    
    // Emotional aside (preserved from Phase 7)
    #[serde(rename = "aside")]
    Aside {
        emotional_cue: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        intensity: Option<f32>,
    },
    
    // Error messages
    #[serde(rename = "error")]
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    
    // End of stream marker
    #[serde(rename = "done")]
    Done,
}

impl WsClientMessage {
    /// Check if this is a heartbeat/pong message
    pub fn is_heartbeat(&self) -> bool {
        matches!(self, 
            WsClientMessage::Command { command, .. } if command == "pong" || command == "heartbeat"
        )
    }
    
    /// Extract content and metadata from any message variant
    pub fn extract_content_and_metadata(&self) -> (Option<String>, Option<String>, Option<MessageMetadata>) {
        match self {
            WsClientMessage::Chat { content, project_id, metadata } => {
                (Some(content.clone()), project_id.clone(), metadata.clone())
            }
            _ => (None, None, None)
        }
    }
}
