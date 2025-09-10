// src/services/mod.rs
// Service module exports with file context service and refactored chat modules
// PHASE 3 UPDATE: Added FileSearchService export

pub mod chat;
pub mod context;
pub mod document;
pub mod summarization;

// Export main services
pub use chat::ChatService;
pub use context::ContextService;
pub use document::DocumentService;
pub use crate::memory::MemoryService;

// Export chat response types for compatibility (from refactored modules)
pub use chat::ChatResponse;

// Export tool-related functionality
