//! Core primitives - shared utilities for Mira
//!
//! This module provides common functionality:
//!
//! - **semantic**: Semantic search with Qdrant + Gemini
//! - **semantic_helpers**: Common embedding patterns
//! - **memory**: Memory fact operations (upsert, recall, forget)
//! - **artifacts**: Large output storage with deduplication
//! - **excerpts**: Smart text excerpting and UTF-8 helpers
//! - **secrets**: Secret detection and redaction
//! - **streaming**: SSE decoder
//! - **limits**: Shared constants and thresholds
//!
//! NOTE: Re-exports are provided for external/library use. Some may be unused internally.

#![allow(unused_imports)] // Re-exports for external use

pub mod limits;
pub mod secrets;
pub mod excerpts;
pub mod semantic;
pub mod semantic_helpers;
pub mod artifacts;
pub mod memory;
pub mod streaming;

// Re-export common types
pub use limits::*;

pub use secrets::{detect_secrets, redact_secrets, SecretMatch};

pub use excerpts::{
    create_diff_excerpt, create_grep_excerpt, create_smart_excerpt, safe_utf8_slice,
};

pub use semantic::{SearchResult, SemanticSearch, COLLECTION_CODE, COLLECTION_CONVERSATION, COLLECTION_DOCS};

pub use semantic_helpers::MetadataBuilder;

pub use artifacts::{ArtifactDecision, ArtifactRef, ArtifactStore, FetchResult};

pub use memory::{
    compute_final_score, compute_freshness, forget_memory_fact, make_memory_key,
    recall_memory_facts, recall_text_search, upsert_memory_fact,
    upsert_memory_fact_with_confidence, MemoryFact, MemoryScope, RecallConfig,
    SearchType, Validity,
};

pub use streaming::{SseDecoder, SseFrame};
