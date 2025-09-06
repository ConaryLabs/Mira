// src/api/ws/message.rs
// Defines the data structures for WebSocket client and server messages.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Contains metadata about the user's context, such as the file being viewed.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageMetadata {
    pub file_path: Option<String>,
    pub repo_id: Option<String>,
    pub attachment_id: Option<String>,
    pub language: Option<String>,
    pub selection: Option<TextSelection>,
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
#[serde(tag = "type")]
pub enum WsClientMessage {
    /// A standard chat message, potentially with file context.
    #[serde(rename = "chat")]
    Chat {
        content: String,
        project_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<MessageMetadata>,
    },
    
    /// A command for control messages, such as heartbeats.
    #[serde(rename = "command")]
    Command {
        command: String,
        args: Option<Value>,
    },
    
    /// A general status message from the client.
    #[serde(rename = "status")]
    Status {
        message: String,
    },
    
    /// Indicates whether the user is currently typing.
    #[serde(rename = "typing")]
    Typing {
        active: bool,
    },
}

/// Represents all possible messages sent from the server to the client (frontend).
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WsServerMessage {
    /// A part of a streaming text response.
    #[serde(rename = "stream_chunk")]
    StreamChunk { text: String },
    
    /// Signals the end of a streaming response.
    #[serde(rename = "stream_end")]
    StreamEnd,
    
    /// The final message in a response, containing all metadata.
    #[serde(rename = "complete")]
    Complete {
        mood: Option<String>,
        salience: Option<f32>,
        tags: Option<Vec<String>>,
    },
    
    /// A general status update for the client UI.
    /// FIXED: Added back the detail field that Gemini removed
    #[serde(rename = "status")]
    Status { 
        message: String,
        detail: Option<String>,  // FIXED: Re-added this field
    },
    
    /// An error message.
    #[serde(rename = "error")]
    Error { message: String, code: String },
    
    /// Signals that the server is connected and ready.
    #[serde(rename = "connection_ready")]
    ConnectionReady,
    
    /// A pong response to a client's ping for heartbeats.
    #[serde(rename = "pong")]
    Pong,
    
    /// Signals that a tool-enabled response is finished.
    #[serde(rename = "done")]
    Done,

    /// A message containing the result of an image generation tool.
    #[serde(rename = "image_generated")]
    ImageGenerated {
        urls: Vec<String>,
        revised_prompt: Option<String>,
    },
}
