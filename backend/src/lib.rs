// src/lib.rs
// Mira Power Suit - Memory and Intelligence Layer for Claude Code

// Core infrastructure
pub mod cache;
pub mod config;
pub mod utils;

// Stub modules for compatibility (being cleaned up)
pub mod llm_stubs;

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

// Compatibility layer re-exports
pub mod llm {
    //! LLM module compatibility layer
    //! Claude Code handles actual LLM calls; we provide embedding support

    pub mod embeddings {
        pub use crate::llm_stubs::EmbeddingHead;
    }

    pub mod provider {
        pub use crate::llm_stubs::{
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
