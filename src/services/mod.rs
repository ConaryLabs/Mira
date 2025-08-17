// src/services/mod.rs
// Service module exports with file context service

pub mod chat;
pub mod context;
pub mod document;
pub mod memory;
pub mod summarization;
pub mod file_context;

pub use chat::ChatService;
pub use context::ContextService;
pub use document::DocumentService;
pub use memory::MemoryService;
pub use summarization::SummarizationService;
pub use file_context::FileContextService;
