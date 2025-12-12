// src/tools/mod.rs
// MCP Tool modules - organized by domain for Claude Code augmentation

pub mod analytics;
pub mod build_intel;
pub mod code_intel;
pub mod corrections;
pub mod documents;
pub mod format;
pub mod git_intel;
pub mod goals;
pub mod memory;
pub mod permissions;
pub mod proactive;
pub mod project;
pub mod response;
pub mod semantic;
pub mod sessions;
pub mod tasks;
pub mod types;

// Re-export for use in main.rs
pub use types::*;
pub use semantic::{SemanticSearch, COLLECTION_CODE};
pub use response::{to_mcp_err, json_response, text_response, vec_response, option_response};
