// backend/src/memory/storage/qdrant/mod.rs

//! Qdrant vector database storage for embeddings

pub mod multi_store;

pub use multi_store::QdrantMultiStore;
