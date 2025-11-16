// src/prompt/mod.rs
// Prompt building module - refactored into focused submodules

pub mod builders;
pub mod context;
pub mod types;
pub mod unified_builder; // Deprecated: kept for backward compatibility
pub mod utils;

// Primary exports
pub use builders::UnifiedPromptBuilder;
pub use types::{CodeElement, ErrorContext, QualityIssue};
