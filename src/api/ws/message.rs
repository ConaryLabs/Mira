// src/api/ws/message.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WsClientMessage {
    #[serde(rename = "message")]
    Message {
        content: String,
        persona: Option<String>, // DEPRECATED - personas emerge naturally from context
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
    #[serde(rename = "chunk")]
    Chunk {
        content: String,
        mood: Option<String>, // Mood is visible, persona is not
    },
    #[serde(rename = "aside")]
    Aside {
        emotional_cue: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        intensity: Option<f32>,
    },
    // No more PersonaUpdate or MemoryStats - keeping it real
    #[serde(rename = "error")]
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    #[serde(rename = "done")]
    Done,
}
