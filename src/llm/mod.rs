//! Barrel file: exports all LLM-related modules/types/functions for easy use.

pub mod openai;
pub mod schema;
pub mod intent;
pub mod emotional_weight; // NEW: for emotional weight detection

// Re-export commonly used items
pub use openai::OpenAIClient;
pub use schema::{
    EvaluateMemoryRequest,
    EvaluateMemoryResponse,
    MemoryType,
    function_schema,
    MiraStructuredReply, // add this so you can use crate::llm::MiraStructuredReply everywhere!
};
pub use intent::{
    ChatIntent,
    chat_intent_function_schema,
};

// --- No broken moderation helper ---
// To log moderation results, just use client.moderate(message).await in your handler.
