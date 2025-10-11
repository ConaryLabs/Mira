//! Consolidated Memory Module
//! 
//! Unified memory management with:
//! - Core: Configuration, traits, and types
//! - Features: Advanced processing (classification, embeddings, recall, summarization)
//! - Storage: SQLite and Qdrant backends
//! - Service: High-level orchestration

pub mod core;
pub mod features;
pub mod storage;
pub mod service;

// Re-export commonly used items
pub use self::core::{
    config::MemoryConfig, 
    traits::*,
    types::*
};

pub use self::features::{
    decay::*,
    session::*,
    recall_engine::{RecallContext, RecallEngine, RecallConfig, SearchMode},
    summarization::SummarizationEngine,
};

pub use self::service::MemoryService;

// Storage backend exports
pub use self::storage::{sqlite, qdrant};
