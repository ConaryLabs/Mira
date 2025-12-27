//! Batch processing module for async bulk operations.
//!
//! Provides 50% cost savings via Gemini Batch API for:
//! - Memory compaction
//! - Document summarization
//! - Codebase analysis

mod worker;

pub use worker::{BatchWorker, BatchWorkerConfig, BatchJobType, create_compaction_job};
