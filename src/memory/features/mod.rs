// src/memory/features/mod.rs

//! Advanced memory features for analysis, classification, and processing.

pub mod decay;
pub mod embedding;
pub mod memory_types;
pub mod message_pipeline;  // NEW - unified pipeline replaces message_analyzer and classification
pub mod recall_engine;  // Consolidated recall/scoring/search
pub mod salience;
pub mod session;
pub mod summarization;
pub mod code_intelligence;

// Re-export commonly used types
pub use message_pipeline::{MessagePipeline, UnifiedAnalysis, RoutingDecision, PipelineConfig};
pub use recall_engine::{RecallEngine, RecallContext, RecallConfig, SearchMode, ScoredMemory};
pub use summarization::SummarizationEngine;
pub use memory_types::SummaryType;  // Import directly from memory_types, not through summarization
pub use session::SessionManager;
