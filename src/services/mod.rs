// src/services/mod.rs
// Service module exports with file context service and refactored chat modules
// PHASE 3 UPDATE: Added FileSearchService export

pub mod chat;
pub mod context;
pub mod document;
pub mod memory;
pub mod summarization;
pub mod file_context;
pub mod chat_with_tools;
pub mod file_search; // PHASE 3 NEW: File search service

// Export main services
pub use chat::ChatService;
pub use context::ContextService;
pub use document::DocumentService;
pub use memory::MemoryService;
pub use summarization::SummarizationService;
pub use file_context::FileContextService;
pub use file_search::{FileSearchService, FileSearchParams, FileSearchResult, SearchMatchType}; // PHASE 3 NEW

// Export chat response types for compatibility (from refactored modules)
pub use chat::{ChatResponse, ChatConfig};

// Export tool-related functionality
pub use chat_with_tools::{
    get_enabled_tools,
    ChatResponseWithTools,
    ChatServiceToolExt,
    ChatServiceWithTools,
};
