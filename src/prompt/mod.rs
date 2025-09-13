// src/prompt/mod.rs
// Prompt building module with unified builder

pub mod builder;
pub mod unified_builder;

// Primary export - the unified builder
pub use unified_builder::UnifiedPromptBuilder;

// Keep legacy exports for backward compatibility during transition
pub use builder::{
    build_system_prompt,
    build_conversation_context,
    extract_memory_themes,
};
