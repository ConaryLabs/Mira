// src/api/ws/message.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    #[serde(rename = "switch_persona")]
    SwitchPersona {
        persona: String,
        #[serde(default)]
        smooth_transition: bool,  // If true, blend mood gradually
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
        persona: String,
        mood: Option<String>,
    },
    #[serde(rename = "aside")]
    Aside {
        emotional_cue: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        intensity: Option<f32>,  // 0.0-1.0 for emotional intensity
    },
    #[serde(rename = "persona_update")]
    PersonaUpdate {
        persona: String,
        mood: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        transition_note: Option<String>,  // e.g. "*shifts to a warmer tone*"
    },
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
