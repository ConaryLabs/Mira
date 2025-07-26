// src/api/ws/message.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WsClientMessage {
    #[serde(rename = "message")]
    Message {
        content: String,
        persona: Option<String>, // Client can still request a persona internally
        project_id: Option<String>,
    },
    #[serde(rename = "typing")]
    Typing {
        active: bool,
    },
    #[serde(rename = "switch_persona")]
    SwitchPersona {
        persona: String,
        smooth_transition: bool, // Internal use only
    },
    #[serde(rename = "get_memory_stats")]
    GetMemoryStats {
        session_id: Option<String>,
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
    // No more PersonaUpdate messages - personas work silently
    #[serde(rename = "memory_stats")]
    MemoryStats {
        total_memories: usize,
        high_salience_count: usize,
        avg_salience: f32,
        mood_distribution: HashMap<String, usize>,
    },
    #[serde(rename = "error")]
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    #[serde(rename = "done")]
    Done,
}
