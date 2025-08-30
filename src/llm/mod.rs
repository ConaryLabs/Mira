// src/llm/mod.rs
// Updated to include refactored client modules and Phase 2 embedding components

pub mod chat;
pub mod client; // Refactored client with sub-modules
pub mod embeddings;
pub mod emotional_weight;
pub mod intent;
pub mod memory_eval;
pub mod moderation;
pub mod classification;
// pub mod persona; // Note: `persona` is typically a top-level module, not under `llm`.
pub mod prompts;
pub mod responses; // Renamed from assistant
pub mod schema;
pub mod streaming;

// Export the main client and its components
pub use client::{
    ClientConfig, EmbeddingClient, EmbeddingModel, EmbeddingUtils, ModelConfig, OpenAIClient,
    ResponseOutput, ResponseStream, StreamProcessor,
};

// PHASE 2: Export new multi-head embedding components
pub use embeddings::{EmbeddingHead, TextChunker};

// Export responses manager and related types
pub use responses::{ResponsesManager, ThreadManager, VectorStoreManager};

// Re-export schema types for services
pub use schema::{
    function_schema, EvaluateMemoryRequest, EvaluateMemoryResponse, MiraStructuredReply,
};

