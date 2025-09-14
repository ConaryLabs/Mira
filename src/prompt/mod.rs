// src/prompt/mod.rs
// Prompt building module with unified builder

pub mod builder;
pub mod unified_builder;

// Primary export - the unified builder
pub use unified_builder::UnifiedPromptBuilder;

