// src/memory/storage/sqlite/mod.rs

//! SQLite-backed memory store for message persistence and analysis.
//! Provides the source of truth for all memory data with support for:
//! - Message threading via parent/response relationships
//! - AI-powered message analysis (mood, salience, topics)
//! - Integration with multi-head Qdrant vector storage

pub mod store;
pub mod query;

// Re-export the main store for convenience
pub use store::SqliteMemoryStore;
