// backend/src/memory/storage/mod.rs

//! Storage backends for memory system
//!
//! Provides SQLite for structured data and Qdrant for vector search.

pub mod qdrant;
pub mod sqlite;
