// src/memory/features/message_pipeline/analyzers/mod.rs

//! Message analysis components
//! 
//! - ChatAnalyzer: Handles sentiment, intent, topics for conversational content
//! - UnifiedAnalyzer: Coordinates analysis and routing decisions
//! 
//! Note: Detailed code analysis with AST parsing is handled by CodeIntelligenceService

mod chat_analyzer;
pub mod unified;  // Must be public for internal routing modules

pub use chat_analyzer::{ChatAnalyzer, ChatAnalysisResult};
pub use unified::{UnifiedAnalyzer, UnifiedAnalysisResult, RoutingDecision, AnalyzerConfig};
