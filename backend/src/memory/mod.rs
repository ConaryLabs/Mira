//! Consolidated Memory Module
//!
//! Unified memory management with:
//! - Core: Configuration, traits, and types
//! - Features: Advanced processing (classification, embeddings, recall, summarization)
//! - Storage: SQLite and Qdrant backends
//! - Service: High-level orchestration
//! - Context: Types for context gathering and recall

pub mod context;
pub mod core;
pub mod features;
pub mod service;
pub mod storage;

// Re-export commonly used items
pub use self::core::{config::MemoryConfig, traits::*, types::*};

pub use self::context::{ContextConfig, ContextOracle, ContextRequest, GatheredContext};

pub use self::features::{
    decay::*,
    recall_engine::{RecallConfig, RecallContext, RecallEngine, SearchMode},
    session::*,
    summarization::SummarizationEngine,
};

pub use self::service::MemoryService;

// Storage backend exports
pub use self::storage::{qdrant, sqlite};
