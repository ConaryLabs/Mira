// src/prompt/unified_builder.rs
// DEPRECATED: This file is kept for backward compatibility
// New code should use: use crate::prompt::{UnifiedPromptBuilder, CodeElement, ErrorContext, QualityIssue};
//
// This module re-exports everything from the refactored submodules.
// The original 612-line file has been split into focused modules:
// - types.rs: Type definitions (CodeElement, QualityIssue, ErrorContext)
// - utils.rs: Helper functions (is_code_related, language_from_extension)
// - context.rs: Context building functions (add_memory_context, add_code_intelligence_context, etc.)
// - builders.rs: Main prompt builders (UnifiedPromptBuilder)

// Re-export types
pub use crate::prompt::types::{CodeElement, ErrorContext, QualityIssue};

// Re-export the builder
pub use crate::prompt::builders::UnifiedPromptBuilder;

// Note: Individual helper functions (add_memory_context, etc.) are not re-exported
// as they were private. If you need them, import from crate::prompt::context or crate::prompt::utils
