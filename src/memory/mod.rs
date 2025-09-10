//! Consolidated Memory Module
//! 
//! Unified memory management with:
//! - Core: Configuration, traits, and types
//! - Features: Classification, scoring, decay, embeddings
//! - Recall: Context building and parallel retrieval
//! - Storage: SQLite and Qdrant backends

pub mod core;
pub mod features;
pub mod recall;
pub mod storage;
pub mod service;

// Re-export commonly used items
pub use self::core::{config::MemoryConfig, traits::*, types::*};
pub use self::features::{
    classification::*,
    decay::*,
    scoring::*,
    session::*,
};
pub use self::recall::{recall::RecallContext, parallel_recall::*};
pub use self::service::MemoryService;

// Temporary compatibility re-exports
pub use self::storage::sqlite;
pub use self::storage::qdrant;
pub use self::core::traits;
pub use self::core::types;
pub use self::features::decay;
pub use self::features::decay_scheduler;
