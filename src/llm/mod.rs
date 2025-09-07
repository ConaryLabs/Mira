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
pub mod responses; // Renamed from assistant
pub mod schema;
pub mod streaming;

// Export the main client and its components
pub use client::OpenAIClient;

// PHASE 2: Export new multi-head embedding components

// Export responses manager and related types

// Re-export schema types for services

