// src/api/types.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMetadata {
    #[serde(default)]
    pub output: String,  // Optional preview/summary
    pub mood: String,
    pub salience: usize,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: String,
    pub summary: String,
    #[serde(default)]
    pub monologue: Option<String>,
    #[serde(default)]
    pub reasoning_summary: Option<String>,
}

impl Default for ResponseMetadata {
    fn default() -> Self {
        Self {
            output: String::new(),
            mood: "present".to_string(),
            salience: 5,
            memory_type: "event".to_string(),
            tags: vec![],
            intent: "response".to_string(),
            summary: "Response to user".to_string(),
            monologue: None,
            reasoning_summary: None,
        }
    }
}
