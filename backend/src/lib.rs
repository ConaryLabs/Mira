// src/lib.rs
// Mira Power Suit - Memory and Intelligence Layer for Claude Code

// Core infrastructure
pub mod cache;
pub mod config;
pub mod utils;

// LLM types and embedding support
pub mod llm;

// Memory and knowledge
pub mod memory;
pub mod relationship;

// Intelligence layers
pub mod build;
pub mod git;
pub mod watcher;

// System integration
pub mod file_system;
pub mod project;
pub mod system;

// Export commonly used items
pub use config::CONFIG;

// Re-export llm types for convenience
pub use llm::{
    ArcEmbeddingProvider, ArcLlmProvider, EmbeddingHead, EmbeddingProvider, LlmProvider,
    LlmResponse, Message, OpenAIEmbeddingProvider, StubLlmProvider, TokenUsage,
};
