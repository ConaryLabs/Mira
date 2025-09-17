// src/memory/features/mod.rs

//! Advanced memory features for analysis, classification, and processing.

pub mod classification;
pub mod decay;
pub mod embedding;
pub mod memory_types;
pub mod message_analyzer;
pub mod recall_engine;  // NEW - replaces scoring module
pub mod salience;
pub mod session;
pub mod summarization;

// Re-export commonly used types
pub use message_analyzer::{MessageAnalyzer, MessageAnalysis, AnalysisService};
pub use recall_engine::{RecallEngine, RecallContext, RecallConfig, SearchMode, ScoredMemory};
