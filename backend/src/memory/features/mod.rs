// src/memory/features/mod.rs

//! Advanced memory features for analysis, classification, and processing.

pub mod code_intelligence;
pub mod decay;
pub mod document_processing;
pub mod embedding;
pub mod memory_types;
pub mod message_pipeline; // Unified pipeline replaces message_analyzer and classification
pub mod prompts; // System prompts for LLM-based features
pub mod recall_engine; // Consolidated recall/scoring/search
pub mod salience;
pub mod session;
pub mod summarization;

// Re-export commonly used types
pub use document_processing::{
    DocumentChunk, DocumentMetadata, DocumentProcessor, ProcessedDocument, ProcessingStatus,
};
pub use memory_types::SummaryType; // Import directly from memory_types, not through summarization
pub use message_pipeline::{MessagePipeline, PipelineConfig, RoutingDecision, UnifiedAnalysis};
pub use recall_engine::{RecallConfig, RecallContext, RecallEngine, ScoredMemory, SearchMode};
pub use session::SessionManager;
pub use summarization::SummarizationEngine;
