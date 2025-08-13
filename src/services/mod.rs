// src/services/mod.rs
// Phase 3: Remove hybrid service module declaration

pub mod chat;
pub mod memory;
pub mod context;
pub mod document;

// Removed: pub mod hybrid;

pub use chat::ChatService;
pub use crate::llm::schema::ChatResponse;
pub use memory::MemoryService;
pub use context::ContextService;
pub use document::DocumentService;

// Removed: pub use hybrid::HybridMemoryService;
