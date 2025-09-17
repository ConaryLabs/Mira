// src/memory/storage/qdrant/mod.rs

//! Qdrant-backed memory store with multi-collection support for 4-head memory system.
//! Provides vector storage for semantic, code, summary, and document embeddings.

pub mod store;
pub mod search;
pub mod mapping;
pub mod multi_store;

// Re-export key types for convenience
pub use crate::llm::embeddings::EmbeddingHead;
