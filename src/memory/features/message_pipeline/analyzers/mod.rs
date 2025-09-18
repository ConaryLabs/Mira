// src/memory/features/message_pipeline/analyzers/mod.rs

//! Analysis modules for different content types
//! 
//! This module contains specialized analyzers for:
//! - `chat_analyzer` - Handles conversational content analysis  
//! - `code_analyzer` - Handles code content analysis (future)
//! - `unified` - Coordinates all analyzers and provides single interface

pub mod chat_analyzer;
pub mod code_analyzer; 
pub mod unified;

// Re-export main types for easier imports
pub use unified::{UnifiedAnalyzer, UnifiedAnalysisResult, AnalyzerConfig};
pub use chat_analyzer::{ChatAnalyzer, ChatAnalysisResult};
pub use code_analyzer::{CodeAnalyzer, CodeAnalysisResult};
