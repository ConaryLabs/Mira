// src/llm/mod.rs
// Updated to include refactored client modules

pub mod chat;
pub mod client;           // Refactored client with sub-modules
pub mod embeddings;
pub mod emotional_weight;
pub mod intent;
pub mod memory_eval;
pub mod moderation;
pub mod persona;
pub mod prompts;
pub mod responses;        // Renamed from assistant
pub mod schema;
pub mod streaming;

// Export the main client and its components
pub use client::{
    OpenAIClient,
    ClientConfig,
    ModelConfig,
    ResponseOutput,
    ResponseStream,
    StreamProcessor,
    EmbeddingClient,
    EmbeddingModel,
    EmbeddingUtils,
};

// Export responses manager and related types
pub use responses::{
    ResponsesManager,
    VectorStoreManager,
    ThreadManager,
};

// Re-export schema types for services
pub use schema::{
    EvaluateMemoryRequest, 
    MiraStructuredReply, 
    EvaluateMemoryResponse, 
    function_schema
};
