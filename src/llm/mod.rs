// src/llm/mod.rs

//! LLM module - OpenAI integration split into focused submodules

pub mod client;
pub mod chat;
pub mod embeddings;
pub mod moderation;
pub mod memory_eval;
pub mod streaming;
pub mod schema;
pub mod intent;
pub mod emotional_weight;
pub mod assistant;

// Re-export the main client and commonly used types
pub use client::OpenAIClient;
pub use moderation::ModerationResult;
pub use schema::{
    EvaluateMemoryRequest,
    EvaluateMemoryResponse,
    MemoryType,
    function_schema,
    MiraStructuredReply,
    ChatResponse,  // Also export ChatResponse since it's used in services
};
pub use intent::{
    ChatIntent,
    chat_intent_function_schema,
};

// Re-export assistant module types for convenience
pub use assistant::{
    AssistantManager,
    VectorStoreManager,
    ThreadManager,
};
