// src/memory/features/message_pipeline/analyzers/mod.rs

//! Message analysis components
//!
//! - ChatAnalyzer: Handles sentiment, intent, topics for conversational content
//! - UnifiedAnalyzer: Coordinates analysis and routing decisions
//!
//! Note: Detailed code analysis with AST parsing is handled by CodeIntelligenceService

mod chat_analyzer;
pub mod unified; // Must be public for internal routing modules

pub use chat_analyzer::{ChatAnalysisResult, ChatAnalyzer};
pub use unified::{AnalyzerConfig, RoutingDecision, UnifiedAnalysisResult, UnifiedAnalyzer};
