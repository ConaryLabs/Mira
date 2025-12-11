// src/lib.rs
// Mira Power Suit - Memory and Intelligence Layer for Claude Code

// Core infrastructure
pub mod cache;
pub mod config;
pub mod utils;

// LLM types and embedding support
mod llm;

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

// Re-export llm module with structured submodules for backward compatibility
pub use llm::{
    EmbeddingHead, EmbeddingProvider, LlmProvider, LlmResponse, Message,
    OpenAIEmbeddingProvider, StubLlmProvider, TokenUsage,
    ArcEmbeddingProvider, ArcLlmProvider,
};

// Backward-compatible module structure
pub mod llm_compat {
    //! LLM module with submodule structure for existing imports

    pub mod embeddings {
        pub use crate::llm::EmbeddingHead;
    }

    pub mod provider {
        pub use crate::llm::{
            ArcEmbeddingProvider, ArcLlmProvider, EmbeddingProvider, LlmProvider,
            LlmResponse, Message, OpenAIEmbeddingProvider, StubLlmProvider, TokenUsage,
        };
    }
}

pub mod prompt {
    //! Prompt module compatibility layer - re-exports from memory::features::prompts
    pub mod internal {
        pub use crate::memory::features::prompts::*;
    }
}

pub mod context_oracle {
    //! Context oracle compatibility layer - re-exports from memory::context
    pub use crate::memory::context::*;
}
