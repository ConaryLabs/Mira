// src/llm/classification.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Classification {
    /// Whether the message content is primarily code.
    pub is_code: bool,
    /// The programming language of the code, if applicable.
    pub lang: String,
    /// A list of topics or keywords that describe the content.
    pub topics: Vec<String>,
    /// A score from 0.0 to 1.0 indicating the importance of the message.
    pub salience: f32,
}
