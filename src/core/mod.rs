//! Core operations - shared implementation for MCP and Chat tools
//!
//! This module contains the pure business logic that both interfaces share.
//! MCP and Chat wrappers are thin layers that call into these operations.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    src/core/                            │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
//! │  │  OpContext  │  │  CoreError  │  │  Shared Types   │  │
//! │  └─────────────┘  └─────────────┘  └─────────────────┘  │
//! │                                                         │
//! │  ┌─────────────────────────────────────────────────────┐│
//! │  │                    ops/                             ││
//! │  │  file, shell, git, web, memory, mira, council, ...  ││
//! │  └─────────────────────────────────────────────────────┘│
//! │                                                         │
//! │  ┌─────────────────────────────────────────────────────┐│
//! │  │                primitives/                          ││
//! │  │  semantic, memory, artifacts, excerpts, secrets, ...││
//! │  └─────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────┘
//!           │                           │
//!     ┌─────┴─────┐               ┌─────┴─────┐
//!     │  src/mcp  │               │ src/chat  │
//!     │  (tools)  │               │  (tools)  │
//!     └───────────┘               └───────────┘
//! ```

mod context;
mod error;
pub mod ops;
pub mod primitives;

pub use context::OpContext;
pub use error::{CoreError, CoreResult};

// Re-export commonly used primitives for convenience
pub use primitives::{
    // Semantic search
    SemanticSearch, SearchResult, COLLECTION_CODE, COLLECTION_CONVERSATION, COLLECTION_DOCS,
    // Memory
    MemoryFact, MemoryScope, RecallConfig, SearchType,
    make_memory_key, upsert_memory_fact, recall_memory_facts, recall_text_search, forget_memory_fact,
    // Artifacts
    ArtifactStore, ArtifactDecision, ArtifactRef, FetchResult,
    // Helpers
    MetadataBuilder, SseDecoder, SseFrame,
    // Utilities
    detect_secrets, redact_secrets, SecretMatch,
    create_smart_excerpt, create_diff_excerpt, create_grep_excerpt, safe_utf8_slice,
    // Constants
    ARTIFACT_THRESHOLD_BYTES, INLINE_MAX_BYTES, MAX_ARTIFACT_SIZE,
};
