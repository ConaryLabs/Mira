// src/memory/features/mod.rs

//! Advanced memory features for analysis, classification, and processing.

pub mod classification;
pub mod decay;
pub mod embedding;
pub mod memory_types;
pub mod message_analyzer;  // NEW!
pub mod salience;
pub mod scoring;
pub mod session;
pub mod summarization;

// Re-export commonly used types
pub use message_analyzer::{MessageAnalyzer, MessageAnalysis, AnalysisService};
