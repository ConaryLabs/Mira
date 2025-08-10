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
pub mod anthropic_client;  // New - Anthropic/Claude client
pub mod claude_system;      // New - Claude orchestration system

pub use client::OpenAIClient;
pub use responses::{
    ResponsesManager,
    VectorStoreManager,
    ThreadManager,
};
// Re-export schema types for services
pub use schema::{EvaluateMemoryRequest, MiraStructuredReply, EvaluateMemoryResponse, function_schema};

// New exports for Claude/Anthropic
pub use anthropic_client::AnthropicClient;
pub use claude_system::{ClaudeSystem, ClaudeDecision, ActionType};
