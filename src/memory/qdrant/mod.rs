//! Qdrant-backed memory store (semantic/long-term memory).
//! PHASE 1: Added multi-collection support for GPT-5 Robust Memory

pub mod store;
pub mod search;
pub mod mapping;
pub mod multi_store;  // PHASE 1: Multi-collection wrapper

// Re-export key types for convenience
pub use store::QdrantMemoryStore;
pub use multi_store::{QdrantMultiStore, EmbeddingHead}; // PHASE 1: New exports
