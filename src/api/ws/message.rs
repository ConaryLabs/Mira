// src/api/ws/message.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WsClientMessage {
    #[serde(rename = "message")]
    Message {
        content: String,
        persona: Option<String>,
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
        persona: String,
        mood: Option<String>,
    },
    #[serde(rename = "aside")]
    Aside {
        emotional_cue: String,
    },
    #[serde(rename = "persona_update")]
    PersonaUpdate {
        persona: String,
        mood: Option<String>,
    },
    #[serde(rename = "done")]
    Done,
    // (Optionally: errors, warnings, etc.)
}
