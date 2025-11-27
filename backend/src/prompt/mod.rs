// backend/src/prompt/mod.rs
// Prompt building module - refactored into focused submodules
//
// Architecture:
// - builders.rs: UnifiedPromptBuilder for user-facing prompts (uses persona)
// - internal.rs: Technical prompts for JSON output, code generation, inner loops (no persona)
// - context.rs: Context building utilities
// - types.rs: Type definitions
// - utils.rs: Utility functions

pub mod builders;
pub mod context;
pub mod internal;
pub mod types;
pub mod unified_builder; // Deprecated: kept for backward compatibility
pub mod utils;

// Primary exports
pub use builders::UnifiedPromptBuilder;
pub use types::{CodeElement, ErrorContext, QualityIssue};
