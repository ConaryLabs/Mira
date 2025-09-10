//! Tools module - extracted from WebSocket handlers
//! Contains all tool-related business logic

pub mod executor;
pub mod prompt_builder;
pub mod message_handler;
pub mod definitions;
pub mod file_context;
pub mod file_search;

// Re-export commonly used items
pub use executor::ToolExecutor;
pub use prompt_builder::ToolPromptBuilder;
pub use message_handler::ToolMessageHandler;
pub use definitions::get_enabled_tools;
pub mod document;
