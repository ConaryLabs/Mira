//! Core operations - shared implementation for MCP and Chat tools
//!
//! This module contains the pure business logic that both interfaces share.
//! MCP and Chat wrappers are thin layers that call into these operations.
//!
//! NOTE: Some items are infrastructure for future features or external use.

#![allow(dead_code)] // Core infrastructure (some items for future use)
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
//! │  │  file, shell, git, web, memory, mira, ...           ││
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

// Re-export commonly used primitives
pub use primitives::{
    SemanticSearch, COLLECTION_CODE,
    ArtifactStore, SseDecoder, ARTIFACT_THRESHOLD_BYTES,
};
