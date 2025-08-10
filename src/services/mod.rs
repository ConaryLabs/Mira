// src/services/mod.rs

pub mod chat;
pub mod memory;
pub mod context;
pub mod hybrid;
pub mod document;
pub mod midjourney_client;    // New - Midjourney API client
pub mod midjourney_personas;  // New - Persona-aware Midjourney engine

pub use chat::ChatService;
pub use crate::llm::schema::ChatResponse;
pub use memory::MemoryService;
pub use context::ContextService;
pub use hybrid::HybridMemoryService;
pub use document::DocumentService;

// New exports for Midjourney
pub use midjourney_client::MidjourneyClient;
pub use midjourney_personas::MidjourneyPersonaEngine;
