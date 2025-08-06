// src/llm/mod.rs
pub mod chat;
pub mod client;
pub mod embeddings;
pub mod emotional_weight;
pub mod intent;
pub mod memory_eval;
pub mod moderation;
pub mod persona;
pub mod prompts;
pub mod responses;  // Renamed from assistant
pub mod schema;
pub mod streaming;

pub use client::OpenAIClient;
pub use responses::{
    ResponsesManager,
    VectorStoreManager,
    ThreadManager,
};
// Re-export schema types for services
pub use schema::{EvaluateMemoryRequest, MiraStructuredReply, EvaluateMemoryResponse, function_schema};
