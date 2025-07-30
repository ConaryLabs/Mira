// src/services/mod.rs

pub mod chat;
pub mod memory;
pub mod context;

pub use chat::ChatService;
pub use crate::llm::schema::ChatResponse;
pub use memory::MemoryService;
pub use context::ContextService;
