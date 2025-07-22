// src/llm/mod.rs

//! Barrel file: exports all LLM-related modules/types/functions for easy use.

pub mod openai;
pub mod schema;
pub mod intent;

// Re-export commonly used items
pub use openai::OpenAIClient;
pub use schema::{
    EvaluateMemoryRequest,
    EvaluateMemoryResponse,
    MemoryType,
    function_schema,
};
pub use intent::{
    ChatIntent,
    chat_intent_function_schema,
};
