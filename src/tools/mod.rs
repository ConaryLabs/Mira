// src/tools/mod.rs
// Tools module - simplified for Claude tool calling
// Tool schemas are in src/llm/structured/tool_schema.rs

pub mod executor;
pub mod types;
pub mod prompt_builder;
pub mod file_context;
pub mod file_search;
// document removed - in memory/features/
// definitions removed - using Claude tool schemas
// message_handler removed - using unified_handler

// Re-export
pub use executor::{ToolExecutor, ToolEvent};
pub use types::Tool;
pub use prompt_builder::ToolPromptBuilder;
