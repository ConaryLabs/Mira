// src/services/mod.rs
// Service module exports with file context service

pub mod chat;
pub mod context;
pub mod document;
pub mod memory;
pub mod summarization;
pub mod file_context;
pub mod chat_with_tools;

pub use chat::ChatService;
pub use context::ContextService;
pub use document::DocumentService;
pub use memory::MemoryService;
pub use summarization::SummarizationService;
pub use file_context::FileContextService;

pub use chat_with_tools::{
    get_enabled_tools,
    ChatResponseWithTools,
    ChatServiceToolExt,
    ChatServiceWithTools,
};
