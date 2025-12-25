//! MCP Tool modules - organized by domain for Claude Code augmentation
//!
//! NOTE: Some items are infrastructure for future features or external use.

#![allow(dead_code)] // Tool infrastructure (some items for future use)

pub mod analytics;
pub mod helpers;
pub mod build_intel;
pub mod code_intel;
pub mod corrections;
pub mod documents;
pub mod format;
pub mod hotline;
pub mod ingest;
pub mod git_intel;
pub mod goals;
pub mod mcp_history;
pub mod memory;
pub mod permissions;
pub mod proactive;
pub mod project;
pub mod response;
pub mod sessions;
pub mod tasks;
pub mod types;
pub mod work_state;

// Re-export from core primitives for backward compatibility
pub use crate::core::primitives::semantic;

// Re-export for use in main.rs
pub use types::*;
pub use crate::core::{SemanticSearch, COLLECTION_CODE};
pub use response::{to_mcp_err, json_response, text_response, vec_response, option_response, with_carousel_context};
