// src/api/ws/tools/mod.rs
// Module organization for extracted tool functionality

pub mod executor;
pub mod message_handler;
pub mod prompt_builder;

// Re-export key types for easy access
pub use executor::{
    ToolExecutor, 
    ToolConfig, 
    ToolChatRequest, 
    ToolChatResponse, 
    ToolEvent,
    ToolCallResult,
    ToolCallStatus,
    ResponseMetadata
};

pub use message_handler::{
    ToolMessageHandler,
    WsServerMessageWithTools
};

pub use prompt_builder::{
    ToolPromptBuilder,
    PromptTemplates
};
