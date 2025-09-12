// src/tools/mod.rs
// Tools module - extracted from WebSocket handlers
// Contains all tool-related business logic

pub mod executor;
pub mod prompt_builder;
pub mod message_handler;
pub mod definitions;
pub mod file_context;
pub mod file_search;
pub mod document;

// Re-export commonly used items
pub use executor::ToolExecutor;
pub use executor::ToolExecutorExt;
pub use prompt_builder::ToolPromptBuilder;
pub use message_handler::ToolMessageHandler;
pub use definitions::get_enabled_tools;
