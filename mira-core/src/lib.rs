//! Mira Core - Shared utilities for Mira ecosystem
//!
//! This crate provides common functionality used by both `mira` (MCP server)
//! and `mira-chat`:
//!
//! - **secrets**: Secret detection and redaction
//! - **excerpts**: Smart text excerpting and UTF-8 helpers
//! - **limits**: Shared constants and thresholds
//! - **semantic** (feature): Semantic search with Qdrant + Gemini
//! - **artifacts** (feature): Large output storage with deduplication
//!
//! # Feature Flags
//!
//! - `secrets` - Secret detection helpers (lightweight)
//! - `excerpts` - Text excerpting and UTF-8 helpers (lightweight)
//! - `semantic` - Full semantic search (requires qdrant-client, reqwest)
//! - `artifacts` - Artifact storage (requires sha2)
//! - `full` - All features

pub mod limits;

#[cfg(feature = "secrets")]
pub mod secrets;

#[cfg(feature = "excerpts")]
pub mod excerpts;

#[cfg(feature = "semantic")]
pub mod semantic;

#[cfg(feature = "semantic")]
pub mod semantic_helpers;

#[cfg(feature = "artifacts")]
pub mod artifacts;

// Re-export common types
pub use limits::*;

#[cfg(feature = "secrets")]
pub use secrets::{detect_secrets, redact_secrets, SecretMatch};

#[cfg(feature = "excerpts")]
pub use excerpts::{
    create_diff_excerpt, create_grep_excerpt, create_smart_excerpt, safe_utf8_slice,
};

#[cfg(feature = "semantic")]
pub use semantic::{SearchResult, SemanticSearch, COLLECTION_CODE, COLLECTION_CONVERSATION, COLLECTION_DOCS};

#[cfg(feature = "semantic")]
pub use semantic_helpers::MetadataBuilder;

#[cfg(feature = "artifacts")]
pub use artifacts::{ArtifactDecision, ArtifactRef, ArtifactStore, FetchResult};
