// src/memory/recall/mod.rs

//! Memory recall and context building for LLM interactions.
//! Provides strategies for retrieving relevant memories from both
//! SQLite (recent) and Qdrant (semantic) stores with multi-head support.

pub mod parallel_recall;
pub mod recall;

// Re-export core types
pub use recall::RecallContext;
pub use parallel_recall::{
    build_context_parallel,
    build_context_multi_head,
    build_context_adaptive,
    build_context_with_metrics,
    ParallelRecallMetrics,
    ScoredMemoryEntry,
};
