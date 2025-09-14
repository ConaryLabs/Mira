// src/llm/mod.rs
// LLM module exports and submodule declarations

pub mod chat;
pub mod chat_service;
pub mod classification;
pub mod client;
pub mod embeddings;
pub mod emotional_weight;
pub mod intent;
pub mod memory_eval;
pub mod moderation;
pub mod responses;
pub mod schema;

// Export the main client
pub use client::OpenAIClient;
