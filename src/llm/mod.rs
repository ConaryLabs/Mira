// src/llm/mod.rs

//! Barrel file: exports all LLM-related modules/types/functions for easy use.

pub mod openai;
pub mod schema;

// In the future you can add:
// pub mod moderation;
// pub mod persona;
// pub mod prompts;
// ...etc.

pub use openai::OpenAIClient;
pub use schema::{
    EvaluateMemoryRequest,
    EvaluateMemoryResponse,
    MemoryType,
    function_schema,
};
