// src/api/ws/message.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WsClientMessage {
    // Chat message variant
    #[serde(rename = "chat")]
    Chat {
        content: String,
        project_id: Option<String>,
    },
    // Command variant for control messages
    #[serde(rename = "command")]
    Command {
        command: String,
        args: Option<serde_json::Value>,
    },
    // Status variant for heartbeat/status
    #[serde(rename = "status")]
    Status {
        message: String,
    },
    // Legacy variants for backward compatibility
    #[serde(rename = "message")]
    Message {
        content: String,
        persona: Option<String>, // DEPRECATED
        project_id: Option<String>,
    },
    #[serde(rename = "typing")]
    Typing {
        active: bool,
    },
}

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
